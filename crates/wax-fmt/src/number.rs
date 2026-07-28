use crate::parser::{append_literal_atom, Atom, Section};

pub(crate) fn is_supported(section: &Section) -> bool {
    if section
        .atoms
        .iter()
        .any(|atom| matches!(atom, Atom::Elapsed(_, _) | Atom::At))
    {
        return false;
    }
    let placeholder_count = section
        .atoms
        .iter()
        .filter(|atom| matches!(atom, Atom::Placeholder(_)))
        .count();
    if placeholder_count == 0 {
        return true;
    }
    if placeholder_count > 30 {
        return false;
    }
    analyze_scientific(section).is_some()
        || analyze_fraction(section).is_some()
        || analyze_decimal(section).is_some()
}

pub(crate) fn render(section: &Section, value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if !section
        .atoms
        .iter()
        .any(|atom| matches!(atom, Atom::Placeholder(_)))
    {
        let mut output = String::new();
        for atom in &section.atoms {
            append_literal_atom(&mut output, atom, "")?;
        }
        return Some(output);
    }

    let percent_count = section
        .atoms
        .iter()
        .filter(|atom| matches!(atom, Atom::Percent))
        .count() as i32;
    let scaled = value * 100_f64.powi(percent_count);
    if !scaled.is_finite() {
        return None;
    }

    if let Some(scientific) = analyze_scientific(section) {
        return render_scientific(section, scaled, scientific);
    }
    if let Some(fraction) = analyze_fraction(section) {
        return render_fraction(section, scaled, fraction);
    }
    let decimal = analyze_decimal(section)?;
    render_decimal(section, scaled, decimal)
}

#[derive(Clone, Copy, Debug)]
struct Decimal {
    first: usize,
    last: usize,
    decimal: Option<usize>,
    scaling_commas: usize,
    grouped: bool,
    embedded_layout: bool,
}

fn analyze_decimal(section: &Section) -> Option<Decimal> {
    let first = section
        .atoms
        .iter()
        .position(|atom| matches!(atom, Atom::Placeholder(_)))?;
    let last = section
        .atoms
        .iter()
        .rposition(|atom| matches!(atom, Atom::Placeholder(_)))?;
    let decimal_positions = section.atoms[first..=last]
        .iter()
        .enumerate()
        .filter_map(|(offset, atom)| matches!(atom, Atom::Raw('.')).then_some(first + offset))
        .collect::<Vec<_>>();
    if decimal_positions.len() > 1 {
        return None;
    }
    let decimal = decimal_positions.first().copied();

    let mut scaling_commas = 0;
    let mut cursor = last + 1;
    while matches!(section.atoms.get(cursor), Some(Atom::Raw(','))) {
        scaling_commas += 1;
        cursor += 1;
    }
    let integer_end = decimal.unwrap_or(last + 1);
    let grouped = section.atoms[first..integer_end]
        .iter()
        .any(|atom| matches!(atom, Atom::Raw(',')));
    let embedded_layout = decimal.is_none()
        && section.atoms[first..=last]
            .iter()
            .any(|atom| !matches!(atom, Atom::Placeholder(_) | Atom::Raw(',') | Atom::Percent))
        || decimal.is_none()
            && section.atoms[first..=last]
                .iter()
                .any(|atom| matches!(atom, Atom::Raw(character) if *character != ','));

    Some(Decimal {
        first,
        last,
        decimal,
        scaling_commas,
        grouped,
        embedded_layout,
    })
}

fn render_decimal(section: &Section, value: f64, decimal: Decimal) -> Option<String> {
    let automatic_negative = value.is_sign_negative() && value != 0.0;
    let mut magnitude = value.abs() / 1000_f64.powi(decimal.scaling_commas as i32);
    if !magnitude.is_finite() {
        return None;
    }

    if decimal.embedded_layout {
        return render_embedded_integer(section, magnitude, decimal, automatic_negative);
    }

    let decimal_index = decimal.decimal.unwrap_or(decimal.last + 1);
    let integer_placeholders = section.atoms[decimal.first..decimal_index]
        .iter()
        .filter_map(placeholder)
        .collect::<Vec<_>>();
    let fractional_placeholders = decimal
        .decimal
        .map(|index| {
            section.atoms[index + 1..=decimal.last]
                .iter()
                .filter_map(placeholder)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if integer_placeholders.is_empty() && fractional_placeholders.is_empty() {
        return None;
    }
    if fractional_placeholders.len() > 15 {
        return None;
    }

    let precision = fractional_placeholders.len();
    let factor = 10_f64.powi(precision as i32);
    magnitude = (magnitude * factor).round() / factor;
    let fixed = format!("{magnitude:.precision$}");
    let (integer, digits) = fixed
        .split_once('.')
        .map_or((fixed.as_str(), ""), |parts| parts);

    let mandatory_integers = integer_placeholders
        .iter()
        .filter(|placeholder| **placeholder == '0')
        .count();
    let visible_zero = mandatory_integers > 0;
    let integer_is_zero = integer.chars().all(|character| character == '0');
    let mut integer_owned;
    if integer_is_zero && !visible_zero {
        integer_owned = String::new();
    } else {
        integer_owned = integer.to_owned();
        if integer_owned.len() < mandatory_integers {
            integer_owned.insert_str(0, &"0".repeat(mandatory_integers - integer_owned.len()));
        }
        if decimal.grouped {
            integer_owned = group_digits(&integer_owned);
        }
    }
    let ungrouped_digit_count = integer_owned
        .chars()
        .filter(|character| character.is_ascii_digit())
        .count();
    let missing_slots = integer_placeholders
        .len()
        .saturating_sub(ungrouped_digit_count);
    let question_padding = integer_placeholders
        .iter()
        .take(missing_slots)
        .filter(|placeholder| **placeholder == '?')
        .count();
    if question_padding > 0 {
        integer_owned.insert_str(0, &" ".repeat(question_padding));
    }

    let fractional = render_fractional_digits(digits, &fractional_placeholders);
    let show_decimal = decimal.decimal.is_some()
        && (!fractional.is_empty()
            || fractional_placeholders
                .iter()
                .any(|placeholder| *placeholder == '0' || *placeholder == '?'));

    let mut output = String::new();
    if automatic_negative {
        output.push('-');
    }
    append_atoms(
        &mut output,
        &section.atoms[..decimal.first],
        AtomMode::Literal,
    )?;
    output.push_str(&integer_owned);
    if show_decimal {
        output.push('.');
        output.push_str(&fractional);
    }
    let suffix_start = decimal.last + 1 + decimal.scaling_commas;
    append_atoms(
        &mut output,
        &section.atoms[suffix_start..],
        AtomMode::Literal,
    )?;
    Some(output)
}

fn render_fractional_digits(digits: &str, placeholders: &[char]) -> String {
    let bytes = digits.as_bytes();
    let last_nonzero = bytes
        .iter()
        .rposition(|digit| *digit != b'0')
        .map(|index| index + 1)
        .unwrap_or(0);
    let last_required = placeholders
        .iter()
        .rposition(|placeholder| *placeholder == '0')
        .map(|index| index + 1)
        .unwrap_or(0);
    let visible = last_nonzero.max(last_required);
    let mut rendered = String::new();
    for (index, placeholder) in placeholders.iter().enumerate() {
        let digit = bytes.get(index).copied().unwrap_or(b'0') as char;
        match placeholder {
            '0' => rendered.push(digit),
            '#' if index < visible => rendered.push(digit),
            '#' => {}
            '?' if index < visible => rendered.push(digit),
            '?' => rendered.push(' '),
            _ => {}
        }
    }
    rendered
}

fn render_embedded_integer(
    section: &Section,
    value: f64,
    decimal: Decimal,
    automatic_negative: bool,
) -> Option<String> {
    let rounded = value.round();
    let mut digits = format!("{rounded:.0}").into_bytes();
    let mut replacements = vec![None; section.atoms.len()];
    for index in (decimal.first..=decimal.last).rev() {
        if let Atom::Placeholder(placeholder) = section.atoms[index] {
            let digit = digits.pop().map(char::from);
            replacements[index] = Some(match (placeholder, digit) {
                (_, Some(digit)) => digit.to_string(),
                ('0', None) => "0".to_owned(),
                ('?', None) => " ".to_owned(),
                ('#', None) => String::new(),
                _ => return None,
            });
        }
    }

    let mut output = String::new();
    if automatic_negative {
        output.push('-');
    }
    append_atoms(
        &mut output,
        &section.atoms[..decimal.first],
        AtomMode::Literal,
    )?;
    if !digits.is_empty() {
        output.push_str(std::str::from_utf8(&digits).ok()?);
    }
    for (index, atom) in section.atoms[decimal.first..=decimal.last]
        .iter()
        .enumerate()
    {
        let absolute_index = decimal.first + index;
        if let Some(replacement) = &replacements[absolute_index] {
            output.push_str(replacement);
        } else {
            append_numeric_literal(&mut output, atom)?;
        }
    }
    append_atoms(
        &mut output,
        &section.atoms[decimal.last + 1..],
        AtomMode::Literal,
    )?;
    Some(output)
}

#[derive(Clone, Copy, Debug)]
struct Scientific {
    first: usize,
    exponent_first: usize,
    exponent_last: usize,
    integer_places: usize,
    fractional_places: usize,
    marker: char,
    sign: Option<char>,
}

fn analyze_scientific(section: &Section) -> Option<Scientific> {
    for (index, atom) in section.atoms.iter().enumerate() {
        let Atom::Raw(marker @ ('e' | 'E')) = atom else {
            continue;
        };
        let first = section.atoms[..index]
            .iter()
            .position(|atom| matches!(atom, Atom::Placeholder(_)))?;
        let mut cursor = index + 1;
        let sign = match section.atoms.get(cursor) {
            Some(Atom::Raw(sign @ ('+' | '-'))) => {
                cursor += 1;
                Some(*sign)
            }
            _ => None,
        };
        let exponent_first = cursor;
        while matches!(
            section.atoms.get(cursor),
            Some(Atom::Placeholder('0' | '#' | '?'))
        ) {
            cursor += 1;
        }
        if cursor == exponent_first {
            continue;
        }
        let exponent_last = cursor - 1;
        let mantissa = &section.atoms[first..index];
        let decimal = mantissa
            .iter()
            .position(|atom| matches!(atom, Atom::Raw('.')));
        let integer_places = mantissa[..decimal.unwrap_or(mantissa.len())]
            .iter()
            .filter(|atom| matches!(atom, Atom::Placeholder(_)))
            .count();
        let fractional_places = decimal.map_or(0, |decimal| {
            mantissa[decimal + 1..]
                .iter()
                .filter(|atom| matches!(atom, Atom::Placeholder(_)))
                .count()
        });
        if integer_places == 0 || fractional_places > 15 {
            return None;
        }
        return Some(Scientific {
            first,
            exponent_first,
            exponent_last,
            integer_places,
            fractional_places,
            marker: *marker,
            sign,
        });
    }
    None
}

fn render_scientific(section: &Section, value: f64, scientific: Scientific) -> Option<String> {
    let automatic_negative = value.is_sign_negative() && value != 0.0;
    let magnitude = value.abs();
    let mut exponent = if magnitude == 0.0 {
        0
    } else {
        magnitude.log10().floor() as i32
    };
    if scientific.integer_places > 1 {
        exponent = exponent.div_euclid(scientific.integer_places as i32)
            * scientific.integer_places as i32;
    }
    let mut mantissa = if magnitude == 0.0 {
        0.0
    } else {
        magnitude / 10_f64.powi(exponent)
    };
    let factor = 10_f64.powi(scientific.fractional_places as i32);
    mantissa = (mantissa * factor).round() / factor;
    let upper = 10_f64.powi(scientific.integer_places as i32);
    if mantissa >= upper {
        mantissa /= upper;
        exponent += scientific.integer_places as i32;
    }

    let mut output = String::new();
    if automatic_negative {
        output.push('-');
    }
    append_atoms(
        &mut output,
        &section.atoms[..scientific.first],
        AtomMode::Literal,
    )?;
    output.push_str(&format!(
        "{mantissa:.precision$}",
        precision = scientific.fractional_places
    ));
    output.push(scientific.marker);
    if exponent < 0 {
        output.push('-');
    } else if scientific.sign == Some('+') {
        output.push('+');
    }
    let exponent_width = section.atoms[scientific.exponent_first..=scientific.exponent_last]
        .iter()
        .filter(|atom| matches!(atom, Atom::Placeholder('0')))
        .count();
    output.push_str(&format!(
        "{:0width$}",
        exponent.unsigned_abs(),
        width = exponent_width
    ));
    append_atoms(
        &mut output,
        &section.atoms[scientific.exponent_last + 1..],
        AtomMode::Literal,
    )?;
    Some(output)
}

#[derive(Clone, Copy, Debug)]
struct Fraction {
    first: usize,
    numerator_first: usize,
    numerator_last: usize,
    denominator_first: usize,
    denominator_last: usize,
    fixed_denominator: Option<u64>,
}

fn analyze_fraction(section: &Section) -> Option<Fraction> {
    for (slash, atom) in section.atoms.iter().enumerate() {
        if !matches!(atom, Atom::Raw('/')) {
            continue;
        }
        let mut numerator_first = slash;
        while numerator_first > 0
            && matches!(
                section.atoms[numerator_first - 1],
                Atom::Placeholder('0' | '#' | '?')
            )
        {
            numerator_first -= 1;
        }
        if numerator_first == slash {
            continue;
        }
        let numerator_last = slash - 1;

        let denominator_first = slash + 1;
        let mut denominator_last = denominator_first;
        while matches!(
            section.atoms.get(denominator_last),
            Some(Atom::Placeholder('0' | '#' | '?')) | Some(Atom::Raw('0'..='9'))
        ) {
            denominator_last += 1;
        }
        if denominator_last == denominator_first {
            continue;
        }
        denominator_last -= 1;

        let first = section.atoms[..numerator_first]
            .iter()
            .position(|atom| matches!(atom, Atom::Placeholder(_)))
            .unwrap_or(numerator_first);
        let denominator_text = section.atoms[denominator_first..=denominator_last]
            .iter()
            .map(|atom| match atom {
                Atom::Raw(character @ '0'..='9') => Some(*character),
                Atom::Placeholder('0') => Some('0'),
                _ => None,
            })
            .collect::<Option<String>>();
        let fixed_denominator = denominator_text.and_then(|value| value.parse().ok());

        return Some(Fraction {
            first,
            numerator_first,
            numerator_last,
            denominator_first,
            denominator_last,
            fixed_denominator,
        });
    }
    None
}

fn render_fraction(section: &Section, value: f64, fraction: Fraction) -> Option<String> {
    let automatic_negative = value.is_sign_negative() && value != 0.0;
    let magnitude = value.abs();
    let whole_placeholders = section.atoms[fraction.first..fraction.numerator_first]
        .iter()
        .filter(|atom| matches!(atom, Atom::Placeholder(_)))
        .count();
    let mixed = whole_placeholders > 0;
    let whole = if mixed { magnitude.floor() } else { 0.0 };
    let fractional_value = if mixed { magnitude - whole } else { magnitude };

    let denominator_width = fraction.denominator_last - fraction.denominator_first + 1;
    let max_denominator = fraction.fixed_denominator.unwrap_or_else(|| {
        10_u64
            .saturating_pow(denominator_width as u32)
            .saturating_sub(1)
    });
    if max_denominator == 0 {
        return None;
    }
    let (mut numerator, denominator) = if let Some(fixed) = fraction.fixed_denominator {
        ((fractional_value * fixed as f64).round() as u64, fixed)
    } else {
        approximate_fraction(fractional_value, max_denominator)
    };
    let mut whole = whole as u64;
    if mixed && numerator >= denominator {
        whole = whole.saturating_add(numerator / denominator);
        numerator %= denominator;
    }

    let mut output = String::new();
    if automatic_negative {
        output.push('-');
    }
    append_atoms(
        &mut output,
        &section.atoms[..fraction.first],
        AtomMode::Literal,
    )?;
    if mixed {
        let whole_atoms = &section.atoms[fraction.first..fraction.numerator_first];
        if whole == 0 && numerator == 0 {
            output.push('0');
        } else {
            output.push_str(&render_integer_mask(whole, whole_atoms, false)?);
        }
    }

    let between_start = if mixed {
        section.atoms[fraction.first..fraction.numerator_first]
            .iter()
            .rposition(|atom| matches!(atom, Atom::Placeholder(_)))
            .map(|offset| fraction.first + offset + 1)
            .unwrap_or(fraction.first)
    } else {
        fraction.first
    };
    append_atoms(
        &mut output,
        &section.atoms[between_start..fraction.numerator_first],
        AtomMode::Literal,
    )?;

    if numerator == 0 {
        output.push_str(&" ".repeat(
            mask_width(&section.atoms[fraction.numerator_first..=fraction.numerator_last])
                + 1
                + mask_width(
                    &section.atoms[fraction.denominator_first..=fraction.denominator_last],
                ),
        ));
    } else {
        output.push_str(&render_integer_mask(
            numerator,
            &section.atoms[fraction.numerator_first..=fraction.numerator_last],
            true,
        )?);
        output.push('/');
        let denominator_mask =
            &section.atoms[fraction.denominator_first..=fraction.denominator_last];
        if fraction.fixed_denominator.is_some() {
            output.push_str(&denominator.to_string());
        } else {
            let rendered = denominator.to_string();
            output.push_str(&rendered);
            let question_padding = denominator_mask
                .iter()
                .filter(|atom| matches!(atom, Atom::Placeholder('?')))
                .count()
                .saturating_sub(rendered.len());
            output.push_str(&" ".repeat(question_padding));
        }
    }
    append_atoms(
        &mut output,
        &section.atoms[fraction.denominator_last + 1..],
        AtomMode::Literal,
    )?;
    Some(output)
}

fn approximate_fraction(value: f64, max_denominator: u64) -> (u64, u64) {
    if value <= 0.0 {
        return (0, 1);
    }
    let whole = value.floor();
    let fraction = value - whole;
    if fraction <= f64::EPSILON {
        return (whole as u64, 1);
    }

    let mut previous_numerator = 0_u64;
    let mut previous_denominator = 1_u64;
    let mut numerator = 1_u64;
    let mut denominator = 0_u64;
    let mut remainder = fraction;

    loop {
        let coefficient = remainder.floor().max(0.0) as u64;
        let next_numerator = coefficient
            .saturating_mul(numerator)
            .saturating_add(previous_numerator);
        let next_denominator = coefficient
            .saturating_mul(denominator)
            .saturating_add(previous_denominator);

        if next_denominator > max_denominator {
            let multiplier = max_denominator
                .saturating_sub(previous_denominator)
                .checked_div(denominator)
                .unwrap_or(0);
            let bounded_numerator = multiplier
                .saturating_mul(numerator)
                .saturating_add(previous_numerator);
            let bounded_denominator = multiplier
                .saturating_mul(denominator)
                .saturating_add(previous_denominator);
            let convergent_error = (fraction - numerator as f64 / denominator.max(1) as f64).abs();
            let bounded_error = if bounded_denominator == 0 {
                f64::INFINITY
            } else {
                (fraction - bounded_numerator as f64 / bounded_denominator as f64).abs()
            };
            let (chosen_numerator, chosen_denominator) =
                if denominator > 0 && convergent_error <= bounded_error {
                    (numerator, denominator)
                } else {
                    (bounded_numerator, bounded_denominator.max(1))
                };
            return (
                (whole as u64)
                    .saturating_mul(chosen_denominator)
                    .saturating_add(chosen_numerator),
                chosen_denominator,
            );
        }

        previous_numerator = numerator;
        previous_denominator = denominator;
        numerator = next_numerator;
        denominator = next_denominator;

        let fractional_remainder = remainder - coefficient as f64;
        if fractional_remainder.abs() <= f64::EPSILON {
            return (
                (whole as u64)
                    .saturating_mul(denominator)
                    .saturating_add(numerator),
                denominator.max(1),
            );
        }
        remainder = fractional_remainder.recip();
    }
}

fn render_integer_mask(value: u64, atoms: &[Atom], align_right: bool) -> Option<String> {
    let width = mask_width(atoms);
    let placeholder_types = atoms.iter().filter_map(placeholder).collect::<Vec<_>>();
    if placeholder_types.is_empty() {
        return None;
    }
    if value == 0 && !placeholder_types.contains(&'0') {
        return Some(
            placeholder_types
                .iter()
                .filter_map(|placeholder| (*placeholder == '?').then_some(' '))
                .collect(),
        );
    }
    let rendered = value.to_string();
    if rendered.len() >= width {
        return Some(rendered);
    }
    let padding = width - rendered.len();
    let pad_character = if placeholder_types
        .iter()
        .take(padding)
        .any(|placeholder| *placeholder == '0')
    {
        '0'
    } else if align_right
        && placeholder_types
            .iter()
            .take(padding)
            .any(|placeholder| *placeholder == '?')
    {
        ' '
    } else {
        '\0'
    };
    let mut output = String::new();
    if pad_character != '\0' {
        output.push_str(&pad_character.to_string().repeat(padding));
    }
    output.push_str(&rendered);
    Some(output)
}

fn mask_width(atoms: &[Atom]) -> usize {
    atoms
        .iter()
        .filter(|atom| matches!(atom, Atom::Placeholder(_) | Atom::Raw('0'..='9')))
        .count()
}

fn placeholder(atom: &Atom) -> Option<char> {
    match atom {
        Atom::Placeholder(value) => Some(*value),
        _ => None,
    }
}

fn group_digits(digits: &str) -> String {
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    let first_group = digits.len() % 3;
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && index % 3 == first_group {
            output.push(',');
        }
        output.push(character);
    }
    output
}

#[derive(Clone, Copy)]
enum AtomMode {
    Literal,
}

fn append_atoms(output: &mut String, atoms: &[Atom], _mode: AtomMode) -> Option<()> {
    for atom in atoms {
        append_numeric_literal(output, atom)?;
    }
    Some(())
}

fn append_numeric_literal(output: &mut String, atom: &Atom) -> Option<()> {
    match atom {
        Atom::Literal(value) => output.push_str(value),
        Atom::Raw(character) => output.push(*character),
        Atom::Percent => output.push('%'),
        Atom::Placeholder('?') => output.push(' '),
        Atom::Placeholder(_) => {}
        Atom::At | Atom::Elapsed(_, _) => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use crate::{render, FmtValue};

    fn number(code: &str, value: f64) -> String {
        render(code, FmtValue::Number(value), false).unwrap()
    }

    #[test]
    fn fixed_decimals_grouping_and_sections() {
        assert_eq!(number("#,##0.00", 12_410.5), "12,410.50");
        assert_eq!(number("0.00", 0.1 + 0.2), "0.30");
        assert_eq!(number("#,##0 ;[Red](#,##0)", -1234.5), "(1,235)");
        assert_eq!(number("###0.00;-###0.00", -2.5), "-2.50");
    }

    #[test]
    fn percentages_currency_and_accounting() {
        assert_eq!(number("0.0%", 0.125), "12.5%");
        assert_eq!(number("\"$\"#,##0.00", 12.5), "$12.50");
        assert_eq!(
            number(
                "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)",
                -1234.5
            ),
            " $(1,234.50)"
        );
        assert_eq!(
            number(
                "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)",
                0.0
            ),
            " $-   "
        );
    }

    #[test]
    fn scientific_notation() {
        assert_eq!(number("0.00E+00", 1234.5), "1.23E+03");
        assert_eq!(number("0.00E+00", 0.25), "2.50E-01");
        assert_eq!(number("##0.0E+0", 12_345.0), "12.3E+3");
    }

    #[test]
    fn fractions_and_fixed_denominators() {
        assert_eq!(number("# ?/?", 1234.5), "1234 1/2");
        assert_eq!(number("# ??/??", 0.25), "  1/4 ");
        assert_eq!(number("# ?/8", 1.25), "1 2/8");
    }

    #[test]
    fn embedded_digit_masks() {
        assert_eq!(number("00000\\-0000", 1_234_567_890.0), "123456-7890");
        assert_eq!(number("00000\\-0000", 12.0), "00000-0012");
    }

    #[test]
    fn optional_digits_scaling_and_literal_suffixes() {
        assert_eq!(number("0.##", 1.2), "1.2");
        assert_eq!(number("#.##", 0.25), ".25");
        assert_eq!(number("#,##0,,\"M\"", 123_456_789.0), "123M");
        assert_eq!(number("#,##0.00 kr", 12_410.5), "12,410.50 kr");
    }
}
