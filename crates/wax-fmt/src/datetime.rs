use crate::parser::{Atom, ElapsedUnit, Section};

const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAY_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const DAY_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

#[derive(Clone, Debug)]
enum Part {
    Literal(String),
    Year(usize),
    MonthOrMinute(usize),
    Month(usize),
    Minute(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    FractionalSecond(usize),
    AmPm { short: bool, lowercase: bool },
    Elapsed(ElapsedUnit, usize),
}

pub(crate) fn is_datetime(section: &Section) -> bool {
    parse_parts(section).is_some()
}

pub(crate) fn render(section: &Section, serial: f64, epoch_1904: bool) -> Option<String> {
    if !serial.is_finite() || serial < 0.0 {
        return None;
    }
    let parts = resolve_minutes(parse_parts(section)?);
    let precision = parts
        .iter()
        .filter_map(|part| match part {
            Part::FractionalSecond(width) => Some(*width),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    if precision > 9 {
        return None;
    }
    let has_seconds = parts.iter().any(|part| {
        matches!(
            part,
            Part::Second(_) | Part::Elapsed(ElapsedUnit::Second, _)
        )
    });
    let has_minutes = parts.iter().any(|part| {
        matches!(
            part,
            Part::Minute(_) | Part::Elapsed(ElapsedUnit::Minute, _)
        )
    });
    let has_hours = parts.iter().any(|part| {
        matches!(
            part,
            Part::Hour(_) | Part::Elapsed(ElapsedUnit::Hour, _) | Part::AmPm { .. }
        )
    });
    let has_time = precision > 0 || has_seconds || has_minutes || has_hours;

    let scale = 10_i128.pow(precision as u32);
    let ticks_per_second = scale;
    let ticks_per_minute = 60 * ticks_per_second;
    let ticks_per_hour = 60 * ticks_per_minute;
    let ticks_per_day = 24 * ticks_per_hour;
    let raw_ticks = serial * ticks_per_day as f64;
    if !raw_ticks.is_finite() || raw_ticks > i128::MAX as f64 {
        return None;
    }
    let quantum = if precision > 0 || has_seconds {
        ticks_per_second / scale.max(1)
    } else if has_minutes {
        ticks_per_minute
    } else if has_hours {
        ticks_per_hour
    } else {
        ticks_per_day
    };
    let total_ticks = if has_time {
        ((raw_ticks / quantum as f64).round() as i128) * quantum
    } else {
        (serial.floor() as i128) * ticks_per_day
    };
    let serial_day = total_ticks.div_euclid(ticks_per_day);
    let day_ticks = total_ticks.rem_euclid(ticks_per_day);
    let date = excel_date(i64::try_from(serial_day).ok()?, epoch_1904)?;
    let hour = (day_ticks / ticks_per_hour) as u32;
    let minute = ((day_ticks % ticks_per_hour) / ticks_per_minute) as u32;
    let second = ((day_ticks % ticks_per_minute) / ticks_per_second) as u32;
    let fractional_ticks = day_ticks % ticks_per_second;
    let use_twelve_hour = parts.iter().any(|part| matches!(part, Part::AmPm { .. }));

    let mut output = String::new();
    for part in parts {
        match part {
            Part::Literal(value) => output.push_str(&value),
            Part::Year(width) => match width {
                1 => output.push_str(&date.year.to_string()),
                2 => output.push_str(&format!("{:02}", date.year.rem_euclid(100))),
                _ => output.push_str(&format!("{:0width$}", date.year, width = width)),
            },
            Part::Month(width) => render_month(&mut output, date.month, width),
            Part::Minute(width) => push_padded(&mut output, minute as i128, width),
            Part::MonthOrMinute(_) => return None,
            Part::Day(width) => render_day(&mut output, &date, width),
            Part::Hour(width) => {
                let displayed = if use_twelve_hour {
                    let hour = hour % 12;
                    if hour == 0 {
                        12
                    } else {
                        hour
                    }
                } else {
                    hour
                };
                push_padded(&mut output, displayed as i128, width);
            }
            Part::Second(width) => push_padded(&mut output, second as i128, width),
            Part::FractionalSecond(width) => {
                output.push_str(&format!("{:0width$}", fractional_ticks, width = width));
            }
            Part::AmPm { short, lowercase } => {
                let value = match (hour < 12, short) {
                    (true, true) => "A",
                    (false, true) => "P",
                    (true, false) => "AM",
                    (false, false) => "PM",
                };
                if lowercase {
                    output.push_str(&value.to_ascii_lowercase());
                } else {
                    output.push_str(value);
                }
            }
            Part::Elapsed(unit, width) => {
                let divisor = match unit {
                    ElapsedUnit::Hour => ticks_per_hour,
                    ElapsedUnit::Minute => ticks_per_minute,
                    ElapsedUnit::Second => ticks_per_second,
                };
                push_padded(&mut output, total_ticks / divisor, width);
            }
        }
    }
    Some(output)
}

fn parse_parts(section: &Section) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut index = 0;
    let mut has_date_field = false;

    while index < section.atoms.len() {
        match &section.atoms[index] {
            Atom::Literal(value) => {
                push_literal(&mut parts, value.clone());
                index += 1;
            }
            Atom::Elapsed(unit, width) => {
                parts.push(Part::Elapsed(*unit, *width));
                has_date_field = true;
                index += 1;
            }
            Atom::Raw(_) => {
                if let Some((part, consumed)) = parse_am_pm(&section.atoms[index..]) {
                    parts.push(part);
                    has_date_field = true;
                    index += consumed;
                    continue;
                }

                let Atom::Raw(character) = section.atoms[index] else {
                    unreachable!()
                };
                if matches!(character.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's') {
                    let lowercase = character.to_ascii_lowercase();
                    let start = index;
                    while matches!(
                        section.atoms.get(index),
                        Some(Atom::Raw(next)) if next.to_ascii_lowercase() == lowercase
                    ) {
                        index += 1;
                    }
                    let width = index - start;
                    let part = match lowercase {
                        'y' => Part::Year(width),
                        'm' => Part::MonthOrMinute(width),
                        'd' => Part::Day(width),
                        'h' => Part::Hour(width),
                        's' => Part::Second(width),
                        _ => unreachable!(),
                    };
                    parts.push(part);
                    has_date_field = true;

                    if lowercase == 's' && matches!(section.atoms.get(index), Some(Atom::Raw('.')))
                    {
                        let decimal_index = index;
                        index += 1;
                        let fractional_start = index;
                        while matches!(section.atoms.get(index), Some(Atom::Placeholder('0'))) {
                            index += 1;
                        }
                        if index > fractional_start {
                            parts.push(Part::Literal(".".to_owned()));
                            parts.push(Part::FractionalSecond(index - fractional_start));
                        } else {
                            index = decimal_index;
                        }
                    }
                } else {
                    push_literal(&mut parts, character.to_string());
                    index += 1;
                }
            }
            Atom::Placeholder(_) | Atom::Percent | Atom::At => return None,
        }
    }

    has_date_field.then_some(parts)
}

fn parse_am_pm(atoms: &[Atom]) -> Option<(Part, usize)> {
    for (pattern, short) in [("AM/PM", false), ("A/P", true)] {
        if atoms.len() < pattern.len() {
            continue;
        }
        let mut original = String::new();
        let matches = atoms.iter().take(pattern.len()).zip(pattern.chars()).all(
            |(atom, expected)| match atom {
                Atom::Raw(character) if character.eq_ignore_ascii_case(&expected) => {
                    original.push(*character);
                    true
                }
                _ => false,
            },
        );
        if matches {
            let lowercase = original
                .chars()
                .filter(|character| character.is_ascii_alphabetic())
                .all(|character| character.is_ascii_lowercase());
            return Some((Part::AmPm { short, lowercase }, pattern.len()));
        }
    }
    None
}

fn resolve_minutes(mut parts: Vec<Part>) -> Vec<Part> {
    for index in 0..parts.len() {
        let Part::MonthOrMinute(width) = parts[index] else {
            continue;
        };
        let previous = parts[..index]
            .iter()
            .rev()
            .find(|part| !matches!(part, Part::Literal(_)));
        let next = parts[index + 1..]
            .iter()
            .find(|part| !matches!(part, Part::Literal(_)));
        let is_minute = matches!(
            previous,
            Some(Part::Hour(_) | Part::Elapsed(ElapsedUnit::Hour, _))
        ) || matches!(
            next,
            Some(
                Part::Second(_) | Part::FractionalSecond(_) | Part::Elapsed(ElapsedUnit::Second, _)
            )
        );
        parts[index] = if is_minute {
            Part::Minute(width)
        } else {
            Part::Month(width)
        };
    }
    parts
}

fn push_literal(parts: &mut Vec<Part>, value: String) {
    if let Some(Part::Literal(existing)) = parts.last_mut() {
        existing.push_str(&value);
    } else {
        parts.push(Part::Literal(value));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExcelDate {
    year: i32,
    month: u32,
    day: u32,
    weekday: usize,
}

fn excel_date(serial_day: i64, epoch_1904: bool) -> Option<ExcelDate> {
    if serial_day < 0 {
        return None;
    }
    if !epoch_1904 && serial_day == 0 {
        return Some(ExcelDate {
            year: 1900,
            month: 1,
            day: 0,
            weekday: 6,
        });
    }
    if !epoch_1904 && serial_day == 60 {
        return Some(ExcelDate {
            year: 1900,
            month: 2,
            day: 29,
            weekday: 3,
        });
    }

    let unix_day = if epoch_1904 {
        serial_day.checked_sub(24_107)?
    } else if serial_day < 60 {
        serial_day.checked_sub(25_568)?
    } else {
        serial_day.checked_sub(25_569)?
    };
    let (year, month, day) = civil_from_days(unix_day);
    if !(0..=9999).contains(&year) {
        return None;
    }
    let weekday = if epoch_1904 {
        (serial_day + 5).rem_euclid(7) as usize
    } else {
        (serial_day + 6).rem_euclid(7) as usize
    };
    Some(ExcelDate {
        year,
        month,
        day,
        weekday,
    })
}

// Howard Hinnant's civil-from-days algorithm, with day zero at 1970-01-01.
fn civil_from_days(day: i64) -> (i32, u32, u32) {
    let day = day + 719_468;
    let era = if day >= 0 { day } else { day - 146_096 } / 146_097;
    let day_of_era = day - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

fn render_month(output: &mut String, month: u32, width: usize) {
    match width {
        1 => output.push_str(&month.to_string()),
        2 => output.push_str(&format!("{month:02}")),
        3 => output.push_str(MONTH_SHORT[(month - 1) as usize]),
        4 => output.push_str(MONTH_LONG[(month - 1) as usize]),
        _ => output.push(
            MONTH_LONG[(month - 1) as usize]
                .chars()
                .next()
                .unwrap_or_default(),
        ),
    }
}

fn render_day(output: &mut String, date: &ExcelDate, width: usize) {
    match width {
        1 => output.push_str(&date.day.to_string()),
        2 => output.push_str(&format!("{:02}", date.day)),
        3 => output.push_str(DAY_SHORT[date.weekday]),
        _ => output.push_str(DAY_LONG[date.weekday]),
    }
}

fn push_padded(output: &mut String, value: i128, width: usize) {
    if width <= 1 {
        output.push_str(&value.to_string());
    } else {
        output.push_str(&format!("{value:0width$}"));
    }
}

#[cfg(test)]
mod tests {
    use crate::{render, FmtValue};

    fn date(code: &str, serial: f64, epoch_1904: bool) -> String {
        render(code, FmtValue::Number(serial), epoch_1904).unwrap()
    }

    #[test]
    fn preserves_excel_1900_leap_bug() {
        assert_eq!(date("yyyy-mm-dd ddd", 59.0, false), "1900-02-28 Tue");
        assert_eq!(date("yyyy-mm-dd ddd", 60.0, false), "1900-02-29 Wed");
        assert_eq!(date("yyyy-mm-dd ddd", 61.0, false), "1900-03-01 Thu");
    }

    #[test]
    fn uses_the_1904_epoch() {
        assert_eq!(date("yyyy-mm-dd dddd", 0.0, true), "1904-01-01 Friday");
        assert_eq!(date("m/d/yy", 1462.0, true), "1/2/08");
    }

    #[test]
    fn resolves_months_and_minutes_contextually() {
        assert_eq!(
            date("mmm d, yyyy h:mm:ss", 45_205.543_219_9, false),
            "Oct 6, 2023 13:02:14"
        );
        assert_eq!(date("hhmm", 0.543_219_9, false), "1302");
        assert_eq!(date("mmss.000", 0.543_219_9, false), "0214.199");
    }

    #[test]
    fn renders_twelve_hour_and_elapsed_time() {
        assert_eq!(date("h:mm AM/PM", 0.75, false), "6:00 PM");
        assert_eq!(date("[h]:mm:ss", 1.5, false), "36:00:00");
        assert_eq!(date("[mm]", 1.5, false), "2160");
    }

    #[test]
    fn rounds_time_without_changing_date_only_formats() {
        assert_eq!(date("m/d/yy", 45_205.999_999, false), "10/6/23");
        assert_eq!(
            date("m/d/yy h:mm:ss", 45_205.999_999, false),
            "10/7/23 0:00:00"
        );
    }
}
