const SIGNIFICANT_DIGITS: i32 = 11;

pub(crate) fn render(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some("0".to_owned());
    }

    let absolute = value.abs();
    let exponent = absolute.log10().floor() as i32;
    if exponent >= SIGNIFICANT_DIGITS || exponent <= -10 {
        return Some(scientific(value, exponent));
    }

    let decimal_places = (SIGNIFICANT_DIGITS - exponent - 1).clamp(0, 15) as usize;
    let mut rendered = format!("{value:.decimal_places$}");
    if rendered.contains('.') {
        while rendered.ends_with('0') {
            rendered.pop();
        }
        if rendered.ends_with('.') {
            rendered.pop();
        }
    }
    if rendered == "-0" {
        rendered.remove(0);
    }
    Some(rendered)
}

fn scientific(value: f64, exponent: i32) -> String {
    let mut mantissa = value / 10_f64.powi(exponent);
    let scale = 10_f64.powi(SIGNIFICANT_DIGITS - 1);
    mantissa = (mantissa * scale).round() / scale;
    let mut exponent = exponent;
    if mantissa.abs() >= 10.0 {
        mantissa /= 10.0;
        exponent += 1;
    }

    let mut rendered = format!("{mantissa:.10}");
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    format!("{rendered}E{exponent:+}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppresses_binary_float_noise() {
        assert_eq!(render(0.1 + 0.2).as_deref(), Some("0.3"));
        assert_eq!(render(1.2300000000004).as_deref(), Some("1.23"));
    }

    #[test]
    fn switches_to_scientific_at_excel_thresholds() {
        assert_eq!(render(99_999_999_999.0).as_deref(), Some("99999999999"));
        assert_eq!(
            render(123_456_789_012.0).as_deref(),
            Some("1.2345678901E+11")
        );
        assert_eq!(render(0.000_000_001).as_deref(), Some("0.000000001"));
        assert_eq!(render(0.000_000_000_1).as_deref(), Some("1E-10"));
    }
}
