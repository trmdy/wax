#![no_main]

use std::io::Write;

use libfuzzer_sys::fuzz_target;
use wax_read::{read_with_deadline, CalamineReader, ReaderOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(mut input) = tempfile::Builder::new().suffix(".xlsx").tempfile() else {
        return;
    };
    if input.write_all(data).is_err() {
        return;
    }
    let options = ReaderOptions {
        max_cells: 10_000,
        timeout_ms: 50,
        max_bytes: 1024 * 1024,
        max_zip_entries: 256,
        max_part_bytes: 4 * 1024 * 1024,
        max_total_uncompressed_bytes: 8 * 1024 * 1024,
        compression_ratio_min_bytes: 64 * 1024,
        max_xml_depth: 64,
        max_xml_token_bytes: 64 * 1024,
        max_xml_tokens: 100_000,
        max_xml_bytes: 4 * 1024 * 1024,
        ..ReaderOptions::default()
    };
    let _ = read_with_deadline(CalamineReader, input.path(), options);
});
