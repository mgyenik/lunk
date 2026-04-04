//! Font encoding resolution.
//!
//! Maps PDF font character codes to Unicode text. Resolution priority:
//! 1. ToUnicode CMap (most reliable)
//! 2. Named encoding (/WinAnsiEncoding, etc.)
//! 3. Encoding dict with /BaseEncoding + /Differences
//! 4. Adobe Glyph List (from glyph names in /Differences)
//! 5. Fallback to StandardEncoding

use std::collections::BTreeMap;

use super::cmap::CMap;
use super::document::PdfDoc;
use super::encodings_data;
use super::glyphlist;
use super::parser::{PdfVal, dict_get};

/// A resolved font encoding.
pub(crate) enum FontEncoding {
    /// CMap-based encoding (from /ToUnicode), with an optional base encoding
    /// fallback for codes not covered by the CMap (common with subset fonts).
    ToUnicode(CMap, Option<&'static encodings_data::CharMap>),
    /// Standard named encoding (WinAnsi, MacRoman, etc.).
    Named(&'static encodings_data::CharMap),
    /// Named encoding with /Differences overrides.
    Differences {
        base: &'static encodings_data::CharMap,
        overrides: BTreeMap<u8, Vec<u16>>,
    },
    /// Identity-H/V with a ToUnicode CMap.
    IdentityH(CMap),
    /// Identity-H/V without ToUnicode — treat bytes as big-endian u16.
    IdentityHRaw,
}

/// Resolve the encoding for a font dictionary.
pub(crate) fn resolve_font_encoding(
    doc: &PdfDoc,
    font_dict: &BTreeMap<Vec<u8>, PdfVal>,
) -> FontEncoding {
    // Priority 1: /ToUnicode CMap
    if let Some(to_unicode) = dict_get(font_dict, b"ToUnicode") {
        let resolved = doc.resolve(to_unicode);
        if let Ok(data) = doc.stream_data(resolved)
            && let Some(cmap) = CMap::parse(&data)
        {
            let enc_name = dict_get(font_dict, b"Encoding")
                .map(|v| doc.resolve(v))
                .and_then(|v| v.as_name().map(|n| n.to_vec()));

            if enc_name.as_deref() == Some(b"Identity-H")
                || enc_name.as_deref() == Some(b"Identity-V")
            {
                return FontEncoding::IdentityH(cmap);
            }

            // Include the base encoding as fallback for codes not in the CMap
            let base = enc_name
                .as_deref()
                .map(named_encoding);

            return FontEncoding::ToUnicode(cmap, base);
        }
    }

    // Get the encoding entry
    let encoding_val = dict_get(font_dict, b"Encoding").map(|v| doc.resolve(v));

    // Priority 2: Named encoding (direct name)
    if let Some(name) = encoding_val.and_then(|v| v.as_name()) {
        match name {
            b"Identity-H" | b"Identity-V" => return FontEncoding::IdentityHRaw,
            _ => {
                let base = named_encoding(name);
                return FontEncoding::Named(base);
            }
        }
    }

    // Priority 3: Encoding dictionary with /BaseEncoding + /Differences
    if let Some(enc_dict) = encoding_val.and_then(|v| v.as_dict()) {
        let base_name = dict_get(enc_dict, b"BaseEncoding")
            .and_then(|v| v.as_name());
        let base = base_name
            .map(named_encoding)
            .unwrap_or(&encodings_data::STANDARD_ENCODING);

        if let Some(diff_arr) = dict_get(enc_dict, b"Differences").and_then(|v| v.as_array()) {
            let overrides = parse_differences(diff_arr);
            if !overrides.is_empty() {
                return FontEncoding::Differences { base, overrides };
            }
        }

        return FontEncoding::Named(base);
    }

    // Priority 4: Check font's base font name for well-known fonts
    // (e.g., /BaseFont /Symbol uses SYMBOL_ENCODING)
    if let Some(base_font) = dict_get(font_dict, b"BaseFont").and_then(|v| v.as_name()) {
        // Strip subset prefix (e.g., "ABCDEF+Symbol" -> "Symbol")
        let font_name = strip_subset_prefix(base_font);
        if font_name == b"Symbol" {
            return FontEncoding::Named(&encodings_data::SYMBOL_ENCODING);
        }
        if font_name == b"ZapfDingbats" {
            // ZapfDingbats uses its own encoding, close to SYMBOL_ENCODING
            return FontEncoding::Named(&encodings_data::SYMBOL_ENCODING);
        }
    }

    // Fallback: StandardEncoding
    FontEncoding::Named(&encodings_data::STANDARD_ENCODING)
}

/// Decode bytes using a font encoding, returning Unicode text.
pub(crate) fn decode_text(encoding: &FontEncoding, bytes: &[u8]) -> String {
    match encoding {
        FontEncoding::ToUnicode(cmap, fallback) => {
            decode_cmap_with_fallback(cmap, fallback.as_ref(), bytes)
        }
        FontEncoding::IdentityH(cmap) => cmap.decode(bytes),
        FontEncoding::Named(table) => decode_single_byte(table, bytes),
        FontEncoding::Differences { base, overrides } => {
            decode_with_differences(base, overrides, bytes)
        }
        FontEncoding::IdentityHRaw => {
            // Treat as big-endian u16 pairs
            let mut u16s = Vec::new();
            let mut i = 0;
            while i + 1 < bytes.len() {
                u16s.push(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
                i += 2;
            }
            String::from_utf16_lossy(&u16s)
        }
    }
}

/// Decode text from a PDF string object (handles UTF-16BE BOM and PDFDocEncoding).
pub(crate) fn decode_pdf_string(bytes: &[u8]) -> String {
    // Check for UTF-16BE BOM
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let mut u16s = Vec::new();
        let mut i = 2;
        while i + 1 < bytes.len() {
            u16s.push(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
            i += 2;
        }
        return String::from_utf16_lossy(&u16s);
    }

    // Check for UTF-8 BOM
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }

    // PDFDocEncoding (superset of ASCII, similar to Latin-1)
    decode_single_byte(&encodings_data::PDF_DOC_ENCODING, bytes)
}

/// Decode using a CMap, falling back to a base encoding for unmapped codes.
fn decode_cmap_with_fallback(
    cmap: &CMap,
    fallback: Option<&&'static encodings_data::CharMap>,
    bytes: &[u8],
) -> String {
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        // Try CMap lookup (1-byte code)
        if let Some(unicode) = cmap.lookup(bytes[i] as u32, 1) {
            result.push_str(&String::from_utf16_lossy(&unicode));
            i += 1;
        } else if let Some(table) = fallback {
            // Fallback to base encoding
            let b = bytes[i];
            if let Some(code_point) = table[b as usize] {
                if let Some(c) = char::from_u32(code_point as u32) {
                    result.push(c);
                }
            } else if (0x20..0x7F).contains(&b) {
                result.push(b as char);
            }
            i += 1;
        } else {
            // No fallback — try as ASCII
            let b = bytes[i];
            if (0x20..0x7F).contains(&b) {
                result.push(b as char);
            }
            i += 1;
        }
    }

    result
}

/// Decode single-byte encoded text.
fn decode_single_byte(table: &encodings_data::CharMap, bytes: &[u8]) -> String {
    let mut result = String::new();
    for &b in bytes {
        match table[b as usize] {
            Some(code_point) => {
                if let Some(c) = char::from_u32(code_point as u32) {
                    result.push(c);
                }
            }
            None => {
                // Unmapped byte — try as ASCII/Latin-1 fallback
                if (0x20..0x7F).contains(&b) {
                    result.push(b as char);
                }
            }
        }
    }
    result
}

/// Decode with a base encoding + /Differences overrides.
fn decode_with_differences(
    base: &encodings_data::CharMap,
    overrides: &BTreeMap<u8, Vec<u16>>,
    bytes: &[u8],
) -> String {
    let mut result = String::new();
    for &b in bytes {
        if let Some(unicode) = overrides.get(&b) {
            result.push_str(&String::from_utf16_lossy(unicode));
        } else if let Some(code_point) = base[b as usize] {
            if let Some(c) = char::from_u32(code_point as u32) {
                result.push(c);
            }
        } else if (0x20..0x7F).contains(&b) {
            result.push(b as char);
        }
    }
    result
}

/// Parse a /Differences array into a map of byte code -> Unicode.
fn parse_differences(arr: &[PdfVal]) -> BTreeMap<u8, Vec<u16>> {
    let mut overrides = BTreeMap::new();
    let mut code: u16 = 0;

    for item in arr {
        match item {
            PdfVal::Int(n) => {
                code = *n as u16;
            }
            PdfVal::Name(glyph_name) => {
                if code <= 255
                    && let Some(unicode) = glyphlist::glyph_to_unicode(glyph_name)
                {
                    overrides.insert(code as u8, unicode);
                }
                code = code.saturating_add(1);
            }
            _ => {}
        }
    }

    overrides
}

/// Get the named encoding table for a given encoding name.
fn named_encoding(name: &[u8]) -> &'static encodings_data::CharMap {
    match name {
        b"WinAnsiEncoding" => &encodings_data::WIN_ANSI_ENCODING,
        b"MacRomanEncoding" => &encodings_data::MAC_ROMAN_ENCODING,
        b"MacExpertEncoding" => &encodings_data::MAC_EXPERT_ENCODING,
        b"StandardEncoding" => &encodings_data::STANDARD_ENCODING,
        b"PDFDocEncoding" => &encodings_data::PDF_DOC_ENCODING,
        b"SymbolEncoding" => &encodings_data::SYMBOL_ENCODING,
        _ => &encodings_data::STANDARD_ENCODING,
    }
}

/// Strip the 6-letter subset prefix from a font name (e.g., "ABCDEF+Arial" -> "Arial").
fn strip_subset_prefix(name: &[u8]) -> &[u8] {
    if name.len() > 7
        && name[6] == b'+'
        && name[..6].iter().all(|b| b.is_ascii_uppercase())
    {
        &name[7..]
    } else {
        name
    }
}

/// Expand Unicode ligature code points to their component characters.
pub(crate) fn expand_ligatures(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{FB00}' => result.push_str("ff"),
            '\u{FB01}' => result.push_str("fi"),
            '\u{FB02}' => result.push_str("fl"),
            '\u{FB03}' => result.push_str("ffi"),
            '\u{FB04}' => result.push_str("ffl"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_winansi() {
        let encoding = FontEncoding::Named(&encodings_data::WIN_ANSI_ENCODING);
        let text = decode_text(&encoding, &[0x48, 0x65, 0x6C, 0x6C, 0x6F]);
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_decode_winansi_special() {
        let encoding = FontEncoding::Named(&encodings_data::WIN_ANSI_ENCODING);
        // 0x93 = left double quotation mark, 0x94 = right double quotation mark
        let text = decode_text(&encoding, &[0x93, 0x48, 0x69, 0x94]);
        assert_eq!(text, "\u{201C}Hi\u{201D}");
    }

    #[test]
    fn test_decode_identity_raw() {
        let encoding = FontEncoding::IdentityHRaw;
        let text = decode_text(&encoding, &[0x00, 0x48, 0x00, 0x69]);
        assert_eq!(text, "Hi");
    }

    #[test]
    fn test_decode_differences() {
        let mut overrides = BTreeMap::new();
        overrides.insert(1, vec![0x0041]); // 1 -> A
        overrides.insert(2, vec![0x0042]); // 2 -> B
        let encoding = FontEncoding::Differences {
            base: &encodings_data::STANDARD_ENCODING,
            overrides,
        };
        let text = decode_text(&encoding, &[1, 2]);
        assert_eq!(text, "AB");
    }

    #[test]
    fn test_parse_differences_array() {
        let arr = vec![
            PdfVal::Int(32),
            PdfVal::Name(b"space".to_vec()),
            PdfVal::Int(97),
            PdfVal::Name(b"a".to_vec()),
            PdfVal::Name(b"b".to_vec()),
            PdfVal::Name(b"c".to_vec()),
        ];
        let overrides = parse_differences(&arr);
        assert_eq!(overrides.get(&32), Some(&vec![0x0020])); // space
        assert_eq!(overrides.get(&97), Some(&vec![0x0061])); // a
        assert_eq!(overrides.get(&98), Some(&vec![0x0062])); // b
        assert_eq!(overrides.get(&99), Some(&vec![0x0063])); // c
    }

    #[test]
    fn test_decode_pdf_string_utf16() {
        let mut bytes = vec![0xFE, 0xFF]; // BOM
        bytes.extend_from_slice(&[0x00, 0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F]);
        assert_eq!(decode_pdf_string(&bytes), "Hello");
    }

    #[test]
    fn test_decode_pdf_string_pdfdoc() {
        let bytes = b"Hello";
        assert_eq!(decode_pdf_string(bytes), "Hello");
    }

    #[test]
    fn test_expand_ligatures() {
        assert_eq!(expand_ligatures("of\u{FB01}ce"), "office");
        assert_eq!(expand_ligatures("a\u{FB00}ect"), "affect");
        assert_eq!(expand_ligatures("no ligatures"), "no ligatures");
    }

    #[test]
    fn test_strip_subset_prefix() {
        assert_eq!(strip_subset_prefix(b"ABCDEF+Arial"), b"Arial");
        assert_eq!(strip_subset_prefix(b"Arial"), b"Arial");
        assert_eq!(strip_subset_prefix(b"AB+Foo"), b"AB+Foo"); // only 2 letters
    }
}
