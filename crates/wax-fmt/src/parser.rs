#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Comparison {
    Less,
    LessEqual,
    Equal,
    NotEqual,
    GreaterEqual,
    Greater,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Condition {
    comparison: Comparison,
    threshold: f64,
}

impl Condition {
    fn matches(self, value: f64) -> bool {
        match self.comparison {
            Comparison::Less => value < self.threshold,
            Comparison::LessEqual => value <= self.threshold,
            Comparison::Equal => value == self.threshold,
            Comparison::NotEqual => value != self.threshold,
            Comparison::GreaterEqual => value >= self.threshold,
            Comparison::Greater => value > self.threshold,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ElapsedUnit {
    Hour,
    Minute,
    Second,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Atom {
    Literal(String),
    Placeholder(char),
    Percent,
    At,
    Elapsed(ElapsedUnit, usize),
    Raw(char),
}

#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub(crate) atoms: Vec<Atom>,
    pub(crate) condition: Option<Condition>,
}

#[derive(Clone, Debug)]
pub(crate) struct Format {
    pub(crate) sections: Vec<Section>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Selection {
    pub(crate) index: usize,
    pub(crate) absolute: bool,
}

pub(crate) fn parse_format(code: &str) -> Option<Format> {
    if code.is_empty() {
        return None;
    }
    let raw_sections = split_sections(code)?;
    if raw_sections.is_empty() || raw_sections.len() > 4 {
        return None;
    }
    let sections = raw_sections
        .into_iter()
        .map(parse_section)
        .collect::<Option<Vec<_>>>()?;
    Some(Format { sections })
}

fn split_sections(code: &str) -> Option<Vec<&str>> {
    let mut sections = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    let mut bracketed = false;

    for (index, character) in code.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && !quoted {
            escaped = true;
            continue;
        }
        if character == '"' && !bracketed {
            quoted = !quoted;
            continue;
        }
        if !quoted {
            match character {
                '[' if !bracketed => bracketed = true,
                ']' if bracketed => bracketed = false,
                ';' if !bracketed => {
                    sections.push(&code[start..index]);
                    start = index + character.len_utf8();
                }
                _ => {}
            }
        }
    }
    if quoted || escaped || bracketed {
        return None;
    }
    sections.push(&code[start..]);
    Some(sections)
}

fn parse_section(section: &str) -> Option<Section> {
    let characters = section.chars().collect::<Vec<_>>();
    let mut atoms = Vec::new();
    let mut condition = None;
    let mut index = 0;

    while index < characters.len() {
        match characters[index] {
            '"' => {
                index += 1;
                let start = index;
                while index < characters.len() && characters[index] != '"' {
                    index += 1;
                }
                if index == characters.len() {
                    return None;
                }
                push_literal(
                    &mut atoms,
                    characters[start..index].iter().collect::<String>(),
                );
                index += 1;
            }
            '\\' => {
                index += 1;
                let character = *characters.get(index)?;
                push_literal(&mut atoms, character.to_string());
                index += 1;
            }
            '_' => {
                // Excel reserves the width of the following glyph. Without
                // font metrics, one ordinary space is the deterministic v1
                // approximation used by the SheetJS baseline too.
                index += 1;
                characters.get(index)?;
                push_literal(&mut atoms, " ".to_owned());
                index += 1;
            }
            '*' => {
                // Fill depends on column width, which is intentionally absent
                // from the frozen API. Consume the fill glyph as a no-op.
                index += 1;
                characters.get(index)?;
                index += 1;
            }
            '[' => {
                let start = index + 1;
                index = start;
                while index < characters.len() && characters[index] != ']' {
                    index += 1;
                }
                if index == characters.len() {
                    return None;
                }
                let content = characters[start..index].iter().collect::<String>();
                index += 1;
                if let Some(parsed) = parse_condition(&content) {
                    if condition.replace(parsed).is_some() {
                        return None;
                    }
                } else if is_color(&content) {
                    continue;
                } else if let Some((unit, width)) = parse_elapsed(&content) {
                    atoms.push(Atom::Elapsed(unit, width));
                } else if let Some(currency) = parse_locale_currency(&content) {
                    if !currency.is_empty() {
                        push_literal(&mut atoms, currency);
                    }
                } else {
                    return None;
                }
            }
            '0' | '#' | '?' => {
                atoms.push(Atom::Placeholder(characters[index]));
                index += 1;
            }
            '%' => {
                atoms.push(Atom::Percent);
                index += 1;
            }
            '@' => {
                atoms.push(Atom::At);
                index += 1;
            }
            character => {
                atoms.push(Atom::Raw(character));
                index += 1;
            }
        }
    }

    Some(Section { atoms, condition })
}

fn push_literal(atoms: &mut Vec<Atom>, value: String) {
    if value.is_empty() {
        return;
    }
    if let Some(Atom::Literal(existing)) = atoms.last_mut() {
        existing.push_str(&value);
    } else {
        atoms.push(Atom::Literal(value));
    }
}

fn parse_condition(content: &str) -> Option<Condition> {
    const OPERATORS: [(&str, Comparison); 6] = [
        ("<=", Comparison::LessEqual),
        (">=", Comparison::GreaterEqual),
        ("<>", Comparison::NotEqual),
        ("<", Comparison::Less),
        ("=", Comparison::Equal),
        (">", Comparison::Greater),
    ];
    for (operator, comparison) in OPERATORS {
        if let Some(raw) = content.strip_prefix(operator) {
            let threshold = raw.parse::<f64>().ok()?;
            if threshold.is_finite() {
                return Some(Condition {
                    comparison,
                    threshold,
                });
            }
        }
    }
    None
}

fn is_color(content: &str) -> bool {
    matches!(
        content.to_ascii_lowercase().as_str(),
        "black" | "blue" | "cyan" | "green" | "magenta" | "red" | "white" | "yellow"
    ) || content
        .to_ascii_lowercase()
        .strip_prefix("color")
        .is_some_and(|number| {
            number
                .parse::<u8>()
                .is_ok_and(|value| (1..=56).contains(&value))
        })
}

fn parse_elapsed(content: &str) -> Option<(ElapsedUnit, usize)> {
    let lowercase = content.to_ascii_lowercase();
    let unit = match lowercase.as_str() {
        "h" | "hh" => ElapsedUnit::Hour,
        "m" | "mm" => ElapsedUnit::Minute,
        "s" | "ss" => ElapsedUnit::Second,
        _ => return None,
    };
    Some((unit, content.chars().count()))
}

fn parse_locale_currency(content: &str) -> Option<String> {
    let value = content.strip_prefix('$')?;
    if value.starts_with('-') {
        return Some(String::new());
    }
    let currency = value.split_once('-').map_or(value, |(symbol, _)| symbol);
    Some(currency.to_owned())
}

pub(crate) fn select_numeric_section(sections: &[Section], value: f64) -> Option<Selection> {
    if sections.is_empty() {
        return None;
    }
    let numeric_len = sections.len().min(3);
    let has_conditions = sections[..numeric_len]
        .iter()
        .any(|section| section.condition.is_some());

    if has_conditions {
        let mut fallback = None;
        for (index, section) in sections[..numeric_len].iter().enumerate() {
            match section.condition {
                Some(condition) if condition.matches(value) => {
                    return Some(Selection {
                        index,
                        absolute: false,
                    });
                }
                Some(_) => {}
                None if fallback.is_none() => fallback = Some(index),
                None => {}
            }
        }
        return fallback.map(|index| Selection {
            index,
            absolute: false,
        });
    }

    let (index, absolute) = match numeric_len {
        1 => (0, false),
        2 if value < 0.0 => (1, true),
        2 => (0, false),
        _ if value > 0.0 => (0, false),
        _ if value < 0.0 => (1, true),
        _ => (2, false),
    };
    Some(Selection { index, absolute })
}

pub(crate) fn is_general(section: &Section) -> bool {
    raw_word(section).is_some_and(|word| word.eq_ignore_ascii_case("general"))
}

pub(crate) fn has_text_placeholder(section: &Section) -> bool {
    section.atoms.iter().any(|atom| matches!(atom, Atom::At))
}

fn raw_word(section: &Section) -> Option<String> {
    let mut word = String::new();
    for atom in &section.atoms {
        match atom {
            Atom::Raw(character) if character.is_ascii_alphabetic() => word.push(*character),
            Atom::Literal(value) if value.trim().is_empty() => {}
            Atom::Raw(character) if character.is_ascii_whitespace() => {}
            _ => return None,
        }
    }
    (!word.is_empty()).then_some(word)
}

pub(crate) fn render_general_section(section: &Section, rendered: &str) -> Option<String> {
    let mut output = String::new();
    let mut inserted = false;
    let mut index = 0;
    while index < section.atoms.len() {
        match &section.atoms[index] {
            Atom::Raw(character) if character.eq_ignore_ascii_case(&'g') => {
                let mut word = String::new();
                let start = index;
                while let Some(Atom::Raw(character)) = section.atoms.get(index) {
                    if !character.is_ascii_alphabetic() {
                        break;
                    }
                    word.push(*character);
                    index += 1;
                }
                if word.eq_ignore_ascii_case("general") {
                    output.push_str(rendered);
                    inserted = true;
                } else {
                    for atom in &section.atoms[start..index] {
                        append_literal_atom(&mut output, atom, "")?;
                    }
                }
                continue;
            }
            atom => append_literal_atom(&mut output, atom, "")?,
        }
        index += 1;
    }
    inserted.then_some(output)
}

pub(crate) fn render_text_section(section: &Section, text: &str) -> Option<String> {
    let mut output = String::new();
    for atom in &section.atoms {
        append_literal_atom(&mut output, atom, text)?;
    }
    Some(output)
}

pub(crate) fn append_literal_atom(output: &mut String, atom: &Atom, text: &str) -> Option<()> {
    match atom {
        Atom::Literal(value) => output.push_str(value),
        Atom::Raw(character) => output.push(*character),
        Atom::Percent => output.push('%'),
        Atom::At => output.push_str(text),
        Atom::Placeholder('?') => output.push(' '),
        Atom::Placeholder(_) => {}
        Atom::Elapsed(_, _) => return None,
    }
    Some(())
}

pub(crate) fn section_is_supported(section: &Section) -> bool {
    if section.atoms.is_empty() {
        return true;
    }
    if is_general(section) || super::datetime::is_datetime(section) {
        return true;
    }
    if section.atoms.iter().any(|atom| matches!(atom, Atom::At)) {
        return section.atoms.iter().all(|atom| {
            matches!(
                atom,
                Atom::Literal(_) | Atom::Raw(_) | Atom::Percent | Atom::At
            )
        });
    }
    if section
        .atoms
        .iter()
        .any(|atom| matches!(atom, Atom::Placeholder(_)))
    {
        return super::number::is_supported(section);
    }
    section.atoms.iter().all(|atom| {
        matches!(
            atom,
            Atom::Literal(_) | Atom::Raw(_) | Atom::Percent | Atom::At
        )
    }) && section
        .atoms
        .iter()
        .any(|atom| matches!(atom, Atom::Literal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_only_unquoted_semicolons() {
        let format = parse_format("0;[Red]-0;\"zero;value\";@").unwrap();
        assert_eq!(format.sections.len(), 4);
    }

    #[test]
    fn selects_sign_sections_and_conditions() {
        let sections = parse_format("0;[Red](0);-").unwrap().sections;
        assert_eq!(
            select_numeric_section(&sections, -2.0),
            Some(Selection {
                index: 1,
                absolute: true
            })
        );
        let conditional = parse_format("[<0]0;[=0]-;0").unwrap().sections;
        assert_eq!(
            select_numeric_section(&conditional, 0.0),
            Some(Selection {
                index: 1,
                absolute: false
            })
        );
        assert_eq!(
            select_numeric_section(&conditional, 4.0),
            Some(Selection {
                index: 2,
                absolute: false
            })
        );
    }

    #[test]
    fn parses_color_locale_and_elapsed_tags() {
        let section = &parse_format("[Red][$$-0409]0.00 [hh]").unwrap().sections[0];
        assert!(matches!(&section.atoms[0], Atom::Literal(value) if value == "$"));
        assert!(section
            .atoms
            .iter()
            .any(|atom| matches!(atom, Atom::Elapsed(ElapsedUnit::Hour, 2))));
    }
}
