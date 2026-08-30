#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedLiteral {
    pub(crate) value: LiteralValue,
    pub(crate) requires_toml_1_1: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LiteralValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    DateTime,
}

pub(crate) fn parse(raw: &str) -> Option<ParsedLiteral> {
    if raw.starts_with('"') {
        let (value, requires_toml_1_1) = parse_basic_string(raw)?;
        return Some(ParsedLiteral {
            value: LiteralValue::String(value),
            requires_toml_1_1,
        });
    }
    if raw.starts_with('\'') {
        return Some(ParsedLiteral {
            value: LiteralValue::String(parse_literal_string(raw)?),
            requires_toml_1_1: false,
        });
    }
    if matches!(raw, "true" | "false") {
        return Some(ParsedLiteral {
            value: LiteralValue::Boolean(raw == "true"),
            requires_toml_1_1: false,
        });
    }
    if let Some(value) = parse_integer(raw).map(LiteralValue::Integer) {
        return Some(ParsedLiteral {
            value,
            requires_toml_1_1: false,
        });
    }
    if let Some(value) = parse_float(raw).map(LiteralValue::Float) {
        return Some(ParsedLiteral {
            value,
            requires_toml_1_1: false,
        });
    }
    let requires_toml_1_1 = parse_date_time(raw)?;
    Some(ParsedLiteral {
        value: LiteralValue::DateTime,
        requires_toml_1_1,
    })
}

fn parse_basic_string(raw: &str) -> Option<(String, bool)> {
    let multiline = raw.starts_with("\"\"\"");
    let content = string_content(raw, '"', multiline)?;
    // Fast path: no escapes, quotes, or control characters means the content
    // is the decoded string verbatim.
    if !content
        .bytes()
        .any(|byte| matches!(byte, b'\\' | b'"' | 0x7f) || (byte < 0x20 && byte != b'\t'))
    {
        return Some((content.to_owned(), false));
    }
    let mut output = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut requires_toml_1_1 = false;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => {
                cursor += 1;
                if multiline {
                    if let Some(after_fold) = consume_line_fold(bytes, cursor) {
                        cursor = after_fold;
                        continue;
                    }
                }
                let escape = *bytes.get(cursor)?;
                cursor += 1;
                match escape {
                    b'b' => output.push('\u{0008}'),
                    b't' => output.push('\t'),
                    b'n' => output.push('\n'),
                    b'f' => output.push('\u{000c}'),
                    b'r' => output.push('\r'),
                    b'e' => {
                        output.push('\u{001b}');
                        requires_toml_1_1 = true;
                    }
                    b'"' => output.push('"'),
                    b'\\' => output.push('\\'),
                    b'x' => {
                        output.push(parse_hex_scalar(bytes, &mut cursor, 2)?);
                        requires_toml_1_1 = true;
                    }
                    b'u' => output.push(parse_hex_scalar(bytes, &mut cursor, 4)?),
                    b'U' => output.push(parse_hex_scalar(bytes, &mut cursor, 8)?),
                    _ => return None,
                }
            }
            b'"' if multiline => {
                let run = quote_run(bytes, cursor, b'"');
                if run > 2 {
                    return None;
                }
                output.extend(std::iter::repeat_n('"', run));
                cursor += run;
            }
            b'"' => return None,
            b'\n' if multiline => {
                output.push('\n');
                cursor += 1;
            }
            b'\r' if multiline && bytes.get(cursor + 1) == Some(&b'\n') => {
                output.push('\n');
                cursor += 2;
            }
            _ => push_unescaped(content, &mut cursor, &mut output)?,
        }
    }
    Some((output, requires_toml_1_1))
}

fn parse_literal_string(raw: &str) -> Option<String> {
    let multiline = raw.starts_with("'''");
    let content = string_content(raw, '\'', multiline)?;
    // Fast path: no quotes or control characters means the content is the
    // decoded string verbatim.
    if !content
        .bytes()
        .any(|byte| matches!(byte, b'\'' | 0x7f) || (byte < 0x20 && byte != b'\t'))
    {
        return Some(content.to_owned());
    }
    let bytes = content.as_bytes();
    let mut output = String::with_capacity(content.len());
    let mut cursor = 0;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' if multiline => {
                let run = quote_run(bytes, cursor, b'\'');
                if run > 2 {
                    return None;
                }
                output.extend(std::iter::repeat_n('\'', run));
                cursor += run;
            }
            b'\'' => return None,
            b'\n' if multiline => {
                output.push('\n');
                cursor += 1;
            }
            b'\r' if multiline && bytes.get(cursor + 1) == Some(&b'\n') => {
                output.push('\n');
                cursor += 2;
            }
            _ => push_unescaped(content, &mut cursor, &mut output)?,
        }
    }
    Some(output)
}

fn string_content(raw: &str, quote: char, multiline: bool) -> Option<&str> {
    let delimiter_length = if multiline { 3 } else { 1 };
    let quote = quote as u8;
    let bytes = raw.as_bytes();
    if raw.len() < delimiter_length * 2
        || !bytes[..delimiter_length].iter().all(|&byte| byte == quote)
        || !bytes[raw.len() - delimiter_length..]
            .iter()
            .all(|&byte| byte == quote)
    {
        return None;
    }
    let content = &raw[delimiter_length..raw.len() - delimiter_length];
    if !multiline {
        return Some(content);
    }
    Some(
        content
            .strip_prefix("\r\n")
            .or_else(|| content.strip_prefix('\n'))
            .unwrap_or(content),
    )
}

fn push_unescaped(content: &str, cursor: &mut usize, output: &mut String) -> Option<()> {
    let character = content[*cursor..].chars().next()?;
    if (character <= '\u{001f}' && character != '\t') || character == '\u{007f}' {
        return None;
    }
    output.push(character);
    *cursor += character.len_utf8();
    Some(())
}

fn parse_hex_scalar(bytes: &[u8], cursor: &mut usize, digits: usize) -> Option<char> {
    let end = cursor.checked_add(digits)?;
    let raw = bytes.get(*cursor..end)?;
    if !raw.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let raw = std::str::from_utf8(raw).ok()?;
    let scalar = u32::from_str_radix(raw, 16).ok()?;
    *cursor = end;
    char::from_u32(scalar)
}

fn consume_line_fold(bytes: &[u8], mut cursor: usize) -> Option<usize> {
    while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
        cursor += 1;
    }
    cursor = consume_newline(bytes, cursor)?;
    loop {
        match bytes.get(cursor) {
            Some(b' ' | b'\t' | b'\n') => cursor += 1,
            Some(b'\r') if bytes.get(cursor + 1) == Some(&b'\n') => cursor += 2,
            _ => return Some(cursor),
        }
    }
}

fn consume_newline(bytes: &[u8], cursor: usize) -> Option<usize> {
    match bytes.get(cursor) {
        Some(b'\n') => Some(cursor + 1),
        Some(b'\r') if bytes.get(cursor + 1) == Some(&b'\n') => Some(cursor + 2),
        _ => None,
    }
}

fn quote_run(bytes: &[u8], start: usize, quote: u8) -> usize {
    bytes[start..]
        .iter()
        .take_while(|&&byte| byte == quote)
        .count()
}

fn parse_integer(raw: &str) -> Option<i64> {
    if let Some(digits) = raw.strip_prefix("0x") {
        return parse_radix_integer(digits, 16, |byte| byte.is_ascii_hexdigit());
    }
    if let Some(digits) = raw.strip_prefix("0o") {
        return parse_radix_integer(digits, 8, |byte| matches!(byte, b'0'..=b'7'));
    }
    if let Some(digits) = raw.strip_prefix("0b") {
        return parse_radix_integer(digits, 2, |byte| matches!(byte, b'0' | b'1'));
    }

    let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    if !valid_digit_run(unsigned.as_bytes(), |byte| byte.is_ascii_digit())
        || (unsigned.len() > 1 && unsigned.starts_with('0'))
    {
        return None;
    }

    if raw.contains('_') {
        raw.replace('_', "").parse::<i64>().ok()
    } else {
        raw.parse::<i64>().ok()
    }
}

fn parse_radix_integer(digits: &str, radix: u32, is_digit: impl Fn(u8) -> bool) -> Option<i64> {
    if !valid_digit_run(digits.as_bytes(), is_digit) {
        return None;
    }
    if digits.contains('_') {
        i64::from_str_radix(&digits.replace('_', ""), radix).ok()
    } else {
        i64::from_str_radix(digits, radix).ok()
    }
}

fn parse_float(raw: &str) -> Option<f64> {
    if matches!(raw, "inf" | "+inf") {
        return Some(f64::INFINITY);
    }
    if raw == "-inf" {
        return Some(f64::NEG_INFINITY);
    }
    if matches!(raw, "nan" | "+nan" | "-nan") {
        return Some(f64::NAN);
    }

    let bytes = raw.as_bytes();
    let mut cursor = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_start = cursor;
    let integer_digits = consume_digit_run(bytes, &mut cursor, |byte| byte.is_ascii_digit())?;
    if integer_digits > 1 && bytes[integer_start] == b'0' {
        return None;
    }

    let mut has_fraction = false;
    if bytes.get(cursor) == Some(&b'.') {
        has_fraction = true;
        cursor += 1;
        consume_digit_run(bytes, &mut cursor, |byte| byte.is_ascii_digit())?;
    }

    let mut has_exponent = false;
    if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        has_exponent = true;
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        consume_digit_run(bytes, &mut cursor, |byte| byte.is_ascii_digit())?;
    }

    if cursor != bytes.len() || (!has_fraction && !has_exponent) {
        return None;
    }
    if raw.contains('_') {
        raw.replace('_', "").parse::<f64>().ok()
    } else {
        raw.parse::<f64>().ok()
    }
}

fn valid_digit_run(bytes: &[u8], is_digit: impl Fn(u8) -> bool) -> bool {
    let mut cursor = 0;
    consume_digit_run(bytes, &mut cursor, is_digit).is_some() && cursor == bytes.len()
}

fn consume_digit_run(
    bytes: &[u8],
    cursor: &mut usize,
    is_digit: impl Fn(u8) -> bool,
) -> Option<usize> {
    let first = *bytes.get(*cursor)?;
    if !is_digit(first) {
        return None;
    }
    *cursor += 1;
    let mut digits = 1;

    while let Some(&byte) = bytes.get(*cursor) {
        if is_digit(byte) {
            *cursor += 1;
            digits += 1;
        } else if byte == b'_' && bytes.get(*cursor + 1).is_some_and(|next| is_digit(*next)) {
            *cursor += 2;
            digits += 1;
        } else {
            break;
        }
    }
    Some(digits)
}

fn parse_date_time(raw: &str) -> Option<bool> {
    let bytes = raw.as_bytes();
    if bytes.len() >= 10 && bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-') {
        let date = parse_date(&bytes[..10])?;
        if bytes.len() == 10 {
            return Some(false);
        }
        if !matches!(bytes.get(10), Some(b'T' | b't' | b' ')) {
            return None;
        }
        let time = parse_partial_time(bytes, 11)?;
        let mut cursor = time.cursor;
        if cursor == bytes.len() {
            return Some(time.requires_toml_1_1);
        }
        match bytes[cursor] {
            b'Z' | b'z'
                if cursor + 1 == bytes.len() && offset_datetime_second_is_valid(date, time, 0) =>
            {
                Some(time.requires_toml_1_1)
            }
            b'+' | b'-' => {
                let sign = if bytes[cursor] == b'+' { 1_i32 } else { -1_i32 };
                cursor += 1;
                let offset_hour = parse_time_component(bytes, &mut cursor, 23)?;
                if bytes.get(cursor) != Some(&b':') {
                    return None;
                }
                cursor += 1;
                let offset_minute = parse_time_component(bytes, &mut cursor, 59)?;
                let offset = sign
                    * i32::try_from(offset_hour * 60 + offset_minute)
                        .expect("validated offset components fit in i32");
                (cursor == bytes.len() && offset_datetime_second_is_valid(date, time, offset))
                    .then_some(time.requires_toml_1_1)
            }
            _ => None,
        }
    } else {
        let time = parse_partial_time(bytes, 0)?;
        (time.cursor == bytes.len()).then_some(time.requires_toml_1_1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Date {
    year: u32,
    month: u32,
    day: u32,
}

fn parse_date(bytes: &[u8]) -> Option<Date> {
    if bytes.len() != 10
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || bytes[7] != b'-'
    {
        return None;
    }
    let year = parse_two_pairs(&bytes[..4])?;
    let month = parse_two_digits(&bytes[5..7])?;
    let day = parse_two_digits(&bytes[8..10])?;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    Some(Date { year, month, day })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PartialTime {
    cursor: usize,
    requires_toml_1_1: bool,
    hour: u32,
    minute: u32,
    second: u32,
}

fn parse_partial_time(bytes: &[u8], mut cursor: usize) -> Option<PartialTime> {
    let hour = parse_time_component(bytes, &mut cursor, 23)?;
    if bytes.get(cursor) != Some(&b':') {
        return None;
    }
    cursor += 1;
    let minute = parse_time_component(bytes, &mut cursor, 59)?;

    if bytes.get(cursor) != Some(&b':') {
        return Some(PartialTime {
            cursor,
            requires_toml_1_1: true,
            hour,
            minute,
            second: 0,
        });
    }
    cursor += 1;
    let second = parse_time_component(bytes, &mut cursor, 60)?;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return None;
        }
    }
    Some(PartialTime {
        cursor,
        requires_toml_1_1: false,
        hour,
        minute,
        second,
    })
}

fn offset_datetime_second_is_valid(date: Date, time: PartialTime, offset_minutes: i32) -> bool {
    if time.second != 60 {
        return true;
    }

    let utc_minute = i32::try_from(time.hour * 60 + time.minute)
        .expect("validated time components fit in i32")
        - offset_minutes;
    let (utc_date, utc_minute) = if utc_minute < 0 {
        let Some(previous) = previous_day(date) else {
            return false;
        };
        (previous, utc_minute + 24 * 60)
    } else if utc_minute >= 24 * 60 {
        let Some(next) = next_day(date) else {
            return false;
        };
        (next, utc_minute - 24 * 60)
    } else {
        (date, utc_minute)
    };

    utc_minute == 23 * 60 + 59
        && POSITIVE_LEAP_SECOND_DATES.contains(&(utc_date.year, utc_date.month, utc_date.day))
}

fn previous_day(date: Date) -> Option<Date> {
    if date.day > 1 {
        return Some(Date {
            day: date.day - 1,
            ..date
        });
    }
    if date.month > 1 {
        let month = date.month - 1;
        return Some(Date {
            year: date.year,
            month,
            day: days_in_month(date.year, month),
        });
    }
    let year = date.year.checked_sub(1)?;
    Some(Date {
        year,
        month: 12,
        day: 31,
    })
}

fn next_day(date: Date) -> Option<Date> {
    if date.day < days_in_month(date.year, date.month) {
        return Some(Date {
            day: date.day + 1,
            ..date
        });
    }
    if date.month < 12 {
        return Some(Date {
            year: date.year,
            month: date.month + 1,
            day: 1,
        });
    }
    let year = date.year.checked_add(1).filter(|year| *year <= 9_999)?;
    Some(Date {
        year,
        month: 1,
        day: 1,
    })
}

// Positive UTC leap-second event dates derived from the IERS Bulletin C
// history updated through Bulletin 72 (2026-07). Parsing is deterministic and
// does not depend on network or wall-clock state.
// https://hpiers.obspm.fr/iers/bul/bulc/Leap_Second_History.dat
const POSITIVE_LEAP_SECOND_DATES: &[(u32, u32, u32)] = &[
    (1972, 6, 30),
    (1972, 12, 31),
    (1973, 12, 31),
    (1974, 12, 31),
    (1975, 12, 31),
    (1976, 12, 31),
    (1977, 12, 31),
    (1978, 12, 31),
    (1979, 12, 31),
    (1981, 6, 30),
    (1982, 6, 30),
    (1983, 6, 30),
    (1985, 6, 30),
    (1987, 12, 31),
    (1989, 12, 31),
    (1990, 12, 31),
    (1992, 6, 30),
    (1993, 6, 30),
    (1994, 6, 30),
    (1995, 12, 31),
    (1997, 6, 30),
    (1998, 12, 31),
    (2005, 12, 31),
    (2008, 12, 31),
    (2012, 6, 30),
    (2015, 6, 30),
    (2016, 12, 31),
];

fn parse_time_component(bytes: &[u8], cursor: &mut usize, maximum: u32) -> Option<u32> {
    let end = cursor.checked_add(2)?;
    let value = parse_two_digits(bytes.get(*cursor..end)?)?;
    if value > maximum {
        return None;
    }
    *cursor = end;
    Some(value)
}

fn parse_two_digits(bytes: &[u8]) -> Option<u32> {
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0'))
}

fn parse_two_pairs(bytes: &[u8]) -> Option<u32> {
    let high = parse_two_digits(bytes.get(..2)?)?;
    let low = parse_two_digits(bytes.get(2..4)?)?;
    Some(high * 100 + low)
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

const fn is_leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
