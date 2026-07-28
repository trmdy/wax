#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use wax_read::{preflight_container, ReaderOptions};

fuzz_target!(|data: &[u8]| {
    let options = ReaderOptions {
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
    let _ = preflight_container(Cursor::new(data), options);
});
