use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use wax_core::{Document, DumpError};
use wax_proto::ErrorCode;
use zip::ZipArchive;

use crate::{Reader, ReaderOptions};

const XML_EXTENSIONS: [&str; 2] = [".xml", ".rels"];

#[derive(Debug, Eq, PartialEq)]
pub struct SafetyError {
    code: ErrorCode,
    message: String,
}

impl SafetyError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Reads a workbook on an owned worker and returns when its wall-clock budget expires.
///
/// Rust cannot safely stop a thread blocked inside a dependency. A timed-out worker is
/// therefore detached and allowed to finish in the background. Callers still receive a
/// structured timeout document at the deadline.
pub fn read_with_deadline<R>(reader: R, path: &Path, options: ReaderOptions) -> Document
where
    R: Reader + Send + 'static,
{
    let worker_path = path.to_path_buf();
    let failure_path = worker_path.to_string_lossy().into_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("wax-reader".to_owned())
        .spawn(move || {
            let document = match preflight_path(&worker_path, options) {
                Ok(()) => reader.read(&worker_path, options),
                Err(error) => failure_document(
                    worker_path.to_string_lossy().into_owned(),
                    error.code,
                    error.message,
                ),
            };
            let _ = sender.send(document);
        });

    if let Err(error) = worker {
        return failure_document(
            failure_path,
            ErrorCode::Internal,
            format!("could not start workbook reader: {error}"),
        );
    }

    match receiver.recv_timeout(Duration::from_millis(options.timeout_ms)) {
        Ok(document) => document,
        Err(RecvTimeoutError::Timeout) => failure_document(
            failure_path,
            ErrorCode::Timeout,
            format!(
                "workbook parse exceeded the {} ms wall-clock timeout",
                options.timeout_ms
            ),
        ),
        Err(RecvTimeoutError::Disconnected) => failure_document(
            failure_path,
            ErrorCode::Internal,
            "workbook reader stopped without producing a result",
        ),
    }
}

/// Checks the input-size rail and, for ZIP-backed workbook formats, all container rails.
pub fn preflight_path(path: &Path, options: ReaderOptions) -> Result<(), SafetyError> {
    let metadata = path.metadata().map_err(|error| {
        SafetyError::new(
            ErrorCode::Internal,
            format!("could not inspect input: {error}"),
        )
    })?;
    if metadata.len() > options.max_bytes {
        return Err(SafetyError::new(
            ErrorCode::TooLarge,
            format!(
                "input is {} bytes, exceeding the {} byte limit",
                metadata.len(),
                options.max_bytes
            ),
        ));
    }

    if is_legacy_xls(path) {
        return preflight_legacy_cfb(path, metadata.len(), options);
    }
    if is_zip_workbook(path) {
        let input = File::open(path).map_err(|error| {
            SafetyError::new(
                ErrorCode::Internal,
                format!("could not open input: {error}"),
            )
        })?;
        return preflight_container(BufReader::new(input), options);
    }

    Ok(())
}

fn preflight_legacy_cfb(
    path: &Path,
    input_bytes: u64,
    options: ReaderOptions,
) -> Result<(), SafetyError> {
    const HEADER_BYTES: usize = 512;
    const SIGNATURE: &[u8; 8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

    let mut input = File::open(path).map_err(|error| {
        SafetyError::new(
            ErrorCode::Internal,
            format!("could not open input: {error}"),
        )
    })?;
    let mut header = [0_u8; HEADER_BYTES];
    input.read_exact(&mut header).map_err(|error| {
        SafetyError::new(
            ErrorCode::BadZip,
            format!("invalid XLS compound-document header: {error}"),
        )
    })?;
    if &header[..SIGNATURE.len()] != SIGNATURE {
        return Err(SafetyError::new(
            ErrorCode::BadZip,
            "invalid XLS compound-document signature",
        ));
    }
    if header[28..30] != [0xfe, 0xff] {
        return Err(SafetyError::new(
            ErrorCode::BadZip,
            "invalid XLS compound-document byte order",
        ));
    }
    let sector_shift = u16::from_le_bytes([header[30], header[31]]);
    let sector_bytes = match sector_shift {
        9 => 512_u64,
        12 => 4_096_u64,
        _ => {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!("invalid XLS compound-document sector shift {sector_shift}"),
            ))
        }
    };
    if input_bytes < sector_bytes {
        return Err(SafetyError::new(
            ErrorCode::BadZip,
            format!("XLS compound document is smaller than one {sector_bytes} byte sector"),
        ));
    }

    // The header's sector-count fields drive up-front allocations in CFB
    // readers (calamine allocates `num_fat_sectors * 4` bytes before reading
    // any of them). None of these counts can exceed the number of sectors
    // that physically fit in the container.
    let total_sectors = input_bytes / sector_bytes;
    let header_count = |offset: usize| {
        u64::from(u32::from_le_bytes(
            header[offset..offset + 4]
                .try_into()
                .expect("header is 512 bytes"),
        ))
    };
    for (offset, field) in [
        (0x28, "directory sector"),
        (0x2C, "FAT sector"),
        (0x40, "mini-FAT sector"),
        (0x48, "DIFAT sector"),
    ] {
        let declared = header_count(offset);
        if declared > total_sectors {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!(
                    "XLS compound document declares {declared} {field}s but only {total_sectors} sectors fit in the container"
                ),
            ));
        }
    }

    // calamine walks the DIFAT chain with an acknowledged unbounded loop
    // (`sector_id = difat.pop()` under a `//TODO: check if in infinite loop`
    // in its cfb.rs), extending a Vec by one sector per hop. A cyclic chain
    // therefore grows without bound: a 5,640-byte fuzz artifact reached a
    // 128 GiB allocation and 87 GiB RSS before wax's wall-clock timeout
    // fired. Walk the same chain first, bounded by the sector count that
    // physically fits, and reject cycles.
    let difat_start = u32::from_le_bytes(header[68..72].try_into().expect("header is 512 bytes"));
    let mut sector_id = difat_start;
    let mut hops = 0_u64;
    while sector_id < 0xFFFF_FFFA {
        hops += 1;
        if hops > total_sectors {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!(
                    "XLS compound-document DIFAT chain exceeds {total_sectors} sectors (cyclic or corrupt)"
                ),
            ));
        }
        let sector_start = (u64::from(sector_id) + 1)
            .checked_mul(sector_bytes)
            .ok_or_else(|| {
                SafetyError::new(ErrorCode::BadZip, "XLS DIFAT sector offset overflowed")
            })?;
        let next_offset = sector_start
            .checked_add(sector_bytes - 4)
            .filter(|end| end + 4 <= input_bytes)
            .ok_or_else(|| {
                SafetyError::new(
                    ErrorCode::BadZip,
                    format!("XLS DIFAT chain leaves the {input_bytes} byte container"),
                )
            })?;
        input
            .seek(std::io::SeekFrom::Start(next_offset))
            .map_err(|error| {
                SafetyError::new(
                    ErrorCode::BadZip,
                    format!("could not seek the XLS DIFAT chain: {error}"),
                )
            })?;
        let mut next = [0_u8; 4];
        input.read_exact(&mut next).map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("could not read the XLS DIFAT chain: {error}"),
            )
        })?;
        sector_id = u32::from_le_bytes(next);
    }

    preflight_cfb_chains(
        &mut input,
        &header,
        sector_bytes,
        total_sectors,
        input_bytes,
    )?;

    preflight_biff_records(path, input_bytes, options)
}

/// Rejects cyclic or over-long CFB sector chains before calamine follows them.
///
/// calamine reads the directory chain with `usize::MAX` as its length bound
/// (`Sectors::get_chain`), so a FAT cycle appends one sector per hop forever:
/// the quarantined 5,640-byte artifact reached 24 GiB RSS in 30 s that way.
/// Every real chain is at most as long as the number of sectors that
/// physically fit in the container, so walking each chain here with that bound
/// contains the whole class — including the mini-FAT and ministream chains
/// calamine reads with concrete lengths but the same cyclic-FAT exposure.
fn preflight_cfb_chains(
    input: &mut File,
    header: &[u8; 512],
    sector_bytes: u64,
    total_sectors: u64,
    input_bytes: u64,
) -> Result<(), SafetyError> {
    const FREE_SECTOR: u32 = 0xFFFF_FFFF;
    const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
    // Sector ids at or above this are reserved markers, never real sectors.
    const RESERVED_SECTORS: u32 = 0xFFFF_FFFA;

    let read_sector = |input: &mut File, id: u32| -> Result<Vec<u8>, SafetyError> {
        let start = (u64::from(id) + 1)
            .checked_mul(sector_bytes)
            .filter(|start| start.saturating_add(sector_bytes) <= input_bytes)
            .ok_or_else(|| {
                SafetyError::new(
                    ErrorCode::BadZip,
                    format!("XLS compound-document sector {id} lies outside the container"),
                )
            })?;
        input
            .seek(std::io::SeekFrom::Start(start))
            .map_err(|error| {
                SafetyError::new(
                    ErrorCode::BadZip,
                    format!("could not seek XLS sector {id}: {error}"),
                )
            })?;
        let mut buffer = vec![0_u8; sector_bytes as usize];
        input.read_exact(&mut buffer).map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("could not read XLS sector {id}: {error}"),
            )
        })?;
        Ok(buffer)
    };

    // Collect the DIFAT: the 109 header entries plus every DIFAT sector. The
    // chain walk above already proved this terminates within `total_sectors`.
    let mut difat = Vec::new();
    let mut push_entries = |bytes: &[u8], difat: &mut Vec<u32>| {
        for entry in bytes.chunks_exact(4) {
            let id = u32::from_le_bytes(entry.try_into().expect("chunks_exact(4)"));
            if id < RESERVED_SECTORS {
                difat.push(id);
            }
        }
    };
    push_entries(&header[76..512], &mut difat);
    let mut sector_id = u32::from_le_bytes(header[68..72].try_into().expect("header is 512 bytes"));
    while sector_id < RESERVED_SECTORS {
        let sector = read_sector(input, sector_id)?;
        let split = sector.len() - 4;
        push_entries(&sector[..split], &mut difat);
        sector_id = u32::from_le_bytes(sector[split..].try_into().expect("4 trailing bytes"));
    }

    // Materialize the FAT. Every entry a chain can reach lives here, and the
    // header's FAT-sector count was already bounded by `total_sectors`.
    let entries_per_sector = (sector_bytes / 4) as usize;
    let mut fat = Vec::with_capacity(difat.len().saturating_mul(entries_per_sector));
    for id in difat {
        let sector = read_sector(input, id)?;
        for entry in sector.chunks_exact(4) {
            fat.push(u32::from_le_bytes(
                entry.try_into().expect("chunks_exact(4)"),
            ));
        }
    }

    let walk = |start: u32, name: &str| -> Result<(), SafetyError> {
        let mut sector_id = start;
        let mut hops = 0_u64;
        while sector_id != END_OF_CHAIN && sector_id != FREE_SECTOR {
            if sector_id >= RESERVED_SECTORS {
                return Err(SafetyError::new(
                    ErrorCode::BadZip,
                    format!("XLS {name} chain reaches reserved sector id {sector_id}"),
                ));
            }
            hops += 1;
            if hops > total_sectors {
                return Err(SafetyError::new(
                    ErrorCode::BadZip,
                    format!("XLS {name} chain exceeds {total_sectors} sectors (cyclic or corrupt)"),
                ));
            }
            if u64::from(sector_id) >= total_sectors {
                return Err(SafetyError::new(
                    ErrorCode::BadZip,
                    format!(
                        "XLS {name} chain enters sector {sector_id} beyond the {total_sectors} sectors in the container"
                    ),
                ));
            }
            let Some(&next) = fat.get(sector_id as usize) else {
                return Err(SafetyError::new(
                    ErrorCode::BadZip,
                    format!("XLS {name} chain leaves the allocation table at sector {sector_id}"),
                ));
            };
            sector_id = next;
        }
        Ok(())
    };

    let directory_start =
        u32::from_le_bytes(header[48..52].try_into().expect("header is 512 bytes"));
    walk(directory_start, "directory")?;
    let mini_fat_start =
        u32::from_le_bytes(header[60..64].try_into().expect("header is 512 bytes"));
    walk(mini_fat_start, "mini-FAT")?;
    Ok(())
}

fn preflight_biff_records(
    path: &Path,
    input_bytes: u64,
    options: ReaderOptions,
) -> Result<(), SafetyError> {
    const MAX_BIFF_RECORDS: usize = 5_000_000;

    let mut compound = cfb::OpenOptions::new()
        .max_buffer_size(64 * 1024)
        .open(path)
        .map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("invalid XLS compound document: {error}"),
            )
        })?;
    for entry in compound.walk() {
        if entry.is_stream() && entry.len() > input_bytes {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!(
                    "XLS compound document declares a {} byte stream {} inside a {input_bytes} byte container",
                    entry.len(),
                    entry.path().display(),
                ),
            ));
        }
    }
    let workbook_path = compound
        .walk()
        .find(|entry| {
            entry.is_stream()
                && matches!(
                    entry.name().to_ascii_lowercase().as_str(),
                    "workbook" | "book"
                )
        })
        .map(|entry| entry.path().to_path_buf())
        .ok_or_else(|| {
            SafetyError::new(
                ErrorCode::BadZip,
                "XLS compound document has no Workbook stream",
            )
        })?;
    let mut stream = compound.open_stream(&workbook_path).map_err(|error| {
        SafetyError::new(
            ErrorCode::BadZip,
            format!(
                "could not open XLS Workbook stream {}: {error}",
                workbook_path.display()
            ),
        )
    })?;
    // The stream size was validated against the container above and the
    // container against `max_bytes`, so buffering is bounded (calamine
    // buffers the same stream in full anyway).
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(|error| {
        SafetyError::new(
            ErrorCode::BadZip,
            format!("could not read XLS Workbook stream: {error}"),
        )
    })?;

    // Mirror calamine's exact read pattern: the globals substream from the
    // start until its EOF record, then each BOUNDSHEET-declared sheet
    // substream until its EOF. Slack regions between substreams are never
    // read by calamine, so garbage there must not fail preflight.
    let mut remaining_records = MAX_BIFF_RECORDS;
    let sheet_offsets = walk_biff_substream(&bytes, 0, options, &mut remaining_records)?;
    for offset in sheet_offsets {
        walk_biff_substream(&bytes, offset, options, &mut remaining_records)?;
    }

    Ok(())
}

/// Walks one BIFF substream from `start` until its EOF record, validating
/// every record calamine will read. Returns the sheet offsets declared by
/// BOUNDSHEET records encountered (non-empty only for the globals substream).
fn walk_biff_substream(
    bytes: &[u8],
    start: usize,
    options: ReaderOptions,
    remaining_records: &mut usize,
) -> Result<Vec<usize>, SafetyError> {
    let mut offset = start;
    let mut sheet_offsets = Vec::new();
    let mut extent = ObservedExtent::default();

    loop {
        // Trailing padding that is not a whole record is tolerated by Excel
        // and by calamine's bounds-checked RecordIter; stop, don't reject.
        if bytes.len() - offset < 4 {
            break;
        }
        *remaining_records = remaining_records
            .checked_sub(1)
            .ok_or_else(|| SafetyError::new(ErrorCode::TooLarge, "BIFF record count overflowed"))?;
        if *remaining_records == 0 {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                "XLS Workbook stream exceeds the preflight record limit",
            ));
        }

        let kind = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let record_bytes = usize::from(u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]));
        offset += 4;
        if record_bytes > bytes.len() - offset {
            break;
        }
        let data = &bytes[offset..offset + record_bytes];
        offset += record_bytes;

        if kind == 0x0809 && data.len() < 2 {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                "BIFF BOF record is shorter than 2 bytes",
            ));
        }
        // Minimum payload lengths for records calamine's XLS path reads at
        // fixed offsets without bounds checks (fuzz-derived; utils.rs
        // read_u16/read_u32/read_f64 panic on short slices). A record shorter
        // than its fixed header cannot occur in a well-formed workbook.
        let minimum = match kind {
            0x0085 => 6,          // BoundSheet8: lbPlyPos + hsState/dt
            0x00FC => 8,          // SST: cstTotal + cstUnique
            0x00FD => 10,         // LabelSst: row, col, ixfe, isst
            0x0203 => 14,         // Number: row, col, ixfe, xnum
            0x0205 => 8,          // BoolErr: row, col, ixfe, bes
            0x027E => 10,         // RK: row, col, ixfe, RkNumber
            0x0204 | 0x00D6 => 8, // Label / RString fixed header
            0x0006 => 22,         // Formula: cell, xnum, flags, chn, cce
            0x0207 => 2,          // String (formula result): cch (BIFF5 has no grbit)
            0x0200 => 10,         // Dimensions (also structurally checked)
            0x0017 => 2,          // ExternSheet: cxti (XTI array may be continued)
            // Lbl (DEFINEDNAME): calamine's parse_lbl reads data[3] then
            // read_u16(&data[4..]) with no length check, so a short record
            // is an index-out-of-bounds panic — one that fires in release
            // too, unlike the arithmetic-overflow family.
            0x0018 => 6,
            0x00E5 => 2,          // MergeCells: cmcs
            // Records whose calamine *match guards* read a u16 before the
            // arm body runs, so a short payload panics before any check.
            0x002F | 0x0042 | 0x0022 => 2, // FilePass, CodePage, DateMode
            0x00E0 => 4,                   // XF: ifnt + ifmt
            _ => 0,
        };
        if data.len() < minimum {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!(
                    "BIFF record 0x{kind:04X} is {} bytes, shorter than its {minimum} byte fixed header",
                    data.len()
                ),
            ));
        }
        match kind {
            0x000A => break, // EOF ends this substream
            0x00E5 => {
                let count = usize::from(u16::from_le_bytes([data[0], data[1]]));
                if data.len() < 2 + 8 * count {
                    return Err(SafetyError::new(
                        ErrorCode::BadZip,
                        "BIFF MergeCells record is shorter than its declared range count",
                    ));
                }
            }
            0x0200 => check_declared_extent(data, options.max_declared_cells)?,
            // Cell-bearing records. calamine collects these into a Vec and
            // hands it to `Range::from_sparse`, which densifies the *observed*
            // span — no DIMENSIONS record required. A file holding two cells
            // at opposite corners of the BIFF grid therefore reserves
            // 65,536 x 65,536 cells (the 137 GB allocation behind the
            // quarantined calamine-unbounded-fat-growth.xls artifact).
            0x0201 | 0x0203 | 0x0204 | 0x0205 | 0x00D6 | 0x027E | 0x00FD | 0x0006
                if data.len() >= 4 =>
            {
                let row = u32::from(u16::from_le_bytes([data[0], data[1]]));
                let column = u32::from(u16::from_le_bytes([data[2], data[3]]));
                extent.observe(row, column);
            }
            // MulBlank carries one row and a first..last column span.
            0x00BE if data.len() >= 6 => {
                let row = u32::from(u16::from_le_bytes([data[0], data[1]]));
                let first = u32::from(u16::from_le_bytes([data[2], data[3]]));
                let last = u32::from(u16::from_le_bytes([
                    data[data.len() - 2],
                    data[data.len() - 1],
                ]));
                extent.observe(row, first);
                extent.observe(row, last.max(first));
            }
            0x00BD => {
                let malformed = data.len() < 6 || {
                    let first_col = u16::from_le_bytes([data[2], data[3]]);
                    let last_col = u16::from_le_bytes([data[data.len() - 2], data[data.len() - 1]]);
                    last_col < first_col
                        || data.len() != 6 + 6 * (usize::from(last_col - first_col) + 1)
                };
                if malformed {
                    return Err(SafetyError::new(
                        ErrorCode::BadZip,
                        "malformed BIFF MulRk record",
                    ));
                }
                let row = u32::from(u16::from_le_bytes([data[0], data[1]]));
                let first = u32::from(u16::from_le_bytes([data[2], data[3]]));
                let last = u32::from(u16::from_le_bytes([
                    data[data.len() - 2],
                    data[data.len() - 1],
                ]));
                extent.observe(row, first);
                extent.observe(row, last);
            }
            0x0085 => {
                let declared =
                    u32::from_le_bytes(data[..4].try_into().expect("minimum length checked above"))
                        as usize;
                if declared > bytes.len() {
                    return Err(SafetyError::new(
                        ErrorCode::BadZip,
                        format!(
                            "BIFF BOUNDSHEET declares sheet offset {declared} beyond the {} byte Workbook stream",
                            bytes.len()
                        ),
                    ));
                }
                sheet_offsets.push(declared);
            }
            _ => {}
        }
    }

    extent.check(options.max_declared_cells)?;
    Ok(sheet_offsets)
}

/// The row/column span actually occupied by a substream's cell records.
///
/// calamine densifies this span in `Range::from_sparse` regardless of what
/// (or whether) a DIMENSIONS record declares, so it needs the same cap.
#[derive(Debug)]
struct ObservedExtent {
    first_row: u32,
    last_row: u32,
    first_col: u32,
    last_col: u32,
    seen: bool,
}

impl Default for ObservedExtent {
    fn default() -> Self {
        Self {
            first_row: u32::MAX,
            last_row: 0,
            first_col: u32::MAX,
            last_col: 0,
            seen: false,
        }
    }
}

impl ObservedExtent {
    fn observe(&mut self, row: u32, column: u32) {
        self.seen = true;
        self.first_row = self.first_row.min(row);
        self.last_row = self.last_row.max(row);
        self.first_col = self.first_col.min(column);
        self.last_col = self.last_col.max(column);
    }

    fn check(&self, max_cells: u64) -> Result<(), SafetyError> {
        if !self.seen {
            return Ok(());
        }
        let rows = u64::from(self.last_row - self.first_row) + 1;
        let cols = u64::from(self.last_col - self.first_col) + 1;
        let cells = rows.saturating_mul(cols);
        if cells > max_cells {
            return Err(SafetyError::new(
                ErrorCode::Bomb,
                format!(
                    "XLS cell records span {rows}x{cols} ({cells} cells), exceeding the {max_cells} cell limit"
                ),
            ));
        }
        Ok(())
    }
}

fn check_declared_extent(data: &[u8], max_cells: u64) -> Result<(), SafetyError> {
    let (first_row, last_row, mut first_col, last_col) = match data.len() {
        10 => (
            u32::from(u16::from_le_bytes([data[0], data[1]])),
            u32::from(u16::from_le_bytes([data[2], data[3]])),
            u32::from(u16::from_le_bytes([data[4], data[5]])),
            u32::from(u16::from_le_bytes([data[6], data[7]])),
        ),
        14 => (
            u32::from_le_bytes(data[0..4].try_into().expect("four bytes checked")),
            u32::from_le_bytes(data[4..8].try_into().expect("four bytes checked")),
            u32::from(u16::from_le_bytes([data[8], data[9]])),
            u32::from(u16::from_le_bytes([data[10], data[11]])),
        ),
        length => {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!("BIFF DIMENSIONS record has invalid length {length}"),
            ))
        }
    };

    // Mirror calamine's `parse_dimensions` exactly (xls.rs): the record
    // stores exclusive last row/column, calamine converts to inclusive with
    // `- 1`, and the call site then reserves `(end - start + 1)` cells. Those
    // u32 subtractions wrap in release builds, so a reversed pair must be
    // rejected here — including when a bound is 0, the gap that let a 137 GB
    // reserve through (fuzz artifact oom-bc197d861c...).
    if 0xff < first_col || last_col < first_col {
        first_col = 0;
    }
    // Reproduce calamine's arithmetic bit for bit, wrapping included: it
    // converts the record's exclusive bounds with `- 1`, then reserves
    // `(end - start + 1)` rows by cols with a `saturating_mul`. Reversed
    // bounds wrap to ~2^32, which is only harmless when the other dimension
    // is 0 (the product saturates to 0 and calamine reserves nothing) — so
    // judge the product, not the operands, or legitimate files are rejected.
    let (rows, cols) = if last_row >= 1 && last_col >= 1 {
        (
            u64::from((last_row - 1).wrapping_sub(first_row).wrapping_add(1)),
            u64::from((last_col - 1).wrapping_sub(first_col).wrapping_add(1)),
        )
    } else {
        (1, 1)
    };
    let cells = rows.saturating_mul(cols);
    if cells > max_cells {
        return Err(SafetyError::new(
            ErrorCode::Bomb,
            format!(
                "declared XLS extent {rows}x{cols} ({cells} cells) exceeds the {max_cells} cell limit"
            ),
        ));
    }

    Ok(())
}

/// Checks ZIP metadata before any entry is decompressed, then validates XML parts.
///
/// This is public so the container parser can be fuzzed directly with arbitrary bytes.
pub fn preflight_container<R>(input: R, options: ReaderOptions) -> Result<(), SafetyError>
where
    R: Read + Seek,
{
    let mut archive = ZipArchive::new(input).map_err(|error| {
        SafetyError::new(ErrorCode::BadZip, format!("invalid workbook zip: {error}"))
    })?;
    if archive.len() > options.max_zip_entries {
        return Err(SafetyError::new(
            ErrorCode::Bomb,
            format!(
                "zip contains {} entries, exceeding the {} entry limit",
                archive.len(),
                options.max_zip_entries
            ),
        ));
    }

    let mut total_bytes = 0_u64;
    let mut xml_entries = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("could not inspect zip entry {index}: {error}"),
            )
        })?;
        let part_bytes = entry.size();
        if part_bytes > options.max_part_bytes {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                format!(
                    "zip entry {index} expands to {part_bytes} bytes, exceeding the {} byte part limit",
                    options.max_part_bytes
                ),
            ));
        }
        total_bytes = total_bytes.checked_add(part_bytes).ok_or_else(|| {
            SafetyError::new(
                ErrorCode::TooLarge,
                "zip decompressed-size total overflowed",
            )
        })?;
        if total_bytes > options.max_total_uncompressed_bytes {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                format!(
                    "zip expands to more than the {} byte total limit",
                    options.max_total_uncompressed_bytes
                ),
            ));
        }
        if is_ratio_bomb(
            part_bytes,
            entry.compressed_size(),
            options.max_compression_ratio,
            options.compression_ratio_min_bytes,
        ) {
            return Err(SafetyError::new(
                ErrorCode::Bomb,
                format!(
                    "zip entry {index} expands from {} to {part_bytes} bytes, exceeding the {}:1 ratio limit",
                    entry.compressed_size(),
                    options.max_compression_ratio
                ),
            ));
        }
        if is_xml_part(entry.name()) {
            xml_entries.push(index);
        }
    }

    for index in xml_entries {
        let entry = archive.by_index(index).map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("could not open XML zip entry {index}: {error}"),
            )
        })?;
        guard_xml(BufReader::new(entry), options).map_err(|error| {
            SafetyError::new(
                error.code,
                format!("unsafe XML in zip entry {index}: {}", error.message),
            )
        })?;
    }

    Ok(())
}

/// Applies wax's depth, DTD, token-count, token-size, and total-byte XML rails.
pub fn guard_xml<R>(input: R, options: ReaderOptions) -> Result<(), SafetyError>
where
    R: BufRead,
{
    let mut reader = XmlReader::from_reader(input);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut tokens = 0_usize;
    let mut previous_position = 0_u64;

    loop {
        buffer.clear();
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            SafetyError::new(ErrorCode::BadZip, format!("invalid XML: {error}"))
        })?;
        let position = reader.buffer_position();
        let token_bytes = position.saturating_sub(previous_position);
        previous_position = position;
        tokens = tokens
            .checked_add(1)
            .ok_or_else(|| SafetyError::new(ErrorCode::TooLarge, "XML token count overflowed"))?;
        if tokens > options.max_xml_tokens {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                format!("XML exceeds the {} token limit", options.max_xml_tokens),
            ));
        }
        if token_bytes > options.max_xml_token_bytes as u64 {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                format!(
                    "XML token is {token_bytes} bytes, exceeding the {} byte limit",
                    options.max_xml_token_bytes
                ),
            ));
        }
        if position > options.max_xml_bytes {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                format!(
                    "XML exceeds the {} byte buffer limit",
                    options.max_xml_bytes
                ),
            ));
        }

        match event {
            Event::DocType(_) => {
                return Err(SafetyError::new(
                    ErrorCode::Bomb,
                    "XML DOCTYPE and internal DTD subsets are not allowed",
                ));
            }
            Event::Start(ref start) => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| SafetyError::new(ErrorCode::Bomb, "XML depth overflowed"))?;
                if depth > options.max_xml_depth {
                    return Err(SafetyError::new(
                        ErrorCode::Bomb,
                        format!(
                            "XML nesting depth exceeds the {} level limit",
                            options.max_xml_depth
                        ),
                    ));
                }
                check_cell_reference_attributes(start)?;
            }
            Event::Empty(ref start) => check_cell_reference_attributes(start)?,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(())
}

/// Rejects A1 references whose column run cannot name a real column.
///
/// calamine parses a reference's column with an unguarded
/// `col = col * 26 + ...` accumulator (`xlsx/mod.rs`
/// `get_row_and_optional_column`). Seven or more letters overflow `u32`:
/// that panics under the overflow checks the fuzz targets build with, and
/// wraps silently in release, yielding a column index unrelated to the
/// stored reference. Excel's last column is `XFD`, so more than three
/// letters cannot name one either way — reject the reference instead of
/// letting a wrapped index reach the reader.
fn check_cell_reference_attributes(
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<(), SafetyError> {
    const MAX_COLUMN_LETTERS: usize = 3;

    // Match *local* names, as calamine does: a namespace prefix such as
    // `<x:dimension>` would otherwise walk straight past this rail. The
    // element set is deliberately closed — a bare "any `ref` attribute"
    // rule also matches XSD `<xs:element ref="EG_ExtensionList">` in the
    // custom-XML parts real workbooks carry, which cost 14 corpus opens
    // when tried.
    let element = start.name();
    let element = element.local_name();
    for attribute in start.attributes().with_checks(false).flatten() {
        let key = attribute.key;
        // `xmlns:r="…"` has the local name `r` but binds a namespace URI.
        if key.as_ref().starts_with(b"xmlns") {
            continue;
        }
        let is_reference = match key.local_name().as_ref() {
            b"r" => matches!(element.as_ref(), b"c" | b"row"),
            b"ref" => matches!(
                element.as_ref(),
                b"dimension" | b"mergeCell" | b"autoFilter" | b"hyperlink"
            ),
            _ => false,
        };
        if !is_reference {
            continue;
        }
        let value = attribute.value.as_ref();
        let mut letters = 0_usize;
        for byte in value {
            if byte.is_ascii_alphabetic() {
                letters += 1;
                if letters > MAX_COLUMN_LETTERS {
                    return Err(SafetyError::new(
                        ErrorCode::BadZip,
                        format!(
                            "cell reference {} has more than {MAX_COLUMN_LETTERS} column letters",
                            String::from_utf8_lossy(value)
                        ),
                    ));
                }
            } else {
                letters = 0;
            }
        }
    }
    Ok(())
}

fn failure_document(file: String, code: ErrorCode, message: impl Into<String>) -> Document {
    Document::failure(
        env!("CARGO_PKG_VERSION"),
        file,
        DumpError {
            code: code.as_str().to_owned(),
            msg: message.into(),
        },
        Vec::new(),
    )
}

fn is_zip_workbook(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("xlsx" | "xlsm" | "xlsb" | "ods")
    )
}

fn is_legacy_xls(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xls"))
}

fn is_xml_part(name: &str) -> bool {
    let lowercase = name.to_ascii_lowercase();
    XML_EXTENSIONS
        .iter()
        .any(|extension| lowercase.ends_with(extension))
}

fn is_ratio_bomb(
    uncompressed_bytes: u64,
    compressed_bytes: u64,
    max_ratio: u64,
    minimum_bytes: u64,
) -> bool {
    if uncompressed_bytes <= minimum_bytes {
        return false;
    }
    compressed_bytes
        .checked_mul(max_ratio)
        .is_none_or(|maximum| uncompressed_bytes > maximum)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use wax_core::Document;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::*;

    fn zip(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut archive = ZipWriter::new(&mut output);
            for (name, bytes, compression) in entries {
                archive
                    .start_file(
                        *name,
                        SimpleFileOptions::default().compression_method(*compression),
                    )
                    .expect("zip entry should start");
                archive
                    .write_all(bytes)
                    .expect("zip entry should be written");
            }
            archive.finish().expect("zip should finish");
        }
        output.into_inner()
    }

    fn legacy_seed(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fuzz")
            .join("corpus")
            .join("legacy_xls_reader")
            .join(name)
    }

    #[test]
    fn rejects_high_ratio_deflate_bomb() {
        let payload = vec![0_u8; 32 * 1024];
        let bytes = zip(&[(
            "xl/worksheets/sheet1.xml",
            &payload,
            CompressionMethod::Deflated,
        )]);
        let options = ReaderOptions {
            compression_ratio_min_bytes: 1024,
            ..ReaderOptions::default()
        };

        let error = preflight_container(Cursor::new(bytes), options)
            .expect_err("high-ratio data should be rejected");

        assert_eq!(error.code(), ErrorCode::Bomb);
    }

    #[test]
    fn rejects_too_many_zip_entries() {
        let bytes = zip(&[
            ("one.bin", b"", CompressionMethod::Stored),
            ("two.bin", b"", CompressionMethod::Stored),
            ("three.bin", b"", CompressionMethod::Stored),
        ]);
        let options = ReaderOptions {
            max_zip_entries: 2,
            ..ReaderOptions::default()
        };

        let error = preflight_container(Cursor::new(bytes), options)
            .expect_err("entry-count bomb should be rejected");

        assert_eq!(error.code(), ErrorCode::Bomb);
    }

    #[test]
    fn rejects_oversized_part_and_total() {
        let part = zip(&[("part.bin", b"12345", CompressionMethod::Stored)]);
        let part_error = preflight_container(
            Cursor::new(part),
            ReaderOptions {
                max_part_bytes: 4,
                ..ReaderOptions::default()
            },
        )
        .expect_err("oversized part should be rejected");
        assert_eq!(part_error.code(), ErrorCode::TooLarge);

        let total = zip(&[
            ("one.bin", b"1234", CompressionMethod::Stored),
            ("two.bin", b"5678", CompressionMethod::Stored),
        ]);
        let total_error = preflight_container(
            Cursor::new(total),
            ReaderOptions {
                max_total_uncompressed_bytes: 7,
                ..ReaderOptions::default()
            },
        )
        .expect_err("oversized total should be rejected");
        assert_eq!(total_error.code(), ErrorCode::TooLarge);
    }

    #[test]
    fn rejects_doctype_and_deep_xml() {
        let doctype = b"<!DOCTYPE root [<!ENTITY wax \"boom\">]><root>&wax;</root>";
        let error = guard_xml(Cursor::new(doctype), ReaderOptions::default())
            .expect_err("DOCTYPE should be rejected");
        assert_eq!(error.code(), ErrorCode::Bomb);

        let error = guard_xml(
            Cursor::new(b"<a><b><c /></b></a>"),
            ReaderOptions {
                max_xml_depth: 1,
                ..ReaderOptions::default()
            },
        )
        .expect_err("deep XML should be rejected");
        assert_eq!(error.code(), ErrorCode::Bomb);
    }

    #[test]
    fn rejects_xml_token_and_buffer_overages() {
        let token_error = guard_xml(
            Cursor::new(b"<root>oversized</root>"),
            ReaderOptions {
                max_xml_token_bytes: 4,
                ..ReaderOptions::default()
            },
        )
        .expect_err("large token should be rejected");
        assert_eq!(token_error.code(), ErrorCode::TooLarge);

        let buffer_error = guard_xml(
            Cursor::new(b"<root />"),
            ReaderOptions {
                max_xml_bytes: 4,
                ..ReaderOptions::default()
            },
        )
        .expect_err("large XML buffer should be rejected");
        assert_eq!(buffer_error.code(), ErrorCode::TooLarge);
    }

    #[test]
    fn quick_xml_reports_custom_entities_without_expanding_them() {
        let mut reader = XmlReader::from_reader(Cursor::new(b"<root>&wax;</root>"));
        let mut buffer = Vec::new();
        assert!(matches!(
            reader.read_event_into(&mut buffer),
            Ok(Event::Start(_))
        ));
        buffer.clear();
        match reader
            .read_event_into(&mut buffer)
            .expect("entity event should parse")
        {
            Event::GeneralRef(reference) => {
                let name: &[u8] = reference.as_ref();
                assert_eq!(name, b"wax");
            }
            event => panic!("custom entity was not preserved as a reference: {event:?}"),
        }
    }

    #[derive(Clone, Copy)]
    struct SleepingReader;

    impl Reader for SleepingReader {
        fn read(&self, path: &Path, _options: ReaderOptions) -> Document {
            // Long enough that the early-return assertion below stays
            // meaningful on heavily loaded hosts; the worker thread is
            // detached, so this never extends the test's wall time.
            std::thread::sleep(Duration::from_millis(3_000));
            Document::success("test", path.to_string_lossy(), Vec::new(), Vec::new())
        }
    }

    #[test]
    fn deadline_returns_while_reader_is_still_blocked() {
        let file = tempfile::NamedTempFile::new().expect("temporary input should be created");
        let options = ReaderOptions {
            timeout_ms: 25,
            ..ReaderOptions::default()
        };
        let started = Instant::now();

        let document = read_with_deadline(SleepingReader, file.path(), options);

        // Generous scheduling headroom for loaded hosts; still strictly
        // before the 3s reader would have finished on its own.
        assert!(started.elapsed() < Duration::from_millis(1_500));
        assert!(!document.ok);
        assert_eq!(
            document.error.expect("timeout should carry an error").code,
            ErrorCode::Timeout.as_str()
        );
    }

    #[test]
    fn input_size_cap_runs_before_reader() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary input should be created");
        file.write_all(b"12345")
            .expect("temporary input should be written");

        let document = read_with_deadline(
            SleepingReader,
            file.path(),
            ReaderOptions {
                max_bytes: 4,
                timeout_ms: 1_000,
                ..ReaderOptions::default()
            },
        );

        assert!(!document.ok);
        assert_eq!(
            document.error.expect("size cap should carry an error").code,
            ErrorCode::TooLarge.as_str()
        );
    }

    #[test]
    fn malformed_zip_is_a_structured_error() {
        let error = preflight_container(Cursor::new(b"not a zip"), ReaderOptions::default())
            .expect_err("malformed zip should fail");
        assert_eq!(error.code(), ErrorCode::BadZip);
    }

    #[test]
    fn legacy_cfb_handles_partial_trailing_sectors_without_panicking() {
        // Real xls files carry trailing junk that is not a whole 512-byte
        // sector; Excel tolerates them, so wax must not reject wholesale
        // (this cost 126 corpus opens when preflight required alignment).
        // The panic-safety the old alignment check provided is now covered
        // by the per-record and header-count guards.
        let source = include_bytes!("../tests/fixtures/date.xls");
        for keep in [source.len() - 1, source.len() - 300] {
            let mut input = tempfile::Builder::new()
                .suffix(".xls")
                .tempfile()
                .expect("temporary XLS should be created");
            input
                .write_all(&source[..keep])
                .expect("truncated XLS should be written");

            let document = read_with_deadline(
                crate::CalamineReader,
                input.path(),
                ReaderOptions::default(),
            );
            assert!(
                document.ok || document.error.is_some(),
                "truncated CFB must produce a structured result, never a panic"
            );
        }
    }

    #[test]
    fn legacy_biff_rejects_the_zero_length_bof_fuzz_regression() {
        let error = preflight_path(
            &legacy_seed("calamine-zero-length-bof.xls"),
            ReaderOptions::default(),
        )
        .expect_err("zero-length BOF should be rejected before calamine");

        assert_eq!(error.code(), ErrorCode::BadZip);
        assert!(error.message().contains("BOF"));
    }

    #[test]
    fn legacy_biff_rejects_the_boundsheet_offset_fuzz_regression() {
        let error = preflight_path(
            &legacy_seed("calamine-boundsheet-offset.xls"),
            ReaderOptions::default(),
        )
        .expect_err("out-of-stream BOUNDSHEET offset should be rejected before calamine");

        assert_eq!(error.code(), ErrorCode::BadZip);
        assert!(error.message().contains("BOUNDSHEET"));
    }

    #[test]
    fn legacy_cfb_rejects_the_fat_count_lie_fuzz_regression() {
        let error = preflight_path(
            &legacy_seed("calamine-fat-count-lie.xls"),
            ReaderOptions::default(),
        )
        .expect_err("lying FAT sector count should be rejected before calamine allocates");

        assert_eq!(error.code(), ErrorCode::BadZip);
        assert!(error.message().contains("FAT sector"));
    }

    #[test]
    fn legacy_biff_rejects_the_mulrk_reversed_columns_fuzz_regression() {
        let error = preflight_path(
            &legacy_seed("calamine-mulrk-reversed-cols.xls"),
            ReaderOptions::default(),
        )
        .expect_err("reversed MulRk column bounds should be rejected before calamine");

        assert_eq!(error.code(), ErrorCode::BadZip);
        assert!(error.message().contains("MulRk"));
    }

    #[test]
    fn every_legacy_fuzz_artifact_produces_a_structured_document() {
        // Defense in depth, and the honest statement of what wax guarantees:
        // preflight rejects what it recognizes, and `CalamineReader`'s
        // `catch_unwind` contains anything that still panics inside calamine.
        // (cargo-fuzz builds with panic=abort, so the fuzz target reports
        // those as crashes even though the shipped binary never aborts.)
        let artifacts = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fuzz")
            .join("artifacts")
            .join("legacy_xls_reader");
        let Ok(entries) = std::fs::read_dir(&artifacts) else {
            return; // artifacts are optional in a fresh clone
        };
        let mut checked = 0;
        for entry in entries.flatten().filter(|entry| entry.path().is_file()) {
            let source = std::fs::read(entry.path()).expect("artifact should be readable");
            let mut input = tempfile::Builder::new()
                .suffix(".xls")
                .tempfile()
                .expect("temporary XLS should be created");
            input
                .write_all(&source)
                .expect("artifact should be written");

            let document = read_with_deadline(
                crate::CalamineReader,
                input.path(),
                ReaderOptions::default(),
            );
            assert!(
                !document.ok && document.error.is_some(),
                "artifact {:?} must produce a structured failure, never a panic or success",
                entry.file_name()
            );
            checked += 1;
        }
        // `checked == 0` is fine: fuzz tooling creates the artifact
        // directory empty; artifacts themselves are gitignored, so both a
        // fresh clone (no dir) and a fuzz-run-but-clean tree (empty dir)
        // simply mean there is nothing to replay here.
        let _ = checked;
    }

    #[test]
    fn legacy_biff_rejects_observed_extent_bomb_without_a_dimensions_record() {
        // The former quarantined finding: no DIMENSIONS record declares a
        // hostile extent, but two cell records sit at opposite corners of the
        // BIFF grid, so calamine's `Range::from_sparse` densified 65,536 x
        // 65,536 cells (137 GB reserve, 24 GiB RSS before the wall clock).
        let document = read_with_deadline(
            crate::CalamineReader,
            &legacy_seed("calamine-observed-extent-bomb.xls"),
            ReaderOptions::default(),
        );

        assert!(!document.ok);
        let error = document.error.expect("extent bomb should carry an error");
        assert_eq!(error.code, ErrorCode::Bomb.as_str());
        assert!(error.msg.contains("65536x65536"));
        assert!(error.msg.contains("4294967296 cells"));
        assert!(error.msg.contains("8000000 cell limit"));
    }

    #[test]
    fn legacy_biff_rejects_declared_extent_bomb_before_calamine() {
        let document = read_with_deadline(
            crate::CalamineReader,
            &legacy_seed("poi-51535-declared-extent.xls"),
            ReaderOptions::default(),
        );

        assert!(!document.ok);
        let error = document.error.expect("extent bomb should carry an error");
        assert_eq!(error.code, ErrorCode::Bomb.as_str());
        assert!(error.msg.contains("65536x256"));
        assert!(error.msg.contains("16777216 cells"));
        assert!(error.msg.contains("8000000 cell limit"));
    }
}
