#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(formula) = std::str::from_utf8(data) {
        wax_eval::fuzz_parse_formula(formula);
    }
});
