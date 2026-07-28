#![no_main]

use std::io::Write;

use libfuzzer_sys::fuzz_target;
use wax_read::{read_with_deadline, CalamineReader, ReaderOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(mut input) = tempfile::Builder::new().suffix(".xls").tempfile() else {
        return;
    };
    if input.write_all(data).is_err() {
        return;
    }
    let options = ReaderOptions {
        max_cells: 10_000,
        timeout_ms: 50,
        max_bytes: 1024 * 1024,
        ..ReaderOptions::default()
    };
    let _ = read_with_deadline(CalamineReader, input.path(), options);
});
