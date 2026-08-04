#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(formula) = std::str::from_utf8(data) {
        wax_eval::fuzz_parse_formula(formula);

        // Exercise the same discriminator and shape parsing used by
        // recalc/export before feeding override-sourced formula text into
        // the parser. `json!` keeps arbitrary UTF-8 safely escaped.
        let request_value = serde_json::json!([{
            "sheet": 0,
            "r": 0,
            "c": 0,
            "f": formula,
        }]);
        if let Ok(overrides) = wax_proto::parse_overrides(&request_value) {
            for entry in overrides {
                if let Some(formula) = entry.f {
                    wax_eval::fuzz_parse_formula(&formula);
                }
            }
        }
    }
});
