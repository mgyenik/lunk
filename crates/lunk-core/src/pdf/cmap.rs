//! ToUnicode CMap parser.
//!
//! Parses the PostScript-like CMap programs embedded in PDF fonts to map
//! character codes to Unicode values. Handles bfchar, bfrange (both
//! incrementing and array forms), and codespacerange.

/// A parsed ToUnicode CMap.
#[derive(Debug, Clone)]
pub(crate) struct CMap {
    /// Valid code space ranges: (low, high, byte_length).
    code_spaces: Vec<(u32, u32, u8)>,
    /// Individual character mappings from bfchar.
    bf_chars: Vec<(u32, u8, Vec<u16>)>, // (code, code_len, unicode)
    /// Range mappings from bfrange.
    bf_ranges: Vec<BfRange>,
}

#[derive(Debug, Clone)]
enum BfRange {
    /// Incrementing: codes start..=end map to base_unicode + (code - start).
    Incrementing {
        start: u32,
        end: u32,
        code_len: u8,
        base: Vec<u16>,
    },
    /// Array: codes start..=end map to explicit unicode values.
    Array {
        start: u32,
        end: u32,
        code_len: u8,
        values: Vec<Vec<u16>>,
    },
}

impl CMap {
    /// Parse a ToUnicode CMap from its decompressed stream data.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let text = String::from_utf8_lossy(data);
        let mut cmap = CMap {
            code_spaces: Vec::new(),
            bf_chars: Vec::new(),
            bf_ranges: Vec::new(),
        };

        // Parse codespacerange sections
        let mut pos = 0;
        while let Some(start) = text[pos..].find("begincodespacerange") {
            let section_start = pos + start + "begincodespacerange".len();
            let section_end = text[section_start..]
                .find("endcodespacerange")
                .map(|p| section_start + p)
                .unwrap_or(text.len());

            parse_codespace_ranges(&text[section_start..section_end], &mut cmap.code_spaces);
            pos = section_end;
        }

        // Default code space if none specified
        if cmap.code_spaces.is_empty() {
            cmap.code_spaces.push((0, 0xFF, 1));
        }

        // Parse bfchar sections
        pos = 0;
        while let Some(start) = text[pos..].find("beginbfchar") {
            let section_start = pos + start + "beginbfchar".len();
            let section_end = text[section_start..]
                .find("endbfchar")
                .map(|p| section_start + p)
                .unwrap_or(text.len());

            parse_bf_chars(&text[section_start..section_end], &cmap.code_spaces, &mut cmap.bf_chars);
            pos = section_end;
        }

        // Parse bfrange sections
        pos = 0;
        while let Some(start) = text[pos..].find("beginbfrange") {
            let section_start = pos + start + "beginbfrange".len();
            let section_end = text[section_start..]
                .find("endbfrange")
                .map(|p| section_start + p)
                .unwrap_or(text.len());

            parse_bf_ranges(&text[section_start..section_end], &cmap.code_spaces, &mut cmap.bf_ranges);
            pos = section_end;
        }

        if cmap.bf_chars.is_empty() && cmap.bf_ranges.is_empty() {
            return None;
        }

        Some(cmap)
    }

    /// Look up a character code and return its Unicode value(s).
    pub fn lookup(&self, code: u32, code_len: u8) -> Option<Vec<u16>> {
        // Check bfchar first (exact matches)
        for (c, cl, unicode) in &self.bf_chars {
            if *c == code && *cl == code_len {
                return Some(unicode.clone());
            }
        }

        // Check bfrange
        for range in &self.bf_ranges {
            match range {
                BfRange::Incrementing {
                    start,
                    end,
                    code_len: cl,
                    base,
                } => {
                    if *cl == code_len && code >= *start && code <= *end {
                        let offset = code - *start;
                        let mut result = base.clone();
                        // Increment the last element
                        if let Some(last) = result.last_mut() {
                            *last = last.wrapping_add(offset as u16);
                        }
                        return Some(result);
                    }
                }
                BfRange::Array {
                    start,
                    end,
                    code_len: cl,
                    values,
                } => {
                    if *cl == code_len && code >= *start && code <= *end {
                        let idx = (code - *start) as usize;
                        if idx < values.len() {
                            return Some(values[idx].clone());
                        }
                    }
                }
            }
        }

        None
    }

    /// Decode a byte sequence using this CMap.
    /// Consumes bytes greedily according to codespace ranges.
    pub fn decode(&self, bytes: &[u8]) -> String {
        let mut result = Vec::new();
        let mut i = 0;

        while i < bytes.len() {
            // Try matching codespace ranges from longest to shortest
            let mut matched = false;
            // Sort code spaces by byte length descending for greedy matching
            let mut sorted_spaces: Vec<_> = self.code_spaces.iter().collect();
            sorted_spaces.sort_by(|a, b| b.2.cmp(&a.2));

            for &&(low, high, byte_len) in &sorted_spaces {
                let bl = byte_len as usize;
                if i + bl > bytes.len() {
                    continue;
                }
                let code = read_be_uint(&bytes[i..i + bl]);
                if code >= low as u64 && code <= high as u64 {
                    if let Some(unicode) = self.lookup(code as u32, byte_len) {
                        result.extend_from_slice(&unicode);
                    }
                    i += bl;
                    matched = true;
                    break;
                }
            }

            if !matched {
                // No codespace matched — try single byte
                if let Some(unicode) = self.lookup(bytes[i] as u32, 1) {
                    result.extend_from_slice(&unicode);
                }
                i += 1;
            }
        }

        String::from_utf16_lossy(&result)
    }
}

/// Read a big-endian unsigned integer from bytes.
fn read_be_uint(bytes: &[u8]) -> u64 {
    let mut val: u64 = 0;
    for &b in bytes {
        val = (val << 8) | b as u64;
    }
    val
}

/// Parse hex string `<ABCD>` and return (value, byte_length).
fn parse_hex_code(s: &str) -> Option<(u32, u8)> {
    let s = s.trim();
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    let bytes = hex_to_bytes(inner)?;
    let byte_len = bytes.len() as u8;
    let mut val: u32 = 0;
    for &b in &bytes {
        val = (val << 8) | b as u32;
    }
    Some((val, byte_len))
}

/// Parse a hex string into a Vec<u16> (UTF-16BE).
fn parse_hex_unicode(s: &str) -> Option<Vec<u16>> {
    let s = s.trim();
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    let bytes = hex_to_bytes(inner)?;
    if bytes.is_empty() {
        return Some(Vec::new()); // Empty mapping = no output
    }
    // Interpret as UTF-16BE
    let mut result = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        result.push(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
        i += 2;
    }
    // Handle odd byte (shouldn't happen in well-formed CMap, but be lenient)
    if i < bytes.len() {
        result.push(u16::from_be_bytes([bytes[i], 0]));
    }
    Some(result)
}

/// Decode hex digits to bytes.
fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    let mut bytes = Vec::new();
    let mut chars = hex.chars();
    loop {
        let hi = match chars.next() {
            Some(c) if c.is_ascii_whitespace() => continue,
            Some(c) => hex_val(c)?,
            None => break,
        };
        let lo = match chars.next() {
            Some(c) if c.is_ascii_whitespace() => {
                bytes.push(hi << 4);
                continue;
            }
            Some(c) => hex_val(c)?,
            None => {
                bytes.push(hi << 4);
                break;
            }
        };
        bytes.push(hi << 4 | lo);
    }
    Some(bytes)
}

fn hex_val(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

/// Determine the byte length for a code value based on codespace ranges.
fn code_byte_len(code_spaces: &[(u32, u32, u8)], code: u32) -> u8 {
    for &(low, high, bl) in code_spaces {
        if code >= low && code <= high {
            return bl;
        }
    }
    // Guess from code magnitude
    if code > 0xFFFF {
        4
    } else if code > 0xFF {
        2
    } else {
        1
    }
}

fn parse_codespace_ranges(section: &str, ranges: &mut Vec<(u32, u32, u8)>) {
    let tokens = extract_hex_tokens(section);
    let mut i = 0;
    while i + 1 < tokens.len() {
        if let (Some((low, bl_low)), Some((high, _))) =
            (parse_hex_code(&tokens[i]), parse_hex_code(&tokens[i + 1]))
        {
            ranges.push((low, high, bl_low));
        }
        i += 2;
    }
}

fn parse_bf_chars(section: &str, code_spaces: &[(u32, u32, u8)], chars: &mut Vec<(u32, u8, Vec<u16>)>) {
    let tokens = extract_hex_tokens(section);
    let mut i = 0;
    while i + 1 < tokens.len() {
        if let (Some((code, bl)), Some(unicode)) =
            (parse_hex_code(&tokens[i]), parse_hex_unicode(&tokens[i + 1]))
        {
            let byte_len = if bl > 0 { bl } else { code_byte_len(code_spaces, code) };
            chars.push((code, byte_len, unicode));
        }
        i += 2;
    }
}

fn parse_bf_ranges(section: &str, code_spaces: &[(u32, u32, u8)], ranges: &mut Vec<BfRange>) {
    // bfrange entries: <start> <end> <base_unicode> OR <start> <end> [<u1> <u2> ...]
    let text = section.trim();
    let mut pos = 0;

    while pos < text.len() {
        // Skip whitespace
        while pos < text.len() && text.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= text.len() {
            break;
        }

        // Parse start code
        let start_token = match extract_one_hex_token(text, pos) {
            Some((tok, new_pos)) => {
                pos = new_pos;
                tok
            }
            None => break,
        };

        // Parse end code
        skip_ws(text, &mut pos);
        let end_token = match extract_one_hex_token(text, pos) {
            Some((tok, new_pos)) => {
                pos = new_pos;
                tok
            }
            None => break,
        };

        skip_ws(text, &mut pos);

        let (start, bl) = match parse_hex_code(&start_token) {
            Some(v) => v,
            None => continue,
        };
        let (end, _) = match parse_hex_code(&end_token) {
            Some(v) => v,
            None => continue,
        };
        let byte_len = if bl > 0 { bl } else { code_byte_len(code_spaces, start) };

        if pos >= text.len() {
            break;
        }

        // Check if next token is [ (array form) or < (incrementing form)
        if text.as_bytes()[pos] == b'[' {
            // Array form
            pos += 1; // skip '['
            let mut values = Vec::new();
            loop {
                skip_ws(text, &mut pos);
                if pos >= text.len() || text.as_bytes()[pos] == b']' {
                    if pos < text.len() {
                        pos += 1;
                    }
                    break;
                }
                if let Some((tok, new_pos)) = extract_one_hex_token(text, pos) {
                    pos = new_pos;
                    if let Some(unicode) = parse_hex_unicode(&tok) {
                        values.push(unicode);
                    }
                } else {
                    pos += 1; // skip unexpected char
                }
            }
            ranges.push(BfRange::Array {
                start,
                end,
                code_len: byte_len,
                values,
            });
        } else if let Some((tok, new_pos)) = extract_one_hex_token(text, pos) {
            // Incrementing form
            pos = new_pos;
            if let Some(base) = parse_hex_unicode(&tok) {
                ranges.push(BfRange::Incrementing {
                    start,
                    end,
                    code_len: byte_len,
                    base,
                });
            }
        }
    }
}

fn skip_ws(text: &str, pos: &mut usize) {
    while *pos < text.len() && text.as_bytes()[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

/// Extract all `<hex>` tokens from a section of text.
fn extract_hex_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'>' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // include '>'
            }
            tokens.push(text[start..i].to_string());
        } else {
            i += 1;
        }
    }
    tokens
}

/// Extract one `<hex>` token starting at pos.
fn extract_one_hex_token(text: &str, pos: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'<' {
        return None;
    }
    let start = pos;
    let mut i = pos + 1;
    while i < bytes.len() && bytes[i] != b'>' {
        i += 1;
    }
    if i < bytes.len() {
        i += 1; // include '>'
    }
    Some((text[start..i].to_string(), i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_cmap() {
        let cmap_data = br#"
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /test def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
3 beginbfchar
<41> <0041>
<42> <0042>
<20> <0020>
endbfchar
endcmap
"#;
        let cmap = CMap::parse(cmap_data).unwrap();
        assert_eq!(cmap.lookup(0x41, 1), Some(vec![0x0041])); // A
        assert_eq!(cmap.lookup(0x42, 1), Some(vec![0x0042])); // B
        assert_eq!(cmap.lookup(0x20, 1), Some(vec![0x0020])); // space
        assert_eq!(cmap.lookup(0x43, 1), None); // C not mapped
    }

    #[test]
    fn test_bfrange_incrementing() {
        let cmap_data = br#"
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfrange
<41> <5A> <0041>
endbfrange
"#;
        let cmap = CMap::parse(cmap_data).unwrap();
        assert_eq!(cmap.lookup(0x41, 1), Some(vec![0x0041])); // A
        assert_eq!(cmap.lookup(0x42, 1), Some(vec![0x0042])); // B
        assert_eq!(cmap.lookup(0x5A, 1), Some(vec![0x005A])); // Z
    }

    #[test]
    fn test_bfrange_array() {
        let cmap_data = br#"
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfrange
<02> <04> [<0066006C> <00660069> <00660066>]
endbfrange
"#;
        let cmap = CMap::parse(cmap_data).unwrap();
        assert_eq!(cmap.lookup(0x02, 1), Some(vec![0x0066, 0x006C])); // fl
        assert_eq!(cmap.lookup(0x03, 1), Some(vec![0x0066, 0x0069])); // fi
        assert_eq!(cmap.lookup(0x04, 1), Some(vec![0x0066, 0x0066])); // ff
    }

    #[test]
    fn test_two_byte_codes() {
        let cmap_data = br#"
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfchar
<0003> <0020>
<0011> <002F>
endbfchar
"#;
        let cmap = CMap::parse(cmap_data).unwrap();
        assert_eq!(cmap.lookup(0x0003, 2), Some(vec![0x0020])); // space
        assert_eq!(cmap.lookup(0x0011, 2), Some(vec![0x002F])); // /
    }

    #[test]
    fn test_decode_bytes() {
        let cmap_data = br#"
1 begincodespacerange
<00> <FF>
endcodespacerange
3 beginbfchar
<48> <0048>
<69> <0069>
<21> <0021>
endbfchar
"#;
        let cmap = CMap::parse(cmap_data).unwrap();
        let text = cmap.decode(&[0x48, 0x69, 0x21]); // H i !
        assert_eq!(text, "Hi!");
    }

    #[test]
    fn test_multi_char_bfchar() {
        // bfchar mapping a single code to multiple unicode chars (e.g., ligature)
        let cmap_data = br#"
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfchar
<03> <006600660069>
endbfchar
"#;
        let cmap = CMap::parse(cmap_data).unwrap();
        let result = cmap.lookup(0x03, 1).unwrap();
        // Should decode to "ffi"
        assert_eq!(String::from_utf16_lossy(&result), "ffi");
    }
}
