//! Cross-reference table parsing.
//!
//! Handles traditional xref tables, cross-reference streams (PDF 1.5+),
//! incremental updates via /Prev chains, hybrid files (/XRefStm), and
//! brute-force object scanning as a last resort.

use std::collections::BTreeMap;

use super::parser::{self, PdfVal, dict_get};
use super::stream;
use super::PdfError;

/// A parsed xref section: entries + the trailer dictionary.
type XrefSection = (BTreeMap<u32, XrefEntry>, BTreeMap<Vec<u8>, PdfVal>);

/// An entry in the cross-reference table.
#[derive(Debug, Clone)]
pub(crate) enum XrefEntry {
    /// Object at a byte offset in the file.
    Normal { offset: usize },
    /// Object compressed inside an object stream.
    Compressed { container: u32, index: u32 },
    /// Free (deleted) object.
    Free,
}

/// Parsed cross-reference table plus trailer metadata.
pub(crate) struct XrefResult {
    pub entries: BTreeMap<u32, XrefEntry>,
    pub root_ref: Option<(u32, u16)>,
    pub info_ref: Option<(u32, u16)>,
}

/// Parse the full xref structure from a PDF buffer.
/// Follows /Prev chains, handles hybrid /XRefStm, falls back to brute-force.
pub(crate) fn parse_xref(buf: &[u8]) -> Result<XrefResult, PdfError> {
    // Find startxref offset
    let startxref = find_startxref(buf)?;

    let mut entries = BTreeMap::new();
    let mut root_ref: Option<(u32, u16)> = None;
    let mut info_ref: Option<(u32, u16)> = None;

    // Follow the /Prev chain
    let mut offset = Some(startxref);
    let mut seen_offsets = std::collections::HashSet::new();

    while let Some(xref_offset) = offset {
        if !seen_offsets.insert(xref_offset) {
            break; // Cycle detection
        }
        if xref_offset >= buf.len() {
            break;
        }

        let result = parse_xref_at(buf, xref_offset);
        match result {
            Ok((section_entries, trailer_dict)) => {
                // Merge entries — earlier entries (from newer revisions) take priority
                for (obj_num, entry) in section_entries {
                    entries.entry(obj_num).or_insert(entry);
                }

                // Extract /Root and /Info from whichever trailer has them
                if root_ref.is_none() {
                    root_ref = extract_ref(&trailer_dict, b"Root");
                }
                if info_ref.is_none() {
                    info_ref = extract_ref(&trailer_dict, b"Info");
                }

                // Handle hybrid /XRefStm
                if let Some(xrefstm_offset) = dict_get(&trailer_dict, b"XRefStm")
                    .and_then(|v| v.as_i64())
                    && let Ok((stm_entries, _)) = parse_xref_at(buf, xrefstm_offset as usize)
                {
                    for (obj_num, entry) in stm_entries {
                        entries.entry(obj_num).or_insert(entry);
                    }
                }

                // Follow /Prev
                offset = dict_get(&trailer_dict, b"Prev")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as usize);
            }
            Err(_) => {
                // This xref section failed — try brute force
                break;
            }
        }
    }

    // If we got nothing useful, try brute-force scanning
    if entries.is_empty() || root_ref.is_none() {
        let (bf_entries, bf_root) = brute_force_scan(buf);
        for (obj_num, entry) in bf_entries {
            entries.entry(obj_num).or_insert(entry);
        }
        if root_ref.is_none() {
            root_ref = bf_root;
        }
    }

    if root_ref.is_none() {
        return Err(PdfError::NoRoot);
    }

    Ok(XrefResult {
        entries,
        root_ref,
        info_ref,
    })
}

/// Find the `startxref` offset by scanning backwards from EOF.
fn find_startxref(buf: &[u8]) -> Result<usize, PdfError> {
    // Scan the last 2048 bytes for "startxref"
    let search_start = buf.len().saturating_sub(2048);
    let tail = &buf[search_start..];

    let needle = b"startxref";
    let mut pos = None;
    // Find the last occurrence
    for i in (0..tail.len().saturating_sub(needle.len())).rev() {
        if &tail[i..i + needle.len()] == needle {
            pos = Some(search_start + i);
            break;
        }
    }

    let pos = pos.ok_or_else(|| PdfError::Xref("startxref not found".into()))?;

    // Parse the offset number after "startxref"
    let after = pos + needle.len();
    let after = parser::skip_whitespace(buf, after);
    let (val, _) = parser::parse_value(buf, after)
        .map_err(|_| PdfError::Xref("could not parse startxref offset".into()))?;
    let offset = val
        .as_i64()
        .ok_or_else(|| PdfError::Xref("startxref offset is not an integer".into()))?;

    Ok(offset as usize)
}

/// Parse a single xref section at the given offset.
/// Returns (entries, trailer_dict). Works for both traditional tables and xref streams.
fn parse_xref_at(
    buf: &[u8],
    offset: usize,
) -> Result<XrefSection, PdfError> {
    let pos = parser::skip_whitespace(buf, offset);

    // Check if this is a traditional xref table or an xref stream
    if pos + 4 <= buf.len() && &buf[pos..pos + 4] == b"xref" {
        parse_traditional_xref(buf, pos)
    } else {
        // Some PDFs have /Prev pointing slightly past the "xref" keyword.
        // Scan backwards a few bytes to find it.
        let scan_start = pos.saturating_sub(16);
        for scan in (scan_start..pos).rev() {
            if scan + 4 <= buf.len() && &buf[scan..scan + 4] == b"xref" {
                return parse_traditional_xref(buf, scan);
            }
        }

        // Not a traditional xref — try parsing as an xref stream
        parse_xref_stream(buf, pos)
    }
}

/// Parse a traditional xref table + trailer.
fn parse_traditional_xref(
    buf: &[u8],
    pos: usize,
) -> Result<XrefSection, PdfError> {
    let mut entries = BTreeMap::new();
    let mut i = pos + 4; // skip "xref"
    i = parser::skip_whitespace(buf, i);

    // Parse subsections: each starts with "first_obj count"
    loop {
        i = parser::skip_whitespace(buf, i);
        if i >= buf.len() {
            break;
        }

        // Check if we've hit "trailer"
        if buf[i..].starts_with(b"trailer") {
            break;
        }

        // Parse subsection header: first_obj_num count
        let (first_val, end) = parser::parse_value(buf, i)
            .map_err(|_| PdfError::Xref("bad xref subsection header".into()))?;
        let first_obj = first_val.as_i64()
            .ok_or_else(|| PdfError::Xref("expected first object number".into()))? as u32;
        i = parser::skip_whitespace(buf, end);

        let (count_val, end) = parser::parse_value(buf, i)
            .map_err(|_| PdfError::Xref("bad xref subsection count".into()))?;
        let count = count_val.as_i64()
            .ok_or_else(|| PdfError::Xref("expected count".into()))? as u32;
        i = end;

        // Parse entries. Each entry is approximately 20 bytes: "offset gen n|f"
        for idx in 0..count {
            i = skip_to_entry(buf, i);
            if i >= buf.len() {
                break;
            }

            // Parse the 20-byte entry more leniently
            let entry_result = parse_xref_entry(buf, i);
            match entry_result {
                Ok((offset_val, _gen_val, in_use, end)) => {
                    let obj_num = first_obj + idx;
                    if in_use {
                        entries.insert(obj_num, XrefEntry::Normal { offset: offset_val });
                    } else {
                        entries.insert(obj_num, XrefEntry::Free);
                    }
                    i = end;
                }
                Err(_) => {
                    // Skip this entry and try to continue
                    i = skip_past_line(buf, i);
                }
            }
        }
    }

    // Parse trailer dict
    i = parser::skip_whitespace(buf, i);
    if !buf[i..].starts_with(b"trailer") {
        return Err(PdfError::Xref("expected 'trailer' keyword".into()));
    }
    i += 7; // skip "trailer"
    i = parser::skip_whitespace(buf, i);

    let (trailer_val, _) = parser::parse_value(buf, i)
        .map_err(|e| PdfError::Xref(format!("bad trailer dict: {e}")))?;
    let trailer_dict = match trailer_val {
        PdfVal::Dict(d) => d,
        _ => return Err(PdfError::Xref("trailer is not a dict".into())),
    };

    Ok((entries, trailer_dict))
}

/// Skip whitespace to get to the start of an xref entry.
fn skip_to_entry(buf: &[u8], mut pos: usize) -> usize {
    while pos < buf.len() && (buf[pos] == b' ' || buf[pos] == b'\r' || buf[pos] == b'\n' || buf[pos] == b'\t') {
        pos += 1;
    }
    pos
}

/// Skip past the current line.
fn skip_past_line(buf: &[u8], mut pos: usize) -> usize {
    while pos < buf.len() && buf[pos] != b'\n' && buf[pos] != b'\r' {
        pos += 1;
    }
    while pos < buf.len() && (buf[pos] == b'\n' || buf[pos] == b'\r') {
        pos += 1;
    }
    pos
}

/// Parse a single traditional xref entry: "0000000009 00000 n"
/// Returns (byte_offset, generation, is_in_use, end_position).
fn parse_xref_entry(buf: &[u8], pos: usize) -> Result<(usize, u16, bool, usize), PdfError> {
    // Parse offset (up to 10 digits)
    let mut i = pos;
    while i < buf.len() && buf[i].is_ascii_digit() {
        i += 1;
    }
    if i == pos {
        return Err(PdfError::Xref("expected offset digits".into()));
    }
    let offset: usize = std::str::from_utf8(&buf[pos..i])
        .map_err(|_| PdfError::Xref("invalid offset".into()))?
        .parse()
        .map_err(|_| PdfError::Xref("invalid offset number".into()))?;

    // Skip space(s)
    while i < buf.len() && buf[i] == b' ' {
        i += 1;
    }

    // Parse generation (up to 5 digits)
    let gen_start = i;
    while i < buf.len() && buf[i].is_ascii_digit() {
        i += 1;
    }
    let generation: u16 = std::str::from_utf8(&buf[gen_start..i])
        .map_err(|_| PdfError::Xref("invalid generation".into()))?
        .parse()
        .map_err(|_| PdfError::Xref("invalid generation number".into()))?;

    // Skip space(s)
    while i < buf.len() && buf[i] == b' ' {
        i += 1;
    }

    // 'n' or 'f'
    let in_use = if i < buf.len() {
        let c = buf[i];
        i += 1;
        c == b'n'
    } else {
        false
    };

    // Skip trailing whitespace (CR, LF, CRLF, space)
    while i < buf.len() && (buf[i] == b' ' || buf[i] == b'\r' || buf[i] == b'\n') {
        i += 1;
    }

    Ok((offset, generation, in_use, i))
}

/// Parse a cross-reference stream (PDF 1.5+).
fn parse_xref_stream(
    buf: &[u8],
    pos: usize,
) -> Result<XrefSection, PdfError> {
    // Parse the indirect object containing the xref stream
    let (_, _, val, _) = parser::parse_indirect_object(buf, pos)
        .map_err(|e| PdfError::Xref(format!("bad xref stream object: {e}")))?;

    let (dict, raw_data) = val.as_stream()
        .ok_or_else(|| PdfError::Xref("xref stream is not a stream".into()))?;

    // Verify /Type /XRef
    let is_xref = dict_get(dict, b"Type")
        .and_then(|v| v.as_name())
        .is_some_and(|n| n == b"XRef");
    if !is_xref {
        return Err(PdfError::Xref("stream is not /Type /XRef".into()));
    }

    // Get /W array (field widths)
    let w_array = dict_get(dict, b"W")
        .and_then(|v| v.as_array())
        .ok_or_else(|| PdfError::Xref("missing /W in xref stream".into()))?;
    if w_array.len() != 3 {
        return Err(PdfError::Xref("/W must have 3 elements".into()));
    }
    let w: [usize; 3] = [
        w_array[0].as_i64().unwrap_or(0) as usize,
        w_array[1].as_i64().unwrap_or(0) as usize,
        w_array[2].as_i64().unwrap_or(0) as usize,
    ];
    let entry_size = w[0] + w[1] + w[2];
    if entry_size == 0 {
        return Err(PdfError::Xref("/W sums to 0".into()));
    }

    // Get /Size
    let size = dict_get(dict, b"Size")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as u32;

    // Get /Index array (defaults to [0 Size])
    let index_ranges = if let Some(idx_arr) = dict_get(dict, b"Index").and_then(|v| v.as_array()) {
        let mut ranges = Vec::new();
        let mut j = 0;
        while j + 1 < idx_arr.len() {
            let first = idx_arr[j].as_i64().unwrap_or(0) as u32;
            let count = idx_arr[j + 1].as_i64().unwrap_or(0) as u32;
            ranges.push((first, count));
            j += 2;
        }
        ranges
    } else {
        vec![(0, size)]
    };

    // Decompress the stream data
    let filters = stream::get_filters(dict);
    let filter_refs: Vec<&[u8]> = filters.iter().map(|f| f.as_slice()).collect();
    let decode_parms = dict_get(dict, b"DecodeParms");
    let data = if filter_refs.is_empty() {
        raw_data.to_vec()
    } else {
        stream::decompress(raw_data, &filter_refs, decode_parms)
            .map_err(|e| PdfError::Xref(format!("xref stream decompress: {e}")))?
    };

    // Parse binary entries
    let mut entries = BTreeMap::new();
    let mut data_pos = 0;

    for (first_obj, count) in &index_ranges {
        for idx in 0..*count {
            if data_pos + entry_size > data.len() {
                break;
            }

            let field1 = read_field(&data, data_pos, w[0]);
            let field2 = read_field(&data, data_pos + w[0], w[1]);
            let field3 = read_field(&data, data_pos + w[0] + w[1], w[2]);
            data_pos += entry_size;

            // If w[0] == 0, type defaults to 1 (NOT 0!)
            let entry_type = if w[0] == 0 { 1 } else { field1 };

            let obj_num = first_obj + idx;
            let entry = match entry_type {
                0 => XrefEntry::Free,
                1 => XrefEntry::Normal { offset: field2 as usize },
                2 => XrefEntry::Compressed {
                    container: field2 as u32,
                    index: field3 as u32,
                },
                _ => XrefEntry::Free, // Unknown type — treat as free
            };

            entries.insert(obj_num, entry);
        }
    }

    Ok((entries, dict.clone()))
}

/// Read a big-endian unsigned integer from `width` bytes starting at `offset`.
fn read_field(data: &[u8], offset: usize, width: usize) -> u64 {
    if width == 0 {
        return 0;
    }
    let mut val: u64 = 0;
    for i in 0..width {
        if offset + i < data.len() {
            val = (val << 8) | data[offset + i] as u64;
        }
    }
    val
}

/// Extract an indirect reference from a trailer dict entry.
fn extract_ref(dict: &BTreeMap<Vec<u8>, PdfVal>, key: &[u8]) -> Option<(u32, u16)> {
    dict_get(dict, key)?.as_ref()
}

/// Brute-force scan: find all `N 0 obj` patterns in the file.
/// Used as a fallback when xref parsing fails completely.
fn brute_force_scan(buf: &[u8]) -> (BTreeMap<u32, XrefEntry>, Option<(u32, u16)>) {
    let mut entries = BTreeMap::new();
    let mut root_ref = None;

    let mut i = 0;
    while i < buf.len() {
        // Look for digit sequences followed by " 0 obj"
        if buf[i].is_ascii_digit() {
            let num_start = i;
            while i < buf.len() && buf[i].is_ascii_digit() {
                i += 1;
            }
            // Check for " 0 obj" (with possible whitespace variations)
            if i < buf.len() && buf[i] == b' ' {
                let after_space = i + 1;
                if after_space < buf.len() && buf[after_space] == b'0' {
                    let after_gen = after_space + 1;
                    if after_gen < buf.len() && buf[after_gen] == b' ' {
                        let obj_pos = after_gen + 1;
                        if obj_pos + 3 <= buf.len() && &buf[obj_pos..obj_pos + 3] == b"obj" {
                            // Check that 'obj' is followed by non-regular char
                            let after_obj = obj_pos + 3;
                            if after_obj >= buf.len() || !is_regular_char(buf[after_obj]) {
                                if let Ok(obj_num) = std::str::from_utf8(&buf[num_start..i])
                                    .unwrap_or("")
                                    .parse::<u32>()
                                {
                                    entries.insert(
                                        obj_num,
                                        XrefEntry::Normal { offset: num_start },
                                    );

                                    // Check if this object is the catalog (/Type /Catalog)
                                    if root_ref.is_none()
                                        && let Ok((_, _, val, _)) =
                                            parser::parse_indirect_object(buf, num_start)
                                        && is_catalog(&val)
                                    {
                                        root_ref = Some((obj_num, 0));
                                    }
                                }
                                i = after_obj;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }

    (entries, root_ref)
}

fn is_regular_char(b: u8) -> bool {
    !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0x00 |
              b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// Check if a PdfVal is a /Type /Catalog dict.
fn is_catalog(val: &PdfVal) -> bool {
    val.as_dict()
        .and_then(|d| dict_get(d, b"Type"))
        .and_then(|v| v.as_name())
        .is_some_and(|n| n == b"Catalog")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_traditional_xref() {
        let input = b"xref\n0 3\n0000000000 65535 f \n0000000009 00000 n \n0000000074 00000 n \ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n0\n%%EOF";
        let (entries, trailer) = parse_traditional_xref(input, 0).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[&0], XrefEntry::Free));
        assert!(matches!(entries[&1], XrefEntry::Normal { offset: 9 }));
        assert!(matches!(entries[&2], XrefEntry::Normal { offset: 74 }));
        assert!(dict_get(&trailer, b"Root").is_some());
    }

    #[test]
    fn test_find_startxref() {
        let input = b"%PDF-1.4\nsome content\nstartxref\n42\n%%EOF\n";
        let offset = find_startxref(input).unwrap();
        assert_eq!(offset, 42);
    }

    #[test]
    fn test_read_field() {
        assert_eq!(read_field(&[0x00, 0x01], 0, 2), 1);
        assert_eq!(read_field(&[0x01, 0x00], 0, 2), 256);
        assert_eq!(read_field(&[0xFF], 0, 1), 255);
        assert_eq!(read_field(&[], 0, 0), 0);
    }

    #[test]
    fn test_xref_stream_parsing() {
        // Build a minimal xref stream
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        // 3 entries, W=[1,2,1]: type(1 byte), offset/container(2 bytes), gen/index(1 byte)
        // Entry 0: free (type=0, next=0, gen=255)
        // Entry 1: normal at offset 256 (type=1, offset=0x0100, gen=0)
        // Entry 2: compressed in obj 5, index 0 (type=2, container=5, index=0)
        let raw_data = vec![
            0, 0, 0, 255, // entry 0: free
            1, 0x01, 0x00, 0, // entry 1: normal, offset=256
            2, 0, 5, 0,    // entry 2: compressed in obj 5, idx 0
        ];

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&raw_data).unwrap();
        let compressed = encoder.finish().unwrap();

        // Build the stream object
        let stream_obj = format!(
            "1 0 obj\n<< /Type /XRef /Size 3 /W [1 2 1] /Length {} /Filter /FlateDecode >>\nstream\n",
            compressed.len()
        );
        let mut buf = stream_obj.into_bytes();
        buf.extend_from_slice(&compressed);
        buf.extend_from_slice(b"\nendstream\nendobj");

        let (entries, _dict) = parse_xref_stream(&buf, 0).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[&0], XrefEntry::Free));
        assert!(matches!(entries[&1], XrefEntry::Normal { offset: 256 }));
        assert!(matches!(
            entries[&2],
            XrefEntry::Compressed {
                container: 5,
                index: 0
            }
        ));
    }

    #[test]
    fn test_xref_stream_default_type() {
        // When W[0]=0, type defaults to 1 (normal), NOT 0 (free)
        // W=[0,2,0]: no type field, 2-byte offset, no gen field
        let raw_data = vec![
            0x00, 0x09, // entry 0: offset 9
            0x00, 0x4A, // entry 1: offset 74
        ];

        let stream_obj = format!(
            "1 0 obj\n<< /Type /XRef /Size 2 /Index [1 2] /W [0 2 0] /Length {} >>\nstream\n",
            raw_data.len()
        );
        let mut buf = stream_obj.into_bytes();
        buf.extend_from_slice(&raw_data);
        buf.extend_from_slice(b"\nendstream\nendobj");

        let (entries, _) = parse_xref_stream(&buf, 0).unwrap();
        assert_eq!(entries.len(), 2);
        // Both should be Normal (type defaults to 1), not Free
        assert!(matches!(entries[&1], XrefEntry::Normal { offset: 9 }));
        assert!(matches!(entries[&2], XrefEntry::Normal { offset: 74 }));
    }

    #[test]
    fn test_brute_force_scan() {
        let input = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages >>\nendobj\n";
        let (entries, root) = brute_force_scan(input);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[&1], XrefEntry::Normal { .. }));
        assert!(matches!(entries[&2], XrefEntry::Normal { .. }));
        assert_eq!(root, Some((1, 0)));
    }

    #[test]
    fn test_xref_entry_parsing() {
        let entry = b"0000000009 00000 n \r\n";
        let (offset, generation, in_use, _) = parse_xref_entry(entry, 0).unwrap();
        assert_eq!(offset, 9);
        assert_eq!(generation, 0);
        assert!(in_use);

        let entry = b"0000000000 65535 f \r\n";
        let (offset, generation, in_use, _) = parse_xref_entry(entry, 0).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(generation, 65535);
        assert!(!in_use);
    }
}
