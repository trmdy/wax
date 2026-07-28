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
    if input_bytes < sector_bytes || !input_bytes.is_multiple_of(sector_bytes) {
        return Err(SafetyError::new(
            ErrorCode::BadZip,
            format!("XLS compound document has a partial {sector_bytes} byte sector"),
        ));
    }

    preflight_biff_records(path, options)
}

fn preflight_biff_records(path: &Path, options: ReaderOptions) -> Result<(), SafetyError> {
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
    let stream_bytes = stream.len();
    let mut consumed = 0_u64;
    let mut records = 0_usize;
    while consumed < stream_bytes {
        records = records
            .checked_add(1)
            .ok_or_else(|| SafetyError::new(ErrorCode::TooLarge, "BIFF record count overflowed"))?;
        if records > MAX_BIFF_RECORDS {
            return Err(SafetyError::new(
                ErrorCode::TooLarge,
                format!("XLS Workbook stream exceeds the {MAX_BIFF_RECORDS} record limit"),
            ));
        }
        if stream_bytes - consumed < 4 {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                "truncated BIFF record header",
            ));
        }

        let mut header = [0_u8; 4];
        stream.read_exact(&mut header).map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("could not read BIFF record header: {error}"),
            )
        })?;
        consumed += 4;
        let kind = u16::from_le_bytes([header[0], header[1]]);
        let record_bytes = u16::from_le_bytes([header[2], header[3]]) as usize;
        let record_bytes_u64 = record_bytes as u64;
        if record_bytes_u64 > stream_bytes - consumed {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                format!("truncated BIFF record 0x{kind:04X}"),
            ));
        }
        let mut data = vec![0_u8; record_bytes];
        stream.read_exact(&mut data).map_err(|error| {
            SafetyError::new(
                ErrorCode::BadZip,
                format!("could not read BIFF record 0x{kind:04X}: {error}"),
            )
        })?;
        consumed += record_bytes_u64;

        if kind == 0x0809 && data.len() < 2 {
            return Err(SafetyError::new(
                ErrorCode::BadZip,
                "BIFF BOF record is shorter than 2 bytes",
            ));
        }
        if kind == 0x0200 {
            check_declared_extent(&data, options.max_declared_cells)?;
        }
    }

    Ok(())
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

    if first_col > 0xff || last_col < first_col {
        first_col = 0;
    }
    let (rows, cols) = if last_row >= 1 && last_col >= 1 {
        let rows = last_row.checked_sub(first_row).ok_or_else(|| {
            SafetyError::new(ErrorCode::BadZip, "BIFF DIMENSIONS row bounds are reversed")
        })?;
        let cols = last_col.checked_sub(first_col).ok_or_else(|| {
            SafetyError::new(
                ErrorCode::BadZip,
                "BIFF DIMENSIONS column bounds are reversed",
            )
        })?;
        (rows, cols)
    } else {
        (1, 1)
    };
    let cells = u64::from(rows)
        .checked_mul(u64::from(cols))
        .ok_or_else(|| SafetyError::new(ErrorCode::Bomb, "declared XLS extent overflowed"))?;
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
            Event::Start(_) => {
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
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
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
            std::thread::sleep(Duration::from_millis(250));
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

        assert!(started.elapsed() < Duration::from_millis(100));
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
    fn legacy_cfb_rejects_the_partial_sector_fuzz_regression() {
        let source = include_bytes!("../tests/fixtures/date.xls");
        let mut input = tempfile::Builder::new()
            .suffix(".xls")
            .tempfile()
            .expect("temporary XLS should be created");
        input
            .write_all(&source[..source.len() - 1])
            .expect("truncated XLS should be written");

        let error = preflight_path(input.path(), ReaderOptions::default())
            .expect_err("partial CFB sector should be rejected");

        assert_eq!(error.code(), ErrorCode::BadZip);
        assert!(error.message().contains("partial"));
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
