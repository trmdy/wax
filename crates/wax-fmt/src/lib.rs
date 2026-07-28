//! ECMA-376 number-format rendering.
//!
//! The renderer is deliberately conservative: syntactically unsupported
//! formats return `None`, allowing callers to expose an honest `d: null`.
//! Formatting is locale-invariant in v1 and uses Excel's en-US separators.

mod datetime;
mod general;
mod number;
mod parser;

use parser::{parse_format, select_numeric_section};

/// A raw cell value as read from the file, before display formatting.
///
/// Date/time cells are passed as `Number` carrying the raw Excel serial — the
/// renderer, not the reader, decides how a date-format code displays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FmtValue<'a> {
    Number(f64),
    Text(&'a str),
    Bool(bool),
    Error(&'a str),
}

/// Return whether the syntax and semantic family of a format code are
/// supported by this interpreter.
///
/// This additive API is useful for corpus coverage reporting. It does not
/// promise that every possible value can be rendered: non-finite numbers and
/// out-of-range dates still return `None` from [`render`].
pub fn is_supported(code: &str) -> bool {
    parse_format(code)
        .map(|format| format.sections.iter().all(parser::section_is_supported))
        .unwrap_or(false)
}

/// Render the display string Excel would show for `value` under the
/// number-format code `code` (`"General"` included).
///
/// `epoch_1904` selects the workbook date epoch for date/time codes. Returns
/// `None` when the code or value/code combination is not supported.
pub fn render(code: &str, value: FmtValue<'_>, epoch_1904: bool) -> Option<String> {
    let format = parse_format(code)?;
    match value {
        FmtValue::Text(text) => render_text(&format, text),
        FmtValue::Bool(value) => Some(if value { "TRUE" } else { "FALSE" }.to_owned()),
        FmtValue::Error(value) => Some(value.to_owned()),
        FmtValue::Number(value) => {
            if !value.is_finite() {
                return None;
            }
            let selection = select_numeric_section(&format.sections, value)?;
            let section = &format.sections[selection.index];
            if datetime::is_datetime(section) {
                datetime::render(section, value, epoch_1904)
            } else if parser::is_general(section) {
                let rendered = general::render(if selection.absolute {
                    value.abs()
                } else {
                    value
                })?;
                parser::render_general_section(section, &rendered)
            } else if parser::has_text_placeholder(section) {
                let rendered = general::render(if selection.absolute {
                    value.abs()
                } else {
                    value
                })?;
                parser::render_text_section(section, &rendered)
            } else {
                number::render(
                    section,
                    if selection.absolute {
                        value.abs()
                    } else {
                        value
                    },
                )
            }
        }
    }
}

fn render_text(format: &parser::Format, text: &str) -> Option<String> {
    let section = if format.sections.len() == 4 {
        &format.sections[3]
    } else {
        // Excel leaves text unchanged when no fourth text section is present.
        return Some(text.to_owned());
    };
    parser::render_text_section(section, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_numeric_values() {
        assert_eq!(
            render("@", FmtValue::Text("hello"), false).as_deref(),
            Some("hello")
        );
        assert_eq!(
            render("0;[Red]-0;0;\"item: \"@", FmtValue::Text("pen"), false).as_deref(),
            Some("item: pen")
        );
        assert_eq!(
            render("0.00", FmtValue::Bool(true), false).as_deref(),
            Some("TRUE")
        );
        assert_eq!(
            render("General", FmtValue::Error("#DIV/0!"), false).as_deref(),
            Some("#DIV/0!")
        );
    }

    #[test]
    fn text_placeholder_uses_general_for_numbers() {
        assert_eq!(
            render("@", FmtValue::Number(1234.5), false).as_deref(),
            Some("1234.5")
        );
    }

    #[test]
    fn renders_sign_zero_and_conditional_sections() {
        let code = "0.0;[Red](0.0);\"-\";\"text: \"@";
        assert_eq!(
            render(code, FmtValue::Number(2.25), false).as_deref(),
            Some("2.3")
        );
        assert_eq!(
            render(code, FmtValue::Number(-2.25), false).as_deref(),
            Some("(2.3)")
        );
        assert_eq!(
            render(code, FmtValue::Number(0.0), false).as_deref(),
            Some("-")
        );
        assert_eq!(
            render(code, FmtValue::Text("value"), false).as_deref(),
            Some("text: value")
        );

        let conditional = "[<0]\"negative\";[=0]\"zero\";0";
        assert_eq!(
            render(conditional, FmtValue::Number(-1.0), false).as_deref(),
            Some("negative")
        );
        assert_eq!(
            render(conditional, FmtValue::Number(0.0), false).as_deref(),
            Some("zero")
        );
        assert_eq!(
            render(conditional, FmtValue::Number(42.0), false).as_deref(),
            Some("42")
        );
    }

    #[test]
    fn refuses_non_finite_numbers_and_invalid_formats() {
        assert_eq!(render("General", FmtValue::Number(f64::NAN), false), None);
        assert_eq!(
            render("[not-a-directive]0", FmtValue::Number(1.0), false),
            None
        );
        assert!(!is_supported("plain junk"));
    }
}
