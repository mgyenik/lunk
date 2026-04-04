//! Hand-written PDF object parser.
//!
//! Operates on `&[u8]` with a cursor position. Each parse function returns
//! `Result<(T, usize)>` where the `usize` is the new cursor position.

use std::collections::BTreeMap;

use super::PdfError;

/// A PDF value. Covers all object types from the spec.
#[derive(Debug, Clone)]
pub(crate) enum PdfVal {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
    Str(Vec<u8>),
    Array(Vec<PdfVal>),
    Dict(BTreeMap<Vec<u8>, PdfVal>),
    Stream {
        dict: BTreeMap<Vec<u8>, PdfVal>,
        data: Vec<u8>,
    },
    Ref(u32, u16),
}

impl PdfVal {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PdfVal::Int(n) => Some(*n),
            PdfVal::Real(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PdfVal::Real(f) => Some(*f),
            PdfVal::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            PdfVal::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_str_bytes(&self) -> Option<&[u8]> {
        match self {
            PdfVal::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<(u32, u16)> {
        match self {
            PdfVal::Ref(n, g) => Some((*n, *g)),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[PdfVal]> {
        match self {
            PdfVal::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&BTreeMap<Vec<u8>, PdfVal>> {
        match self {
            PdfVal::Dict(d) => Some(d),
            PdfVal::Stream { dict, .. } => Some(dict),
            _ => None,
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn as_stream(&self) -> Option<(&BTreeMap<Vec<u8>, PdfVal>, &[u8])> {
        match self {
            PdfVal::Stream { dict, data } => Some((dict, data)),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, PdfVal::Null)
    }

    /// Get a value from a dict by key.
    pub fn dict_get(&self, key: &[u8]) -> Option<&PdfVal> {
        dict_get(self.as_dict()?, key)
    }
}

/// Look up a key in a PDF dictionary.
pub(crate) fn dict_get<'a>(dict: &'a BTreeMap<Vec<u8>, PdfVal>, key: &[u8]) -> Option<&'a PdfVal> {
    dict.get(&key.to_vec())
}

// -- Tokenizer helpers --

fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0x00)
}

fn is_delimiter(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

/// Skip whitespace and comments.
pub(crate) fn skip_whitespace(buf: &[u8], mut pos: usize) -> usize {
    loop {
        if pos >= buf.len() {
            return pos;
        }
        if is_whitespace(buf[pos]) {
            pos += 1;
        } else if buf[pos] == b'%' {
            // Comment — skip to end of line
            while pos < buf.len() && buf[pos] != b'\n' && buf[pos] != b'\r' {
                pos += 1;
            }
        } else {
            return pos;
        }
    }
}

/// Parse a single PDF value (not an indirect object definition).
/// This handles: null, bool, int, real, name, string, hex string, array, dict, ref.
pub(crate) fn parse_value(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    let pos = skip_whitespace(buf, pos);
    if pos >= buf.len() {
        return Err(PdfError::Parse("unexpected end of input".into()));
    }

    match buf[pos] {
        b'/' => parse_name(buf, pos),
        b'(' => parse_literal_string(buf, pos),
        b'<' => {
            if pos + 1 < buf.len() && buf[pos + 1] == b'<' {
                parse_dict_or_stream(buf, pos)
            } else {
                parse_hex_string(buf, pos)
            }
        }
        b'[' => parse_array(buf, pos),
        b'+' | b'-' | b'.' | b'0'..=b'9' => parse_number_or_ref(buf, pos),
        b't' | b'f' => parse_bool_or_keyword(buf, pos),
        b'n' => parse_null_or_keyword(buf, pos),
        _ => Err(PdfError::Parse(format!(
            "unexpected byte 0x{:02x} at offset {}",
            buf[pos], pos
        ))),
    }
}

/// Parse a name object: /SomeName
fn parse_name(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    debug_assert_eq!(buf[pos], b'/');
    let mut i = pos + 1;
    let mut name = Vec::new();

    while i < buf.len() && is_regular(buf[i]) {
        if buf[i] == b'#' && i + 2 < buf.len() {
            // #XX hex escape
            if let (Some(hi), Some(lo)) = (hex_digit(buf[i + 1]), hex_digit(buf[i + 2])) {
                name.push(hi << 4 | lo);
                i += 3;
            } else {
                name.push(buf[i]);
                i += 1;
            }
        } else {
            name.push(buf[i]);
            i += 1;
        }
    }

    Ok((PdfVal::Name(name), i))
}

/// Parse a literal string: (text with \escapes and (nesting))
fn parse_literal_string(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    debug_assert_eq!(buf[pos], b'(');
    let mut i = pos + 1;
    let mut result = Vec::new();
    let mut depth = 1u32;

    while i < buf.len() && depth > 0 {
        match buf[i] {
            b'(' => {
                depth += 1;
                result.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth > 0 {
                    result.push(b')');
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= buf.len() {
                    break;
                }
                match buf[i] {
                    b'n' => {
                        result.push(b'\n');
                        i += 1;
                    }
                    b'r' => {
                        result.push(b'\r');
                        i += 1;
                    }
                    b't' => {
                        result.push(b'\t');
                        i += 1;
                    }
                    b'b' => {
                        result.push(0x08);
                        i += 1;
                    }
                    b'f' => {
                        result.push(0x0C);
                        i += 1;
                    }
                    b'(' => {
                        result.push(b'(');
                        i += 1;
                    }
                    b')' => {
                        result.push(b')');
                        i += 1;
                    }
                    b'\\' => {
                        result.push(b'\\');
                        i += 1;
                    }
                    b'\r' => {
                        // Backslash + CR (or CR+LF) = line continuation
                        i += 1;
                        if i < buf.len() && buf[i] == b'\n' {
                            i += 1;
                        }
                    }
                    b'\n' => {
                        // Backslash + LF = line continuation
                        i += 1;
                    }
                    b'0'..=b'7' => {
                        // Octal escape: 1-3 digits
                        let mut val = buf[i] - b'0';
                        i += 1;
                        if i < buf.len() && buf[i] >= b'0' && buf[i] <= b'7' {
                            val = val * 8 + (buf[i] - b'0');
                            i += 1;
                            if i < buf.len() && buf[i] >= b'0' && buf[i] <= b'7' {
                                val = val * 8 + (buf[i] - b'0');
                                i += 1;
                            }
                        }
                        result.push(val);
                    }
                    other => {
                        // Unknown escape — just include the character
                        result.push(other);
                        i += 1;
                    }
                }
            }
            b'\r' => {
                // Normalize CR and CR+LF to LF
                result.push(b'\n');
                i += 1;
                if i < buf.len() && buf[i] == b'\n' {
                    i += 1;
                }
            }
            other => {
                result.push(other);
                i += 1;
            }
        }
    }

    if depth != 0 {
        return Err(PdfError::Parse("unterminated literal string".into()));
    }
    Ok((PdfVal::Str(result), i))
}

/// Parse a hex string: <48656C6C6F>
fn parse_hex_string(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    debug_assert_eq!(buf[pos], b'<');
    let mut i = pos + 1;
    let mut hex_bytes = Vec::new();

    while i < buf.len() && buf[i] != b'>' {
        if is_whitespace(buf[i]) {
            i += 1;
            continue;
        }
        if let Some(d) = hex_digit(buf[i]) {
            hex_bytes.push(d);
        }
        i += 1;
    }

    if i < buf.len() {
        i += 1; // skip '>'
    }

    // Assemble bytes from hex digits. Odd count: last digit gets 0 appended.
    let mut result = Vec::with_capacity(hex_bytes.len().div_ceil(2));
    let mut j = 0;
    while j < hex_bytes.len() {
        let hi = hex_bytes[j];
        let lo = if j + 1 < hex_bytes.len() {
            hex_bytes[j + 1]
        } else {
            0
        };
        result.push(hi << 4 | lo);
        j += 2;
    }

    Ok((PdfVal::Str(result), i))
}

/// Parse a number (int or real), or an indirect reference (N G R).
fn parse_number_or_ref(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    // First, parse the token as a number
    let (num, end) = parse_number(buf, pos)?;

    // Check if this could be the start of an indirect reference: N G R
    if let PdfVal::Int(n) = num
        && n >= 0
    {
        let pos2 = skip_whitespace(buf, end);
        if pos2 < buf.len()
            && (buf[pos2].is_ascii_digit() || buf[pos2] == b'+')
            && let Ok((PdfVal::Int(g), end2)) = parse_number(buf, pos2)
            && g >= 0
        {
            let pos3 = skip_whitespace(buf, end2);
            if pos3 < buf.len() && buf[pos3] == b'R' {
                let after_r = pos3 + 1;
                if after_r >= buf.len() || !is_regular(buf[after_r]) {
                    return Ok((PdfVal::Ref(n as u32, g as u16), after_r));
                }
            }
        }
    }

    Ok((num, end))
}

/// Parse a numeric token (integer or real).
fn parse_number(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    let mut i = pos;
    let mut has_dot = false;

    // Optional sign
    if i < buf.len() && (buf[i] == b'+' || buf[i] == b'-') {
        i += 1;
    }

    let start = i;
    while i < buf.len() {
        if buf[i] == b'.' && !has_dot {
            has_dot = true;
            i += 1;
        } else if buf[i].is_ascii_digit() {
            i += 1;
        } else {
            break;
        }
    }

    if i == start && !has_dot {
        return Err(PdfError::Parse(format!("expected number at offset {pos}")));
    }

    let s = std::str::from_utf8(&buf[pos..i])
        .map_err(|_| PdfError::Parse("invalid number encoding".into()))?;

    if has_dot {
        let f: f64 = s
            .parse()
            .map_err(|_| PdfError::Parse(format!("invalid real: {s}")))?;
        Ok((PdfVal::Real(f), i))
    } else {
        let n: i64 = s
            .parse()
            .map_err(|_| PdfError::Parse(format!("invalid integer: {s}")))?;
        Ok((PdfVal::Int(n), i))
    }
}

/// Parse an array: [val val val ...]
fn parse_array(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    debug_assert_eq!(buf[pos], b'[');
    let mut i = pos + 1;
    let mut items = Vec::new();

    loop {
        i = skip_whitespace(buf, i);
        if i >= buf.len() {
            return Err(PdfError::Parse("unterminated array".into()));
        }
        if buf[i] == b']' {
            return Ok((PdfVal::Array(items), i + 1));
        }
        let (val, end) = parse_value(buf, i)?;
        items.push(val);
        i = end;
    }
}

/// Parse a dict (and possibly a stream).
fn parse_dict_or_stream(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    let (dict, end) = parse_dict(buf, pos)?;

    // Check if followed by `stream`
    let mut i = end;
    while i < buf.len() && is_whitespace(buf[i]) {
        i += 1;
    }

    if i + 6 <= buf.len() && &buf[i..i + 6] == b"stream" {
        i += 6;
        // stream keyword must be followed by a single EOL (CR, LF, or CRLF)
        if i < buf.len() && buf[i] == b'\r' {
            i += 1;
            if i < buf.len() && buf[i] == b'\n' {
                i += 1;
            }
        } else if i < buf.len() && buf[i] == b'\n' {
            i += 1;
        }

        // Try to get length from dict
        let stream_start = i;
        let length = dict_get(&dict, b"Length").and_then(|v| v.as_i64());

        let data;
        let stream_end;

        if let Some(len) = length {
            if len >= 0 {
                let len = len as usize;
                let end_pos = stream_start + len;
                if end_pos <= buf.len() {
                    data = buf[stream_start..end_pos].to_vec();
                    stream_end = end_pos;
                } else {
                    // Length exceeds file — fall back to scanning
                    let (d, e) = scan_for_endstream(buf, stream_start);
                    data = d;
                    stream_end = e;
                }
            } else {
                let (d, e) = scan_for_endstream(buf, stream_start);
                data = d;
                stream_end = e;
            }
        } else {
            // No direct length — /Length might be an indirect ref we can't resolve yet.
            // Scan for endstream.
            let (d, e) = scan_for_endstream(buf, stream_start);
            data = d;
            stream_end = e;
        }

        // Skip past `endstream`
        let mut after = stream_end;
        // Skip whitespace before endstream
        while after < buf.len() && is_whitespace(buf[after]) {
            after += 1;
        }
        if after + 9 <= buf.len() && &buf[after..after + 9] == b"endstream" {
            after += 9;
        }

        return Ok((
            PdfVal::Stream { dict, data },
            after,
        ));
    }

    Ok((PdfVal::Dict(dict), end))
}

/// Parse just the dict part: << /Key Value ... >>
fn parse_dict(buf: &[u8], pos: usize) -> Result<(BTreeMap<Vec<u8>, PdfVal>, usize), PdfError> {
    if pos + 1 >= buf.len() || buf[pos] != b'<' || buf[pos + 1] != b'<' {
        return Err(PdfError::Parse("expected '<<'".into()));
    }
    let mut i = pos + 2;
    let mut map = BTreeMap::new();

    loop {
        i = skip_whitespace(buf, i);
        if i >= buf.len() {
            return Err(PdfError::Parse("unterminated dict".into()));
        }
        if i + 1 < buf.len() && buf[i] == b'>' && buf[i + 1] == b'>' {
            return Ok((map, i + 2));
        }

        // Key must be a name
        if buf[i] != b'/' {
            return Err(PdfError::Parse(format!(
                "expected name key in dict at offset {i}, got 0x{:02x}",
                buf[i]
            )));
        }
        let (key_val, end) = parse_name(buf, i)?;
        let key = match key_val {
            PdfVal::Name(n) => n,
            _ => unreachable!(),
        };
        i = end;

        // Value
        let (val, end) = parse_value(buf, i)?;
        map.insert(key, val);
        i = end;
    }
}

/// Parse `true`, `false`, or treat as unknown keyword.
fn parse_bool_or_keyword(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    if buf[pos..].starts_with(b"true") && (pos + 4 >= buf.len() || !is_regular(buf[pos + 4])) {
        return Ok((PdfVal::Bool(true), pos + 4));
    }
    if buf[pos..].starts_with(b"false") && (pos + 5 >= buf.len() || !is_regular(buf[pos + 5])) {
        return Ok((PdfVal::Bool(false), pos + 5));
    }
    Err(PdfError::Parse(format!(
        "unexpected keyword at offset {pos}"
    )))
}

/// Parse `null` keyword.
fn parse_null_or_keyword(buf: &[u8], pos: usize) -> Result<(PdfVal, usize), PdfError> {
    if buf[pos..].starts_with(b"null") && (pos + 4 >= buf.len() || !is_regular(buf[pos + 4])) {
        return Ok((PdfVal::Null, pos + 4));
    }
    Err(PdfError::Parse(format!(
        "unexpected keyword at offset {pos}"
    )))
}

/// Parse an indirect object definition: `N G obj <value> endobj`
/// Returns (object_number, generation, value, end_position).
pub(crate) fn parse_indirect_object(
    buf: &[u8],
    pos: usize,
) -> Result<(u32, u16, PdfVal, usize), PdfError> {
    let pos = skip_whitespace(buf, pos);

    // Object number
    let (num_val, end) = parse_number(buf, pos)?;
    let obj_num = num_val
        .as_i64()
        .ok_or_else(|| PdfError::Parse("expected object number".into()))? as u32;

    // Generation number
    let pos = skip_whitespace(buf, end);
    let (gen_val, end) = parse_number(buf, pos)?;
    let generation = gen_val
        .as_i64()
        .ok_or_else(|| PdfError::Parse("expected generation number".into()))? as u16;

    // `obj` keyword
    let pos = skip_whitespace(buf, end);
    if !buf[pos..].starts_with(b"obj") {
        return Err(PdfError::Parse(format!(
            "expected 'obj' at offset {pos}"
        )));
    }
    let pos = pos + 3;

    // Parse the object value
    let (val, end) = parse_value(buf, pos)?;

    // Skip to `endobj`
    let mut pos = skip_whitespace(buf, end);
    if pos + 6 <= buf.len() && buf[pos..].starts_with(b"endobj") {
        pos += 6;
    }

    Ok((obj_num, generation, val, pos))
}

/// Scan for `endstream` marker, returning (data, position_of_endstream).
fn scan_for_endstream(buf: &[u8], start: usize) -> (Vec<u8>, usize) {
    // Look for "\nendstream" or "\r\nendstream" or "\rendstream"
    let needle = b"endstream";
    let mut i = start;
    while i + needle.len() <= buf.len() {
        if &buf[i..i + needle.len()] == needle {
            // Trim trailing whitespace before endstream
            let mut data_end = i;
            if data_end > start && buf[data_end - 1] == b'\n' {
                data_end -= 1;
            }
            if data_end > start && buf[data_end - 1] == b'\r' {
                data_end -= 1;
            }
            return (buf[start..data_end].to_vec(), i);
        }
        i += 1;
    }
    // No endstream found — take everything to EOF
    (buf[start..].to_vec(), buf.len())
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse hex digits from a byte slice, returning the numeric value and number of digits.
pub(crate) fn parse_hex_bytes(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut digits = Vec::new();
    for &b in data {
        if let Some(d) = hex_digit(b) {
            digits.push(d);
        }
    }
    let mut i = 0;
    while i < digits.len() {
        let hi = digits[i];
        let lo = if i + 1 < digits.len() { digits[i + 1] } else { 0 };
        result.push(hi << 4 | lo);
        i += 2;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_null() {
        let (val, end) = parse_value(b"null", 0).unwrap();
        assert!(val.is_null());
        assert_eq!(end, 4);
    }

    #[test]
    fn test_parse_bools() {
        let (val, _) = parse_value(b"true", 0).unwrap();
        assert!(matches!(val, PdfVal::Bool(true)));
        let (val, _) = parse_value(b"false", 0).unwrap();
        assert!(matches!(val, PdfVal::Bool(false)));
    }

    #[test]
    fn test_parse_integers() {
        let (val, _) = parse_value(b"42", 0).unwrap();
        assert_eq!(val.as_i64(), Some(42));
        let (val, _) = parse_value(b"-17", 0).unwrap();
        assert_eq!(val.as_i64(), Some(-17));
        let (val, _) = parse_value(b"+3", 0).unwrap();
        assert_eq!(val.as_i64(), Some(3));
        let (val, _) = parse_value(b"0", 0).unwrap();
        assert_eq!(val.as_i64(), Some(0));
    }

    #[test]
    fn test_parse_reals() {
        let (val, _) = parse_value(b"3.75", 0).unwrap();
        assert!((val.as_f64().unwrap() - 3.75).abs() < 1e-10);
        let (val, _) = parse_value(b"-.5", 0).unwrap();
        assert!((val.as_f64().unwrap() + 0.5).abs() < 1e-10);
        let (val, _) = parse_value(b"1.", 0).unwrap();
        assert!((val.as_f64().unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_name() {
        let (val, _) = parse_value(b"/Type", 0).unwrap();
        assert_eq!(val.as_name(), Some(b"Type".as_slice()));
        let (val, _) = parse_value(b"/A#20B", 0).unwrap();
        assert_eq!(val.as_name(), Some(b"A B".as_slice()));
        let (val, end) = parse_value(b"/ ", 0).unwrap();
        assert_eq!(val.as_name(), Some(b"".as_slice()));
        assert_eq!(end, 1);
    }

    #[test]
    fn test_parse_literal_string() {
        let (val, _) = parse_value(b"(hello)", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(b"hello".as_slice()));

        // Nested parens
        let (val, _) = parse_value(b"(a(b)c)", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(b"a(b)c".as_slice()));

        // Escape sequences
        let (val, _) = parse_value(b"(a\\nb)", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(b"a\nb".as_slice()));

        // Octal escape
        let (val, _) = parse_value(b"(\\101)", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(b"A".as_slice()));
    }

    #[test]
    fn test_parse_hex_string() {
        let (val, _) = parse_value(b"<48656C6C6F>", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(b"Hello".as_slice()));

        // Odd digits — last digit padded with 0
        let (val, _) = parse_value(b"<ABC>", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(&[0xAB, 0xC0][..]));

        // Whitespace inside
        let (val, _) = parse_value(b"<48 65>", 0).unwrap();
        assert_eq!(val.as_str_bytes(), Some(b"He".as_slice()));
    }

    #[test]
    fn test_parse_array() {
        let (val, _) = parse_value(b"[1 2 3]", 0).unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_i64(), Some(1));
        assert_eq!(arr[2].as_i64(), Some(3));

        // Empty array
        let (val, _) = parse_value(b"[]", 0).unwrap();
        assert_eq!(val.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_parse_dict() {
        let (val, _) = parse_value(b"<< /Type /Page /Count 5 >>", 0).unwrap();
        let dict = val.as_dict().unwrap();
        assert_eq!(dict_get(dict, b"Type").unwrap().as_name(), Some(b"Page".as_slice()));
        assert_eq!(dict_get(dict, b"Count").unwrap().as_i64(), Some(5));
    }

    #[test]
    fn test_parse_ref() {
        let (val, end) = parse_value(b"10 0 R", 0).unwrap();
        assert_eq!(val.as_ref(), Some((10, 0)));
        assert_eq!(end, 6);
    }

    #[test]
    fn test_parse_indirect_object() {
        let input = b"5 0 obj\n<< /Type /Page >>\nendobj";
        let (num, generation, val, _) = parse_indirect_object(input, 0).unwrap();
        assert_eq!(num, 5);
        assert_eq!(generation, 0);
        assert_eq!(dict_get(val.as_dict().unwrap(), b"Type").unwrap().as_name(), Some(b"Page".as_slice()));
    }

    #[test]
    fn test_parse_stream() {
        let input = b"<< /Length 5 >>\nstream\nHello\nendstream";
        let (val, _) = parse_value(input, 0).unwrap();
        let (dict, data) = val.as_stream().unwrap();
        assert_eq!(dict_get(dict, b"Length").unwrap().as_i64(), Some(5));
        assert_eq!(data, b"Hello");
    }

    #[test]
    fn test_skip_comments() {
        let pos = skip_whitespace(b"% this is a comment\n42", 0);
        assert_eq!(pos, 20); // should be at '4'
        let (val, _) = parse_value(b"% this is a comment\n42", 0).unwrap();
        assert_eq!(val.as_i64(), Some(42));
    }

    #[test]
    fn test_parse_number_not_ref() {
        // "1 0" at end of input — not enough for a ref (no R)
        let (val, _) = parse_value(b"1 0]", 0).unwrap();
        assert_eq!(val.as_i64(), Some(1));
    }

    #[test]
    fn test_parse_negative_not_ref() {
        let (val, _) = parse_value(b"-5", 0).unwrap();
        assert_eq!(val.as_i64(), Some(-5));
    }
}
