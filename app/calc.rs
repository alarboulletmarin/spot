//! Small, dependency-free calculator. Parsing is bounded and never executes input.
#[derive(Debug, PartialEq)]
pub struct CalcError;

pub fn eval(text: &str) -> Option<Result<f64, CalcError>> {
    let text = text.trim();
    let calculation = text.contains(['+', '-', '*', '/', '%', '^', '×', '÷'])
        || ["sqrt", "sin", "cos", "tan", "ln", "log", "abs"]
            .iter()
            .any(|f| text.starts_with(f) && text[f.len()..].trim_start().starts_with('('));
    if !calculation {
        return None;
    }
    Some((|| {
        if text.len() > 1024 {
            return Err(CalcError);
        }
        let normalized = text.replace('×', "*").replace('÷', "/").replace(',', ".");
        let mut p = Parser {
            text: normalized.as_bytes(),
            pos: 0,
            depth: 0,
        };
        let value = p.sum()?;
        p.space();
        if p.pos != p.text.len() || !value.is_finite() {
            return Err(CalcError);
        }
        Ok(value)
    })())
}

struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
    depth: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.text.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.space();
        if self.text.get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> Result<f64, CalcError> {
        let mut n = self.product()?;
        loop {
            if self.take(b'+') {
                n += self.product()?;
            } else if self.take(b'-') {
                n -= self.product()?;
            } else {
                return Ok(n);
            }
        }
    }
    fn product(&mut self) -> Result<f64, CalcError> {
        let mut n = self.unary()?;
        loop {
            if self.take(b'*') {
                n *= self.unary()?;
            } else if self.take(b'/') {
                n /= self.unary()?;
            } else if self.take(b'%') {
                n %= self.unary()?;
            } else {
                return Ok(n);
            }
        }
    }
    fn unary(&mut self) -> Result<f64, CalcError> {
        self.depth += 1;
        if self.depth > 64 {
            return Err(CalcError);
        }
        let result = if self.take(b'-') {
            self.unary().map(|n| -n)
        } else if self.take(b'+') {
            self.unary()
        } else {
            self.power()
        };
        self.depth -= 1;
        result
    }
    fn power(&mut self) -> Result<f64, CalcError> {
        let n = self.atom()?;
        if self.take(b'^') {
            Ok(n.powf(self.unary()?))
        } else {
            Ok(n)
        }
    }
    fn atom(&mut self) -> Result<f64, CalcError> {
        if self.take(b'(') {
            let n = self.sum()?;
            return if self.take(b')') {
                Ok(n)
            } else {
                Err(CalcError)
            };
        }
        self.space();
        let start = self.pos;
        if self
            .text
            .get(start..start + 2)
            .is_some_and(|s| s == b"0x" || s == b"0X" || s == b"0b" || s == b"0B")
        {
            let radix = if self.text[start + 1].eq_ignore_ascii_case(&b'x') {
                16
            } else {
                2
            };
            self.pos += 2;
            let digits = self.pos;
            while self
                .text
                .get(self.pos)
                .is_some_and(|c| (*c as char).is_digit(radix))
            {
                self.pos += 1;
            }
            return u64::from_str_radix(
                std::str::from_utf8(&self.text[digits..self.pos]).unwrap(),
                radix,
            )
            .map(|n| n as f64)
            .map_err(|_| CalcError);
        }
        if self.text.get(start).is_some_and(u8::is_ascii_alphabetic) {
            while self.text.get(self.pos).is_some_and(u8::is_ascii_alphabetic) {
                self.pos += 1;
            }
            let name = &self.text[start..self.pos];
            match name {
                b"pi" => return Ok(std::f64::consts::PI),
                b"e" => return Ok(std::f64::consts::E),
                _ => {}
            }
            if !self.take(b'(') {
                return Err(CalcError);
            }
            let n = self.sum()?;
            if !self.take(b')') {
                return Err(CalcError);
            }
            return match name {
                b"sqrt" => Ok(n.sqrt()),
                b"sin" => Ok(n.sin()),
                b"cos" => Ok(n.cos()),
                b"tan" => Ok(n.tan()),
                b"ln" => Ok(n.ln()),
                b"log" => Ok(n.log10()),
                b"abs" => Ok(n.abs()),
                _ => Err(CalcError),
            };
        }
        while self
            .text
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.pos += 1;
        }
        if self
            .text
            .get(self.pos)
            .is_some_and(|c| *c == b'e' || *c == b'E')
        {
            self.pos += 1;
            if self
                .text
                .get(self.pos)
                .is_some_and(|c| *c == b'+' || *c == b'-')
            {
                self.pos += 1;
            }
            while self.text.get(self.pos).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
            }
        }
        std::str::from_utf8(&self.text[start..self.pos])
            .unwrap()
            .parse()
            .map_err(|_| CalcError)
    }
}

pub fn hexadecimal(n: f64) -> Option<String> {
    (n > 0. && n.fract() == 0. && n < u64::MAX as f64).then(|| format!("0x{:X}", n as u64))
}

/// Numeric locale is initialized by GTK; copy libc's separators without changing global locale.
pub fn display(n: f64) -> String {
    use std::ffi::{CStr, c_char};
    #[repr(C)]
    struct Locale {
        decimal: *const c_char,
        thousands: *const c_char,
        grouping: *const c_char,
    }
    unsafe extern "C" {
        fn localeconv() -> *const Locale;
    }
    // SAFETY: localeconv returns a live libc struct; only its standard first three fields are read.
    let (decimal, thousands, grouping) = unsafe {
        let locale = &*localeconv();
        (
            CStr::from_ptr(locale.decimal)
                .to_string_lossy()
                .into_owned(),
            CStr::from_ptr(locale.thousands)
                .to_string_lossy()
                .into_owned(),
            CStr::from_ptr(locale.grouping).to_bytes().to_vec(),
        )
    };
    format_number(&n.to_string(), &decimal, &thousands, &grouping)
}
fn format_number(raw: &str, decimal: &str, separator: &str, groups: &[u8]) -> String {
    if raw.contains(['e', 'E']) {
        return raw.replace('.', decimal);
    }
    let (whole, fraction) = raw
        .split_once('.')
        .map_or((raw, None), |(a, b)| (a, Some(b)));
    let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |s| ("-", s));
    let mut chunks = Vec::new();
    let mut end = digits.len();
    let mut group_index = 0;
    let mut width = groups.first().copied().unwrap_or(0) as usize;
    while width > 0 && width < 127 && end > width {
        chunks.push(&digits[end - width..end]);
        end -= width;
        if let Some(&next) = groups.get(group_index + 1) {
            group_index += 1;
            if next != 0 {
                width = next as usize;
            }
        }
    }
    chunks.push(&digits[..end]);
    chunks.reverse();
    let mut result = format!("{sign}{}", chunks.join(separator));
    if let Some(fraction) = fraction {
        result += decimal;
        result += fraction;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar() {
        for (s, n) in [
            ("2^10 * 3", 3072.),
            ("0xff + 1", 256.),
            ("(1+2)*3", 9.),
            ("10 ÷ 4", 2.5),
            ("2,5 * 2", 5.),
            ("2^3^2", 512.),
            ("-2^2", -4.),
            ("2^-2", 0.25),
            ("abs(-2)+log(100)", 4.),
            ("0b10+1", 3.),
        ] {
            assert_eq!(eval(s), Some(Ok(n)), "{s}");
        }
        assert!((eval("sqrt(2)").unwrap().unwrap() - 2f64.sqrt()).abs() < 1e-12);
        for s in ["42", "e", "firefox"] {
            assert!(eval(s).is_none());
        }
        for s in ["c++", "1/0", "sqrt(-1)", "2+", "0xgg+1", "sin(1/0)"] {
            assert!(eval(s).unwrap().is_err(), "{s}");
        }
        assert!(
            eval(&format!("{}1+2{}", "(".repeat(100), ")".repeat(100)))
                .unwrap()
                .is_err()
        );
        assert_eq!(hexadecimal(3072.).as_deref(), Some("0xC00"));
        assert_eq!(hexadecimal(-1.), None);
    }
    #[test]
    fn locale_formatting() {
        assert_eq!(format_number("3072", ",", "\u{202f}", &[3]), "3\u{202f}072");
        assert_eq!(format_number("-1234567.5", ".", ",", &[3]), "-1,234,567.5");
        assert_eq!(format_number("1234567", ".", ",", &[3, 2]), "12,34,567");
    }
}
