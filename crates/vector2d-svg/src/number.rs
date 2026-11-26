//! Hand-written scanner for the SVG number grammar (#201).
//!
//! Shared by [`crate::path_data`] (the `d` attribute) and
//! [`crate::transform`]/the `points`/geometry attributes: SVG numbers may be
//! packed with no separator at all (`"1.5.5"` is the two numbers `1.5` and
//! `0.5`; `"10-5"` is `10` and `-5`), so a plain `split_whitespace` or
//! `split(',')` is not enough — this is exactly the "fiddly, well-solved
//! problem" the issue calls out for hand-writing rather than depending on
//! `svgtypes`.

/// A cursor over an attribute value that understands the SVG number grammar:
/// `sign? (digit+ ('.' digit*)? | '.' digit+) (('e'|'E') sign? digit+)?`.
pub struct NumberScanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> NumberScanner<'a> {
    pub fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    pub fn is_at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Peek the next raw byte without consuming it, or `None` at the end.
    /// Used by [`crate::path_data`] to spot a command letter vs. an implicit
    /// repeat's bare number.
    pub(crate) fn peek_byte(&self) -> Option<u8> {
        self.peek()
    }

    /// Consume exactly one byte (a command letter) unconditionally.
    pub(crate) fn bump(&mut self) {
        self.pos += 1;
    }

    /// `skip_separators` then [`Self::next_number`] — the common case for
    /// most path-data fields.
    pub(crate) fn read_number(&mut self) -> Option<f32> {
        self.skip_separators();
        self.next_number()
    }

    /// Read one arc flag with its (optional) separators on both sides.
    pub(crate) fn read_flag(&mut self) -> Option<bool> {
        self.skip_flag_separator();
        let f = self.next_flag();
        self.skip_number_separator();
        f
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Skip whitespace and comma separators (SVG treats `wsp* comma? wsp*`
    /// as one separator between list items).
    pub fn skip_separators(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r' | b',') = self.peek() {
            self.pos += 1;
        }
    }

    /// Skip whitespace only (no comma) — used between an arc flag and the
    /// number that follows it, where a comma is optional but a flag is a
    /// single digit with no separator required at all.
    fn skip_whitespace_only(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Parse one SVG number. Does not skip leading separators — callers
    /// call [`Self::skip_separators`] first so an empty remainder is
    /// distinguishable from a malformed number.
    pub fn next_number(&mut self) -> Option<f32> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut saw_digit = false;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
            saw_digit = true;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                saw_digit = true;
            }
        }
        if !saw_digit {
            self.pos = start;
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exp_start = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let mut saw_exp_digit = false;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                saw_exp_digit = true;
            }
            if !saw_exp_digit {
                // Not actually an exponent (e.g. a trailing flag letter) —
                // back off to just before 'e'/'E' and let the number end there.
                self.pos = exp_start;
            }
        }
        // SAFETY net: the slice we just scanned is pure ASCII (digits, sign,
        // '.', 'e'/'E'), so `from_utf8`/`parse` cannot fail here.
        let value = std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()?
            .parse::<f32>()
            .ok()?;
        // The SVG number grammar has no `inf`/`nan` spelling, and an
        // in-grammar literal can still overflow `f32` (e.g. `1e39`). Either
        // way this is not a number the subset accepts — reject it exactly
        // like a malformed token, resetting the cursor so a caller that
        // backtracks sees the same "nothing consumed" state.
        if !value.is_finite() {
            self.pos = start;
            return None;
        }
        Some(value)
    }

    /// Parse a single `0`/`1` flag digit (SVG arc-command flags), which may
    /// be packed with no separator against the number that follows it
    /// (`"10010"` after an arc's radii is `1 0 0 1 0`, flags then end coords).
    pub fn next_flag(&mut self) -> Option<bool> {
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                Some(false)
            }
            Some(b'1') => {
                self.pos += 1;
                Some(true)
            }
            _ => None,
        }
    }

    /// Separator before a flag: whitespace/comma are both optional (a flag
    /// is a single required digit), so this only consumes what is there.
    pub fn skip_flag_separator(&mut self) {
        self.skip_separators();
    }

    /// Separator before a number that may immediately follow a flag with no
    /// whitespace at all (e.g. `...1100 10 10...`): consume at most the
    /// optional comma/whitespace, same grammar as [`Self::skip_separators`].
    pub fn skip_number_separator(&mut self) {
        self.skip_whitespace_only();
        if self.peek() == Some(b',') {
            self.pos += 1;
            self.skip_whitespace_only();
        }
    }
}

/// Parse a whitespace/comma-separated list of numbers (used by `points`,
/// `viewBox`, and the argument list inside one `transform` function). Returns
/// `None` if any token in the list fails to parse as a number or trailing
/// garbage remains.
pub fn parse_number_list(s: &str) -> Option<Vec<f32>> {
    let mut scanner = NumberScanner::new(s);
    let mut out = Vec::new();
    loop {
        scanner.skip_separators();
        if scanner.is_at_end() {
            break;
        }
        out.push(scanner.next_number()?);
    }
    Some(out)
}

/// Parse a value that is exactly one SVG number and nothing else (used for
/// every plain-number attribute: geometry coordinates, `opacity`, root
/// `width`/`height`). Unlike [`parse_number_list`] this rejects trailing
/// content after the number, and — via [`NumberScanner::next_number`] —
/// `inf`/`nan` spellings and overflow, which bare `str::parse::<f32>()`
/// would silently accept.
pub fn parse_single_number(s: &str) -> Option<f32> {
    let mut scanner = NumberScanner::new(s.trim());
    let v = scanner.next_number()?;
    scanner.is_at_end().then_some(v)
}

/// Parse a single number-or-percentage value (used by gradient coordinates
/// and stop `offset`), returning the percentage as a `0.0..=1.0` fraction.
pub fn parse_number_or_percent(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return parse_single_number(pct).map(|v| v / 100.0);
    }
    parse_single_number(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_decimals_split_correctly() {
        // "1.5.5" is the two numbers 1.5 and 0.5, not 1.5 and .5-as-garbage.
        assert_eq!(parse_number_list("1.5.5"), Some(vec![1.5, 0.5]));
    }

    #[test]
    fn packed_sign_splits_correctly() {
        assert_eq!(parse_number_list("10-5"), Some(vec![10.0, -5.0]));
    }

    #[test]
    fn exponent_is_parsed() {
        assert_eq!(parse_number_list("1e2 -1.5e-1"), Some(vec![100.0, -0.15]));
    }

    #[test]
    fn comma_and_whitespace_both_separate() {
        assert_eq!(
            parse_number_list("1, 2  3,4"),
            Some(vec![1.0, 2.0, 3.0, 4.0])
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(parse_number_list("1 2 foo"), None);
    }

    #[test]
    fn non_finite_spellings_are_rejected() {
        assert_eq!(parse_single_number("inf"), None);
        assert_eq!(parse_single_number("-inf"), None);
        assert_eq!(parse_single_number("nan"), None);
        assert_eq!(parse_single_number("NaN"), None);
    }

    #[test]
    fn in_grammar_overflow_is_rejected() {
        // `1e39` is valid SVG-number syntax but overflows `f32` to infinity.
        assert_eq!(parse_single_number("1e39"), None);
        assert_eq!(parse_number_list("0,0 1e39,0"), None);
    }

    #[test]
    fn single_number_rejects_trailing_garbage() {
        assert_eq!(parse_single_number("10px"), None);
        assert_eq!(parse_single_number("10"), Some(10.0));
    }

    #[test]
    fn percent_is_a_fraction() {
        assert_eq!(parse_number_or_percent("50%"), Some(0.5));
        assert_eq!(parse_number_or_percent("1"), Some(1.0));
    }

    #[test]
    fn flags_split_from_following_number_with_no_separator() {
        let mut sc = NumberScanner::new("10010 10");
        assert_eq!(sc.next_flag(), Some(true));
        assert_eq!(sc.next_flag(), Some(false));
        assert_eq!(sc.next_flag(), Some(false));
        sc.skip_flag_separator();
        assert_eq!(sc.next_number(), Some(10.0));
        sc.skip_number_separator();
        assert_eq!(sc.next_number(), Some(10.0));
    }
}
