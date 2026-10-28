//! The subset of the C runtime's `sprintf` (`FUN_004cfbd0`) the game's text
//! formats use: `%d`, `%Nd`, `%0Nd`, `%.Nd` (at least N digits, the `0`
//! flag then ignored), `%s`, `%.Ns` (at most N bytes) and `%%`. An unsupported or
//! unmatched conversion is copied through unchanged, so a malformed format
//! never reads past its arguments.

/// One `sprintf` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatArg<'a> {
    Int(i64),
    Bytes(&'a [u8]),
}

/// Format `fmt` with `args` in order.
#[must_use]
pub fn c_format(fmt: &[u8], args: &[FormatArg]) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len() + 16);
    let mut args = args.iter();
    let mut index = 0;
    while index < fmt.len() {
        let byte = fmt[index];
        if byte != b'%' {
            out.push(byte);
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        if fmt.get(index) == Some(&b'%') {
            out.push(b'%');
            index += 1;
            continue;
        }
        let zero = fmt.get(index) == Some(&b'0');
        if zero {
            index += 1;
        }
        let mut width = 0usize;
        while let Some(digit) = fmt.get(index).filter(|byte| byte.is_ascii_digit()) {
            width = width * 10 + usize::from(digit - b'0');
            index += 1;
        }
        let mut precision = None;
        if fmt.get(index) == Some(&b'.') {
            index += 1;
            let mut digits = 0usize;
            while let Some(digit) = fmt.get(index).filter(|byte| byte.is_ascii_digit()) {
                digits = digits * 10 + usize::from(digit - b'0');
                index += 1;
            }
            precision = Some(digits);
        }
        let converted = match (fmt.get(index), args.clone().next()) {
            (Some(b'd'), Some(&FormatArg::Int(value))) => {
                let mut digits = value.unsigned_abs().to_string().into_bytes();
                if let Some(minimum) = precision {
                    if value == 0 && minimum == 0 {
                        digits.clear();
                    }
                    let lead = minimum.saturating_sub(digits.len());
                    digits.splice(0..0, std::iter::repeat_n(b'0', lead));
                }
                let sign = usize::from(value < 0);
                let pad = width.saturating_sub(digits.len() + sign);
                let mut field = Vec::new();
                if zero && precision.is_none() {
                    if value < 0 {
                        field.push(b'-');
                    }
                    field.resize(field.len() + pad, b'0');
                } else {
                    field.resize(pad, b' ');
                    if value < 0 {
                        field.push(b'-');
                    }
                }
                field.extend(digits);
                Some(field)
            }
            (Some(b'c'), Some(&FormatArg::Int(value))) => {
                let mut field = vec![b' '; width.saturating_sub(1)];
                field.push(value as u8);
                Some(field)
            }
            (Some(b's'), Some(&FormatArg::Bytes(text))) => {
                let text = &text[..precision.map_or(text.len(), |n| n.min(text.len()))];
                let mut field = vec![b' '; width.saturating_sub(text.len())];
                field.extend_from_slice(text);
                Some(field)
            }
            _ => None,
        };
        match converted {
            Some(field) => {
                args.next();
                out.extend(field);
                index += 1;
            }
            None => {
                let end = (index + 1).min(fmt.len());
                out.extend_from_slice(&fmt[start..end]);
                index = end;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_formats() {
        assert_eq!(
            c_format(
                b"%02d:%02d.%02d",
                &[FormatArg::Int(1), FormatArg::Int(5), FormatArg::Int(7)]
            ),
            b"01:05.07"
        );
        assert_eq!(c_format(b"%d", &[FormatArg::Int(12)]), b"12");
        assert_eq!(
            c_format(b"AFTER %s", &[FormatArg::Bytes(b"KENYA")]),
            b"AFTER KENYA"
        );
        assert_eq!(c_format(b"100%%", &[]), b"100%");
        assert_eq!(c_format(b"F%.3d.bmp", &[FormatArg::Int(7)]), b"F007.bmp");
        assert_eq!(c_format(b"%05.3d", &[FormatArg::Int(-7)]), b" -007");
        assert_eq!(c_format(b"[%.0d]", &[FormatArg::Int(0)]), b"[]");
        assert_eq!(c_format(b"%.2s", &[FormatArg::Bytes(b"ABC")]), b"AB");
        assert_eq!(
            c_format(b"%3d|%03d", &[FormatArg::Int(-4), FormatArg::Int(-4)]),
            b" -4|-04"
        );
    }

    #[test]
    fn missing_or_mismatched_arguments_copy_through() {
        assert_eq!(c_format(b"stage %d", &[]), b"stage %d");
        assert_eq!(c_format(b"%s", &[FormatArg::Int(1)]), b"%s");
        assert_eq!(c_format(b"%x and %", &[]), b"%x and %");
    }

    #[test]
    fn a_char_takes_the_low_byte() {
        let out = c_format(
            b"CAC%04d%c: %s",
            &[
                FormatArg::Int(42),
                FormatArg::Int(i64::from(b'7')),
                FormatArg::Bytes(b"123"),
            ],
        );
        assert_eq!(out, b"CAC00427: 123");
    }
}
