//! ECMA-376 number-format rendering.
//!
//! W2 seam: `wax-read` calls [`render`] to produce the display string (`d`)
//! for every cell. This crate ships as a stub until shard W2B lands the real
//! interpreter; the stub returns `None` for every input, which callers must
//! surface as `d: null` (honest zero display coverage, never a guess).
//!
//! The signature below is frozen by `docs/w2-contracts.md` §2. W2B implements
//! behind it without changing it; changes go through the coordinator.

/// A raw cell value as read from the file, before display formatting.
///
/// Date/time cells are passed as `Number` carrying the raw Excel serial —
/// the renderer, not the reader, decides how a date-format code displays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FmtValue<'a> {
    Number(f64),
    Text(&'a str),
    Bool(bool),
    Error(&'a str),
}

/// Render the display string Excel would show for `value` under the
/// number-format code `code` (`"General"` included).
///
/// `epoch_1904` selects the workbook date epoch for date/time codes.
/// Returns `None` when the code (or value/code combination) is not yet
/// supported; callers emit `d: null` in that case.
pub fn render(code: &str, value: FmtValue<'_>, epoch_1904: bool) -> Option<String> {
    let _ = (code, value, epoch_1904);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_renders_nothing() {
        assert_eq!(render("General", FmtValue::Number(1.5), false), None);
        assert_eq!(render("#,##0.00", FmtValue::Number(12410.5), false), None);
        assert_eq!(render("@", FmtValue::Text("hi"), true), None);
    }
}
