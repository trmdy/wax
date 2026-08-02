//! NDJSON protocol v0 server.
//!
//! Operations that can block run on detached worker threads. Cancellation is
//! cooperative at wax checkpoints. If calamine is blocked when an open request
//! times out or is cancelled, the server answers immediately, forgets the
//! request, and deliberately abandons that worker; any late result is ignored.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use wax_core::CellOverride;
#[cfg(test)]
use wax_core::SizeOverrides;
use wax_eval::{FormulaWorkbook, RecalcOutcome, DEFAULT_EVAL_BUDGET};
use wax_proto::{
    parse_request, server_caps, CancelResponse, CloseResponse, ErrorCode, ErrorResponse,
    ExportResponse, MetaResponse, OpenResponse, RecalcCell, RecalcResponse, Request, Response,
    SheetSummary, StatsResponse, VersionResponse, WindowResponse, WireCell, PROTO_VERSION,
};
use wax_read::{read_with_deadline, CalamineReader, ReaderOptions};
use wax_store::{Window, WindowCell, WorkbookStore};
use wax_write::ExportOutcome;

use crate::peak_rss_bytes;

const EVENT_POLL: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub idle_timeout_ms: u64,
    pub max_handles: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idle_timeout_ms: 300_000,
            max_handles: 16,
        }
    }
}

pub fn run(config: Config) -> io::Result<()> {
    install_sigterm_handler()?;

    let (input_tx, input_rx) = mpsc::channel();
    thread::spawn(move || read_input(input_tx));

    let (work_tx, work_rx) = mpsc::channel();
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let mut state = State::new(config);

    loop {
        if termination_requested() {
            state.cancel_all();
            return Ok(());
        }

        let now = Instant::now();
        state.expire_handles(now);
        for response in state.expire_requests(now) {
            write_response(&mut output, &response)?;
        }

        while let Ok(result) = work_rx.try_recv() {
            if let Some(response) = state.complete(result) {
                write_response(&mut output, &response)?;
            }
        }

        match input_rx.recv_timeout(EVENT_POLL) {
            Ok(Input::Line(line)) => {
                let response = match parse_request(&line) {
                    Ok(request) => dispatch(&mut state, request, &work_tx),
                    Err(error) => Some(error_response(error.id, ErrorCode::BadRequest, error.msg)),
                };
                if let Some(response) = response {
                    write_response(&mut output, &response)?;
                }
            }
            Ok(Input::Eof) => {
                state.cancel_all();
                return Ok(());
            }
            Ok(Input::Error(error)) => {
                state.cancel_all();
                return Err(error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                state.cancel_all();
                return Ok(());
            }
        }
    }
}

enum Input {
    Line(String),
    Eof,
    Error(io::Error),
}

fn read_input(sender: mpsc::Sender<Input>) {
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    loop {
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => {
                let _ = sender.send(Input::Eof);
                return;
            }
            Ok(_) => {
                if sender.send(Input::Line(line)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Input::Error(error));
                return;
            }
        }
    }
}

fn write_response(output: &mut impl Write, response: &Response) -> io::Result<()> {
    serde_json::to_writer(&mut *output, response).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

struct State {
    idle_timeout: Duration,
    max_handles: usize,
    next_handle: u64,
    pending_opens: usize,
    handles: HashMap<String, Handle>,
    expired_handles: HashSet<String>,
    in_flight: HashMap<u64, InFlight>,
}

impl State {
    fn new(config: Config) -> Self {
        Self {
            idle_timeout: Duration::from_millis(config.idle_timeout_ms),
            max_handles: config.max_handles,
            next_handle: 1,
            pending_opens: 0,
            handles: HashMap::new(),
            expired_handles: HashSet::new(),
            in_flight: HashMap::new(),
        }
    }

    fn expire_handles(&mut self, now: Instant) {
        let expired = self
            .handles
            .iter()
            .filter(|(_, handle)| now.duration_since(handle.last_used) >= self.idle_timeout)
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        for name in expired {
            self.handles.remove(&name);
            self.expired_handles.insert(name);
        }
    }

    fn expire_requests(&mut self, now: Instant) -> Vec<Response> {
        let expired = self
            .in_flight
            .iter()
            .filter_map(|(id, request)| {
                request
                    .deadline
                    .filter(|deadline| now >= *deadline)
                    .map(|_| *id)
            })
            .collect::<Vec<_>>();
        expired
            .into_iter()
            .filter_map(|id| {
                let request = self.remove_in_flight(id)?;
                let was_cancelled = request.cancel.swap(true, Ordering::AcqRel);
                Some(if was_cancelled {
                    error_response(Some(id), ErrorCode::Cancelled, "request cancelled")
                } else {
                    error_response(Some(id), ErrorCode::Timeout, "operation timed out")
                })
            })
            .collect()
    }

    fn complete(&mut self, result: WorkResult) -> Option<Response> {
        let request = self.remove_in_flight(result.id)?;
        if request.cancel.load(Ordering::Acquire) {
            return Some(error_response(
                Some(result.id),
                ErrorCode::Cancelled,
                "request cancelled",
            ));
        }
        if request
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            request.cancel.store(true, Ordering::Release);
            return Some(error_response(
                Some(result.id),
                ErrorCode::Timeout,
                "operation timed out",
            ));
        }

        Some(match result.outcome {
            Err(failure) => error_response(Some(result.id), failure.code, failure.msg),
            Ok(WorkPayload::Open(opened)) => {
                let handle = format!("h{}", self.next_handle);
                self.next_handle = self.next_handle.saturating_add(1);
                let sheets = sheet_summaries(&opened.store);
                self.handles.insert(
                    handle.clone(),
                    Handle {
                        store: opened.store,
                        formula: opened.formula,
                        truncated: opened.truncated,
                        warnings: opened.warnings.clone(),
                        last_used: Instant::now(),
                    },
                );
                Response::Open(OpenResponse {
                    id: result.id,
                    ok: true,
                    proto: PROTO_VERSION,
                    caps: server_caps(),
                    handle,
                    truncated: opened.truncated,
                    sheets,
                    warnings: opened.warnings,
                })
            }
            Ok(WorkPayload::Window(window)) => Response::Window(window_response(result.id, window)),
            Ok(WorkPayload::Export(outcome)) => Response::Export(ExportResponse {
                id: result.id,
                ok: true,
                bytes: outcome.bytes,
                applied: outcome.applied,
                dropped: outcome.dropped,
            }),
            Ok(WorkPayload::Recalc(outcome)) => Response::Recalc(RecalcResponse {
                id: result.id,
                ok: true,
                changed: outcome
                    .changed
                    .into_iter()
                    .map(|cell| RecalcCell {
                        sheet: cell.sheet,
                        r: cell.r,
                        c: cell.c,
                        v: cell.v,
                        d: cell.d,
                        e: cell.e,
                    })
                    .collect(),
                evaluated: outcome.evaluated,
                skipped: outcome.skipped,
                truncated: outcome.truncated,
                warnings: outcome.warnings,
            }),
        })
    }

    fn remove_in_flight(&mut self, id: u64) -> Option<InFlight> {
        let request = self.in_flight.remove(&id)?;
        if request.kind == WorkKind::Open {
            self.pending_opens = self.pending_opens.saturating_sub(1);
        }
        Some(request)
    }

    fn cancel(&mut self, target: u64) -> bool {
        let Some(request) = self.in_flight.get(&target) else {
            return false;
        };
        request.cancel.store(true, Ordering::Release);
        true
    }

    fn cancel_all(&mut self) {
        for (_, request) in self.in_flight.drain() {
            request.cancel.store(true, Ordering::Release);
        }
        self.pending_opens = 0;
    }

    fn touch(&mut self, name: &str) -> Result<HandleSnapshot, Failure> {
        self.expire_handles(Instant::now());
        let Some(handle) = self.handles.get_mut(name) else {
            let msg = if self.expired_handles.contains(name) {
                format!("handle {name:?} expired")
            } else {
                format!("unknown handle {name:?}")
            };
            return Err(Failure::new(ErrorCode::BadHandle, msg));
        };
        handle.last_used = Instant::now();
        Ok(HandleSnapshot {
            store: Arc::clone(&handle.store),
            formula: Arc::clone(&handle.formula),
            truncated: handle.truncated,
            warnings: handle.warnings.clone(),
        })
    }
}

struct Handle {
    store: Arc<WorkbookStore>,
    formula: Arc<FormulaWorkbook>,
    truncated: bool,
    warnings: Vec<String>,
    last_used: Instant,
}

struct HandleSnapshot {
    store: Arc<WorkbookStore>,
    formula: Arc<FormulaWorkbook>,
    truncated: bool,
    warnings: Vec<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WorkKind {
    Open,
    Other,
}

struct InFlight {
    cancel: Arc<AtomicBool>,
    deadline: Option<Instant>,
    kind: WorkKind,
}

struct WorkResult {
    id: u64,
    outcome: Result<WorkPayload, Failure>,
}

enum WorkPayload {
    Open(OpenedWorkbook),
    Window(Window),
    Export(ExportOutcome),
    Recalc(RecalcOutcome),
}

struct OpenedWorkbook {
    store: Arc<WorkbookStore>,
    formula: Arc<FormulaWorkbook>,
    truncated: bool,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct Failure {
    code: ErrorCode,
    msg: String,
}

impl Failure {
    fn new(code: ErrorCode, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
        }
    }
}

fn dispatch(
    state: &mut State,
    request: Request,
    worker_sender: &mpsc::Sender<WorkResult>,
) -> Option<Response> {
    state.expire_handles(Instant::now());
    let id = request.id();
    if state.in_flight.contains_key(&id) {
        return Some(error_response(
            Some(id),
            ErrorCode::BadRequest,
            format!("request id {id} is already in flight"),
        ));
    }

    match request {
        Request::Version { id } => Some(Response::Version(VersionResponse {
            id,
            ok: true,
            proto: PROTO_VERSION,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            caps: server_caps(),
        })),
        Request::Open {
            id,
            path,
            max_cells,
            max_bytes,
            timeout_ms,
        } => {
            if state.handles.len().saturating_add(state.pending_opens) >= state.max_handles {
                return Some(error_response(
                    Some(id),
                    ErrorCode::BadRequest,
                    format!("maximum of {} open handles reached", state.max_handles),
                ));
            }
            let max_cells = match usize::try_from(max_cells) {
                Ok(value) => value,
                Err(_) => {
                    return Some(error_response(
                        Some(id),
                        ErrorCode::BadRequest,
                        "maxCells exceeds platform range",
                    ))
                }
            };
            let cancel = Arc::new(AtomicBool::new(false));
            state.pending_opens += 1;
            state.in_flight.insert(
                id,
                InFlight {
                    cancel: Arc::clone(&cancel),
                    deadline: Some(
                        Instant::now()
                            .checked_add(Duration::from_millis(timeout_ms))
                            .unwrap_or_else(Instant::now),
                    ),
                    kind: WorkKind::Open,
                },
            );
            let sender = worker_sender.clone();
            thread::spawn(move || {
                let outcome = open_workbook(
                    PathBuf::from(path),
                    max_cells,
                    max_bytes,
                    timeout_ms,
                    &cancel,
                )
                .map(WorkPayload::Open);
                let _ = sender.send(WorkResult { id, outcome });
            });
            None
        }
        Request::Meta { id, handle } => Some(match state.touch(&handle) {
            Ok(handle) => Response::Meta(MetaResponse {
                id,
                ok: true,
                truncated: handle.truncated,
                sheets: sheet_summaries(&handle.store),
                warnings: handle.warnings,
            }),
            Err(failure) => error_response(Some(id), failure.code, failure.msg),
        }),
        Request::Window {
            id,
            handle,
            sheet,
            r0,
            c0,
            nr,
            nc,
        } => {
            let handle = match state.touch(&handle) {
                Ok(handle) => handle,
                Err(failure) => return Some(error_response(Some(id), failure.code, failure.msg)),
            };
            if handle.store.sheet_meta(sheet).is_none() {
                return Some(error_response(
                    Some(id),
                    ErrorCode::BadRequest,
                    format!("sheet index {sheet} is out of range"),
                ));
            }
            let cancel = Arc::new(AtomicBool::new(false));
            state.in_flight.insert(
                id,
                InFlight {
                    cancel: Arc::clone(&cancel),
                    deadline: None,
                    kind: WorkKind::Other,
                },
            );
            let sender = worker_sender.clone();
            thread::spawn(move || {
                let outcome = if cancel.load(Ordering::Acquire) {
                    Err(Failure::new(ErrorCode::Cancelled, "request cancelled"))
                } else {
                    handle
                        .store
                        .window(sheet, r0, c0, nr, nc)
                        .map(|mut window| {
                            handle.formula.apply_to_window(
                                sheet,
                                window.r0,
                                window.c0,
                                &mut window.rows,
                            );
                            WorkPayload::Window(window)
                        })
                        .ok_or_else(|| {
                            Failure::new(
                                ErrorCode::BadRequest,
                                format!("sheet index {sheet} is out of range"),
                            )
                        })
                };
                let _ = sender.send(WorkResult { id, outcome });
            });
            None
        }
        Request::Export {
            id,
            handle,
            format,
            out,
            sheet,
            overrides,
            size_overrides,
        } => {
            let handle = match state.touch(&handle) {
                Ok(handle) => handle,
                Err(failure) => return Some(error_response(Some(id), failure.code, failure.msg)),
            };
            let format = if format.eq_ignore_ascii_case("csv") {
                ExportFormat::Csv
            } else if format.eq_ignore_ascii_case("xlsx") {
                ExportFormat::Xlsx
            } else {
                return Some(error_response(
                    Some(id),
                    ErrorCode::Unsupported,
                    format!("export format {format:?} is unsupported in protocol v0"),
                ));
            };
            if handle.store.sheet_meta(sheet).is_none() {
                return Some(error_response(
                    Some(id),
                    ErrorCode::BadRequest,
                    format!("sheet index {sheet} is out of range"),
                ));
            }
            let cancel = Arc::new(AtomicBool::new(false));
            state.in_flight.insert(
                id,
                InFlight {
                    cancel: Arc::clone(&cancel),
                    deadline: None,
                    kind: WorkKind::Other,
                },
            );
            let sender = worker_sender.clone();
            thread::spawn(move || {
                let evaluation_overrides = formula_overrides_for_export(format, sheet, &overrides);
                let evaluation = handle
                    .formula
                    .recalculate(&handle.store, &evaluation_overrides, DEFAULT_EVAL_BUDGET)
                    .map_err(eval_failure);
                let outcome = evaluation.and_then(|evaluation| {
                    match format {
                        ExportFormat::Csv => wax_write::write_csv_with_evaluated_overrides(
                            &handle.store,
                            sheet,
                            Path::new(&out),
                            &overrides,
                            &size_overrides,
                            &evaluation.all_evaluated,
                            &cancel,
                        )
                        .map_err(writer_failure),
                        ExportFormat::Xlsx => wax_write::write_xlsx_with_evaluated_overrides(
                            &handle.store,
                            Path::new(&out),
                            &overrides,
                            &size_overrides,
                            &evaluation.all_evaluated,
                            &cancel,
                        )
                        .map_err(writer_failure),
                    }
                    .map(|mut outcome| {
                        outcome.dropped.extend(evaluation.warnings);
                        let unevaluated = if matches!(format, ExportFormat::Csv) {
                            handle.formula.unevaluated_formulas_on_sheet(sheet)
                        } else {
                            handle.formula.unevaluated_formulas()
                        };
                        if unevaluated > 0 {
                            outcome.dropped.push(format!(
                                "formulas kept file-cached values ({unevaluated} unevaluated)"
                            ));
                        }
                        outcome.dropped.extend(handle.warnings);
                        WorkPayload::Export(outcome)
                    })
                });
                let _ = sender.send(WorkResult { id, outcome });
            });
            None
        }
        Request::Recalc {
            id,
            handle,
            overrides,
        } => {
            let handle = match state.touch(&handle) {
                Ok(handle) => handle,
                Err(failure) => return Some(error_response(Some(id), failure.code, failure.msg)),
            };
            let cancel = Arc::new(AtomicBool::new(false));
            state.in_flight.insert(
                id,
                InFlight {
                    cancel: Arc::clone(&cancel),
                    deadline: None,
                    kind: WorkKind::Other,
                },
            );
            let sender = worker_sender.clone();
            thread::spawn(move || {
                let outcome = if cancel.load(Ordering::Acquire) {
                    Err(Failure::new(ErrorCode::Cancelled, "request cancelled"))
                } else {
                    handle
                        .formula
                        .recalculate(&handle.store, &overrides, DEFAULT_EVAL_BUDGET)
                        .map(WorkPayload::Recalc)
                        .map_err(eval_failure)
                };
                let _ = sender.send(WorkResult { id, outcome });
            });
            None
        }
        Request::Close { id, handle } => {
            if state.handles.remove(&handle).is_some() {
                Some(Response::Close(CloseResponse { id, ok: true }))
            } else {
                let msg = if state.expired_handles.contains(&handle) {
                    format!("handle {handle:?} expired")
                } else {
                    format!("unknown handle {handle:?}")
                };
                Some(error_response(Some(id), ErrorCode::BadHandle, msg))
            }
        }
        Request::Cancel { id, target } => {
            let found = state.cancel(target);
            if found {
                let _ = worker_sender.send(WorkResult {
                    id: target,
                    outcome: Err(Failure::new(ErrorCode::Cancelled, "request cancelled")),
                });
            }
            Some(Response::Cancel(CancelResponse {
                id,
                ok: true,
                found,
            }))
        }
        Request::Stats { id } => {
            let store_bytes = state.handles.values().fold(0_u64, |total, handle| {
                total.saturating_add(u64::try_from(handle.store.approx_bytes()).unwrap_or(u64::MAX))
            });
            Some(Response::Stats(StatsResponse {
                id,
                ok: true,
                peak_rss_bytes: peak_rss_bytes().unwrap_or(0),
                handles: state.handles.len(),
                store_bytes,
            }))
        }
    }
}

#[derive(Clone, Copy)]
enum ExportFormat {
    Xlsx,
    Csv,
}

fn formula_overrides_for_export(
    format: ExportFormat,
    sheet: u32,
    overrides: &[CellOverride],
) -> Vec<CellOverride> {
    overrides
        .iter()
        .filter(|entry| matches!(format, ExportFormat::Xlsx) || entry.sheet == sheet)
        .cloned()
        .collect()
}

fn open_workbook(
    path: PathBuf,
    max_cells: usize,
    max_bytes: u64,
    timeout_ms: u64,
    cancel: &AtomicBool,
) -> Result<OpenedWorkbook, Failure> {
    checkpoint(cancel)?;
    if let Ok(metadata) = path.metadata() {
        if metadata.len() > max_bytes {
            return Err(Failure::new(
                ErrorCode::TooLarge,
                format!(
                    "input is {} bytes, exceeding maxBytes {max_bytes}",
                    metadata.len()
                ),
            ));
        }
    }
    checkpoint(cancel)?;

    let formula_supported = supports_formula_container(&path);
    let document = read_document_with_deadline(
        &path,
        ReaderOptions {
            max_cells,
            timeout_ms,
            max_bytes,
            ..ReaderOptions::default()
        },
    );
    checkpoint(cancel)?;
    if !document.ok {
        let error = document.error.unwrap_or_else(|| wax_core::DumpError {
            code: ErrorCode::Internal.as_str().to_owned(),
            msg: "reader returned a failure without error details".to_owned(),
        });
        let code = ErrorCode::from_code(&error.code).unwrap_or(ErrorCode::Internal);
        return Err(Failure::new(code, error.msg));
    }

    let truncated = document.truncated;
    let mut warnings = document.warnings.clone();
    let store = Arc::new(WorkbookStore::from_document(document));
    let (formula, evaluation) = if formula_supported {
        FormulaWorkbook::open(&store, DEFAULT_EVAL_BUDGET)
    } else {
        FormulaWorkbook::file_cached(&store)
    };
    warnings.extend(evaluation.warnings);
    let formula = Arc::new(formula);
    checkpoint(cancel)?;
    Ok(OpenedWorkbook {
        store,
        formula,
        truncated,
        warnings,
    })
}

fn supports_formula_container(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "xlsx" | "xlsm"))
}

fn read_document_with_deadline(path: &Path, options: ReaderOptions) -> wax_core::Document {
    read_with_deadline(CalamineReader, path, options)
}

fn checkpoint(cancel: &AtomicBool) -> Result<(), Failure> {
    if cancel.load(Ordering::Acquire) {
        Err(Failure::new(ErrorCode::Cancelled, "request cancelled"))
    } else {
        Ok(())
    }
}

fn sheet_summaries(store: &WorkbookStore) -> Vec<SheetSummary> {
    (0..store.sheet_count())
        .filter_map(|sheet| {
            let meta = store.sheet_meta(sheet)?;
            Some(SheetSummary {
                name: meta.name,
                rows: meta.rows,
                cols: meta.cols,
                truncated: meta.truncated,
                col_infos: store.sheet_col_infos(sheet).unwrap_or_default().to_vec(),
                row_infos: store.sheet_row_infos(sheet).unwrap_or_default().to_vec(),
                // The sheetSizeInfos contract promises concrete defaults:
                // container declarations when present, Excel fallbacks
                // otherwise, so consumers never carry their own.
                default_row_height: meta
                    .default_row_height
                    .unwrap_or(wax_core::DEFAULT_ROW_HEIGHT_POINTS),
                default_col_width: meta
                    .default_col_width
                    .unwrap_or(wax_core::DEFAULT_COL_WIDTH_CHARS),
                frozen_rows: meta.frozen_rows,
                frozen_cols: meta.frozen_cols,
            })
        })
        .collect()
}

fn window_response(id: u64, window: Window) -> WindowResponse {
    WindowResponse {
        id,
        ok: true,
        sheet: window.sheet,
        r0: window.r0,
        c0: window.c0,
        nr: window.nr,
        nc: window.nc,
        rows: window
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(|cell| cell.map(wire_cell)).collect())
            .collect(),
        merges: window.merges,
    }
}

fn wire_cell(cell: WindowCell) -> WireCell {
    WireCell {
        t: cell.t,
        v: cell.v,
        d: cell.d,
        f: cell.f,
        fmt: cell.fmt,
        e: cell.e.then_some(true),
    }
}

#[cfg(test)]
fn export_csv(
    store: &WorkbookStore,
    sheet: u32,
    out: &Path,
    overrides: &[CellOverride],
    sizes: &SizeOverrides,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, Failure> {
    wax_write::write_csv_with_overrides(store, sheet, out, overrides, sizes, cancel)
        .map_err(writer_failure)
}

fn writer_failure(error: wax_write::WriteError) -> Failure {
    Failure::new(
        ErrorCode::from_code(&error.code).unwrap_or(ErrorCode::Internal),
        error.msg,
    )
}

fn eval_failure(error: wax_eval::EvalError) -> Failure {
    Failure::new(
        ErrorCode::from_code(error.code).unwrap_or(ErrorCode::Internal),
        error.msg,
    )
}

fn error_response(id: Option<u64>, code: ErrorCode, msg: impl Into<String>) -> Response {
    Response::Error(ErrorResponse::new(id, code, msg))
}

#[cfg(unix)]
static TERMINATE: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_sigterm(_: libc::c_int) {
    TERMINATE.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
fn install_sigterm_handler() -> io::Result<()> {
    TERMINATE.store(false, Ordering::Relaxed);
    // SAFETY: the structure is initialized before use; the handler only stores
    // to a lock-free atomic, which is async-signal-safe.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_sigterm as *const () as usize;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn termination_requested() -> bool {
    TERMINATE.load(Ordering::Relaxed)
}

#[cfg(not(unix))]
fn install_sigterm_handler() -> io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn termination_requested() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use wax_core::{Cell, CellType, CellValue, Document, Sheet};

    fn store_with_cell(cell: Cell) -> WorkbookStore {
        WorkbookStore::from_document(Document::success(
            "0.1.0",
            "test.xlsx",
            vec![Sheet {
                col_infos: Vec::new(),
                row_infos: Vec::new(),
                default_row_height: None,
                default_col_width: None,
                name: "Sheet1".to_owned(),
                index: 0,
                rows: 1,
                cols: 1,
                truncated: false,
                merges: Vec::new(),
                cells: vec![cell],
                frozen_rows: 0,
                frozen_cols: 0,
            }],
            Vec::new(),
        ))
    }

    #[test]
    fn csv_uses_display_and_rfc_4180_quoting() {
        let store = store_with_cell(Cell {
            s: None,
            r: 0,
            c: 0,
            t: CellType::S,
            v: Some(CellValue::Text("raw".to_owned())),
            d: Some("say \"hello\",\nfriend".to_owned()),
            f: None,
            fmt: None,
        });
        let temp = tempfile::tempdir().expect("temp directory");
        let out = temp.path().join("quoted.csv");
        export_csv(
            &store,
            0,
            &out,
            &[],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("export should work");
        assert_eq!(
            std::fs::read(&out).expect("csv should be readable"),
            b"\"say \"\"hello\"\",\nfriend\"\r\n"
        );
    }

    #[test]
    fn csv_raw_values_use_protocol_spellings() {
        let store = WorkbookStore::from_document(Document::success(
            "0.1.0",
            "test.xlsx",
            vec![Sheet {
                col_infos: Vec::new(),
                row_infos: Vec::new(),
                default_row_height: None,
                default_col_width: None,
                name: "Sheet1".to_owned(),
                index: 0,
                rows: 1,
                cols: 3,
                truncated: false,
                merges: Vec::new(),
                frozen_rows: 0,
                frozen_cols: 0,
                cells: vec![
                    Cell {
                        s: None,
                        r: 0,
                        c: 0,
                        t: CellType::N,
                        v: Some(CellValue::Number(1.25)),
                        d: None,
                        f: None,
                        fmt: None,
                    },
                    Cell {
                        s: None,
                        r: 0,
                        c: 1,
                        t: CellType::B,
                        v: Some(CellValue::Bool(false)),
                        d: None,
                        f: None,
                        fmt: None,
                    },
                    Cell {
                        s: None,
                        r: 0,
                        c: 2,
                        t: CellType::N,
                        v: None,
                        d: None,
                        f: Some("A1".to_owned()),
                        fmt: None,
                    },
                ],
            }],
            Vec::new(),
        ));
        let temp = tempfile::tempdir().expect("temp directory");
        let out = temp.path().join("raw.csv");
        export_csv(
            &store,
            0,
            &out,
            &[],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("export should work");
        assert_eq!(
            std::fs::read(&out).expect("csv should be readable"),
            b"1.25,FALSE,\r\n"
        );
    }

    #[test]
    fn cancellation_checkpoint_is_cooperative() {
        let cancel = AtomicBool::new(true);
        assert_eq!(
            checkpoint(&cancel).expect_err("cancel should fail").code,
            ErrorCode::Cancelled
        );
    }

    #[test]
    fn csv_formula_evaluation_ignores_other_sheet_overrides() {
        let overrides = vec![
            CellOverride {
                sheet: 0,
                r: 0,
                c: 0,
                v: Some(CellValue::Number(1.0)),
            },
            CellOverride {
                sheet: 1,
                r: 0,
                c: 0,
                v: Some(CellValue::Number(2.0)),
            },
        ];
        assert_eq!(
            formula_overrides_for_export(ExportFormat::Csv, 0, &overrides),
            overrides[..1]
        );
        assert_eq!(
            formula_overrides_for_export(ExportFormat::Xlsx, 0, &overrides),
            overrides
        );
    }

    #[test]
    fn cancelled_csv_export_leaves_no_output_file() {
        let store = store_with_cell(Cell {
            s: None,
            r: 0,
            c: 0,
            t: CellType::S,
            v: Some(CellValue::Text("value".to_owned())),
            d: None,
            f: None,
            fmt: None,
        });
        let temp = tempfile::tempdir().expect("temp directory");
        let out = temp.path().join("cancelled.csv");
        let error = export_csv(
            &store,
            0,
            &out,
            &[],
            &SizeOverrides::default(),
            &AtomicBool::new(true),
        )
        .expect_err("cancelled export should fail");
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(!out.exists(), "cancelled export left an output file");
    }
}
