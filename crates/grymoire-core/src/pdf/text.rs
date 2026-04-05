//! Content stream interpreter for text extraction.
//!
//! Walks the PDF content stream operators (BT/ET, Tf, Tj, TJ, ', ", Do)
//! and decodes text using resolved font encodings. Handles Form XObject
//! recursion and ActualText marked content.

use std::collections::BTreeMap;

use super::document::PdfDoc;
use super::encoding::{self, FontEncoding};
use super::parser::{PdfVal, dict_get};

/// Extract text from all pages of a document.
pub(crate) fn extract_all_pages(doc: &PdfDoc) -> Vec<(i32, String)> {
    let pages = doc.pages();
    let mut result = Vec::new();

    for page in &pages {
        let text = extract_page_text(doc, page);
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            result.push((page.page_num, trimmed));
        }
    }

    result
}

/// Extract text from a single page.
fn extract_page_text(doc: &PdfDoc, page: &super::document::PageInfo) -> String {
    // Get content stream
    let content_data = match doc.page_content(page) {
        Ok(d) => d,
        Err(_) => return String::new(),
    };

    // Build font encodings from page resources
    let resources = doc.page_resources(page);
    let fonts = build_font_map(doc, &resources);

    // Interpret content stream
    let mut ctx = TextContext {
        doc,
        fonts: &fonts,
        text: String::new(),
        current_font: None,
        actual_text: None,
        actual_text_depth: 0,
    };

    interpret_content(&content_data, &mut ctx, &resources, 0);

    // Post-processing
    let text = encoding::expand_ligatures(&ctx.text);
    super::dehyphenate::dehyphenate(&text)
}

/// Extract the title from page 1 using font-size heuristics.
/// Returns the text rendered in the largest font on the first page.
/// This is the most reliable way to find the title of a PDF document.
#[allow(clippy::collapsible_if, clippy::unnecessary_to_owned)]
pub(crate) fn extract_title_by_font_size(doc: &PdfDoc) -> Option<String> {
    let pages = doc.pages();
    let page = pages.first()?;

    let content_data = doc.page_content(page).ok()?;
    let resources = doc.page_resources(page);
    let fonts = build_font_map(doc, &resources);

    let ops = parse_content_ops(&content_data)?;

    // Collect (font_size, text) spans
    let mut spans: Vec<(f64, String)> = Vec::new();
    let mut current_font: Option<&FontEncoding> = None;
    let mut current_size: f64 = 0.0;
    let mut current_text = String::new();

    for op in &ops {
        match op.operator.as_slice() {
            b"Tf" => {
                // Flush current span
                let trimmed = current_text.trim().to_string();
                if !trimmed.is_empty() && current_size > 0.0 {
                    spans.push((current_size, trimmed));
                }
                current_text.clear();

                if let Some(font_name) = op.operands.first().and_then(|v| v.as_name()) {
                    current_font = fonts.get(&font_name.to_vec());
                }
                if let Some(size) = op.operands.get(1).and_then(|v| v.as_f64()) {
                    current_size = size.abs();
                }
            }
            b"Tj" | b"'" | b"\"" => {
                for operand in &op.operands {
                    if let Some(bytes) = operand.as_str_bytes() {
                        if let Some(enc) = current_font {
                            current_text.push_str(&encoding::decode_text(enc, bytes));
                        }
                    }
                }
            }
            b"TJ" => {
                for operand in &op.operands {
                    if let PdfVal::Array(arr) = operand {
                        for item in arr {
                            if let Some(bytes) = item.as_str_bytes() {
                                if let Some(enc) = current_font {
                                    current_text.push_str(&encoding::decode_text(enc, bytes));
                                }
                            } else if let Some(n) = item.as_i64() {
                                if n < -100 {
                                    current_text.push(' ');
                                }
                            }
                        }
                    }
                }
            }
            b"ET" => {
                current_text.push(' ');
            }
            b"Do" => {
                // Recurse into Form XObjects for font-size extraction
                if let Some(xobj_name) = op.operands.first().and_then(|v| v.as_name()) {
                    if let Some(form_spans) = extract_form_xobject_spans(doc, &resources, xobj_name, &fonts) {
                        // Flush current span first
                        let trimmed = current_text.trim().to_string();
                        if !trimmed.is_empty() && current_size > 0.0 {
                            spans.push((current_size, trimmed));
                        }
                        current_text.clear();
                        spans.extend(form_spans);
                    }
                }
            }
            _ => {}
        }
    }

    // Flush final span
    let trimmed = current_text.trim().to_string();
    if !trimmed.is_empty() && current_size > 0.0 {
        spans.push((current_size, trimmed));
    }

    if spans.is_empty() {
        return None;
    }

    // Find the maximum font size
    let max_size = spans.iter().map(|(s, _)| *s).fold(0.0f64, f64::max);

    if max_size <= 0.0 {
        return None;
    }

    // Collect all text at the largest font size (may span multiple Tf blocks)
    let title_parts: Vec<&str> = spans
        .iter()
        .filter(|(size, _)| (*size - max_size).abs() < 0.5) // Within 0.5pt of max
        .map(|(_, text)| text.as_str())
        .collect();

    let title = title_parts.join(" ");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" "); // normalize whitespace
    let title = title.trim().to_string();

    if title.len() >= 5 && title.len() <= 300 && !is_publisher_junk(&title) {
        Some(title)
    } else {
        // Try second-largest font size
        let second_max = spans
            .iter()
            .map(|(s, _)| *s)
            .filter(|s| (s - max_size).abs() >= 0.5)
            .fold(0.0f64, f64::max);

        if second_max > 0.0 {
            let parts: Vec<&str> = spans
                .iter()
                .filter(|(size, _)| (*size - second_max).abs() < 0.5)
                .map(|(_, text)| text.as_str())
                .collect();
            let title2 = parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
            let title2 = title2.trim().to_string();
            if title2.len() >= 5 && title2.len() <= 300 && !is_publisher_junk(&title2) {
                return Some(title2);
            }
        }

        None
    }
}

/// Check if text looks like publisher/journal junk rather than an actual title.
fn is_publisher_junk(text: &str) -> bool {
    let l = text.to_lowercase();
    let junk = [
        "view article online",
        "author's accepted manuscript",
        "accepted manuscript",
        "published online",
        "downloaded from",
        "this article",
        "all rights reserved",
        "doi:",
        "issn",
        "copyright",
        "open access",
        "creative commons",
        "supplementary",
        "electronic supplementary",
        "lab on a chip",
        "labon",
    ];
    junk.iter().any(|j| l.contains(j))
}

/// Extract (font_size, text) spans from a Form XObject.
#[allow(clippy::collapsible_if, clippy::unnecessary_to_owned)]
fn extract_form_xobject_spans(
    doc: &PdfDoc,
    resources: &[&BTreeMap<Vec<u8>, PdfVal>],
    xobj_name: &[u8],
    page_fonts: &FontMap,
) -> Option<Vec<(f64, String)>> {
    let xobj_ref = resources.iter().find_map(|res| {
        let xobjects = dict_get(res, b"XObject")?;
        let xobj_dict = doc.resolve(xobjects).as_dict()?;
        Some(dict_get(xobj_dict, xobj_name)?.clone())
    })?;

    let xobj = doc.resolve(&xobj_ref);
    let dict = xobj.as_dict()?;

    let is_form = dict_get(dict, b"Subtype")
        .and_then(|v| v.as_name())
        .is_some_and(|n| n == b"Form");
    if !is_form {
        return None;
    }

    let form_data = doc.stream_data(xobj).ok()?;

    // Build fonts from form resources + page resources
    let form_res = dict_get(dict, b"Resources")
        .map(|v| doc.resolve(v))
        .and_then(|v| v.as_dict());
    let mut combined: Vec<&BTreeMap<Vec<u8>, PdfVal>> = Vec::new();
    if let Some(frd) = form_res {
        combined.push(frd);
    }
    combined.extend_from_slice(resources);
    let form_fonts = build_font_map(doc, &combined);

    let ops = parse_content_ops(&form_data)?;

    let mut spans = Vec::new();
    let mut current_font: Option<&FontEncoding> = None;
    let mut current_size: f64 = 0.0;
    let mut current_text = String::new();

    // Use form_fonts, falling back to page_fonts
    let get_font = |name: &[u8]| -> Option<&FontEncoding> {
        form_fonts.get(name).or_else(|| page_fonts.get(name))
    };

    for op in &ops {
        match op.operator.as_slice() {
            b"Tf" => {
                let trimmed = current_text.trim().to_string();
                if !trimmed.is_empty() && current_size > 0.0 {
                    spans.push((current_size, trimmed));
                }
                current_text.clear();

                if let Some(font_name) = op.operands.first().and_then(|v| v.as_name()) {
                    current_font = get_font(font_name);
                }
                if let Some(size) = op.operands.get(1).and_then(|v| v.as_f64()) {
                    current_size = size.abs();
                }
            }
            b"Tj" | b"'" | b"\"" => {
                for operand in &op.operands {
                    if let Some(bytes) = operand.as_str_bytes() {
                        if let Some(enc) = current_font {
                            current_text.push_str(&encoding::decode_text(enc, bytes));
                        }
                    }
                }
            }
            b"TJ" => {
                for operand in &op.operands {
                    if let PdfVal::Array(arr) = operand {
                        for item in arr {
                            if let Some(bytes) = item.as_str_bytes() {
                                if let Some(enc) = current_font {
                                    current_text.push_str(&encoding::decode_text(enc, bytes));
                                }
                            } else if let Some(n) = item.as_i64() {
                                if n < -100 {
                                    current_text.push(' ');
                                }
                            }
                        }
                    }
                }
            }
            b"ET" => {
                current_text.push(' ');
            }
            _ => {}
        }
    }

    let trimmed = current_text.trim().to_string();
    if !trimmed.is_empty() && current_size > 0.0 {
        spans.push((current_size, trimmed));
    }

    Some(spans)
}

/// Font map: font name -> encoding.
type FontMap = BTreeMap<Vec<u8>, FontEncoding>;

/// State for content stream interpretation.
struct TextContext<'a> {
    doc: &'a PdfDoc,
    fonts: &'a FontMap,
    text: String,
    current_font: Option<&'a FontEncoding>,
    /// If inside a BDC/EMC span with /ActualText, this holds the replacement text.
    actual_text: Option<String>,
    /// Nesting depth of the ActualText span (for tracking nested BMC/EMC).
    actual_text_depth: u32,
}

/// Build font name -> encoding map from page resources.
fn build_font_map(
    doc: &PdfDoc,
    resources: &[&BTreeMap<Vec<u8>, PdfVal>],
) -> FontMap {
    let mut fonts = FontMap::new();

    for res_dict in resources {
        let font_dict_val = match dict_get(res_dict, b"Font") {
            Some(v) => v,
            None => continue,
        };
        let font_dict = match doc.resolve(font_dict_val).as_dict() {
            Some(d) => d,
            None => continue,
        };

        for (name, val) in font_dict.iter() {
            if fonts.contains_key(name) {
                continue; // Earlier (higher priority) resource already defined this font
            }
            let font = match doc.resolve(val).as_dict() {
                Some(d) => d,
                None => continue,
            };
            let enc = encoding::resolve_font_encoding(doc, font);
            fonts.insert(name.clone(), enc);
        }
    }

    fonts
}

/// Interpret a content stream, extracting text.
fn interpret_content(
    data: &[u8],
    ctx: &mut TextContext,
    resources: &[&BTreeMap<Vec<u8>, PdfVal>],
    depth: u32,
) {
    if depth > 10 {
        return; // Prevent infinite recursion
    }

    let ops = match parse_content_ops(data) {
        Some(ops) => ops,
        None => return,
    };

    let mut marked_depth: u32 = 0; // Track BMC/BDC nesting

    for op in &ops {
        match op.operator.as_slice() {
            // Begin text object
            b"BT" => {}

            // End text object — add newline
            b"ET" => {
                if ctx.actual_text.is_none() && !ctx.text.ends_with('\n') {
                    ctx.text.push('\n');
                }
            }

            // Set font
            b"Tf" => {
                if let Some(font_name) = op.operands.first().and_then(|v| v.as_name()) {
                    ctx.current_font = ctx.fonts.get(&font_name.to_vec());
                }
            }

            // Show string
            b"Tj" | b"'" | b"\"" => {
                if ctx.actual_text.is_none() {
                    for operand in &op.operands {
                        if let Some(bytes) = operand.as_str_bytes() {
                            decode_and_append(ctx, bytes);
                        }
                    }
                }
            }

            // Show string with positioning
            b"TJ" => {
                if ctx.actual_text.is_none() {
                    for operand in &op.operands {
                        match operand {
                            PdfVal::Array(arr) => {
                                for item in arr {
                                    match item {
                                        PdfVal::Str(bytes) => {
                                            decode_and_append(ctx, bytes);
                                        }
                                        PdfVal::Int(n) => {
                                            if *n < -100 {
                                                ctx.text.push(' ');
                                            }
                                        }
                                        PdfVal::Real(f) => {
                                            if *f < -100.0 {
                                                ctx.text.push(' ');
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            PdfVal::Str(bytes) => {
                                decode_and_append(ctx, bytes);
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Text positioning — insert newline for vertical movement.
            // Any vertical offset > 0.5 indicates a new line. This can
            // split chart axis labels that use per-character positioning,
            // but catches all real line breaks needed for dehyphenation.
            b"Td" | b"TD" => {
                if let Some(ty) = op.operands.get(1).and_then(|v| v.as_f64())
                    && ty.abs() > 0.5
                    && !ctx.text.ends_with('\n')
                {
                    ctx.text.push('\n');
                }
            }

            b"T*" => {
                if !ctx.text.ends_with('\n') {
                    ctx.text.push('\n');
                }
            }

            b"Tm" => {}

            // Begin marked content (for ActualText)
            b"BDC" => {
                marked_depth += 1;
                // Check if this has /ActualText
                if ctx.actual_text.is_none()
                    && let Some(actual) = extract_actual_text(&op.operands, ctx.doc)
                {
                    ctx.actual_text = Some(actual);
                    ctx.actual_text_depth = marked_depth;
                }
            }

            b"BMC" => {
                marked_depth += 1;
            }

            // End marked content
            b"EMC" => {
                if ctx.actual_text.is_some() && marked_depth == ctx.actual_text_depth {
                    // End of ActualText span — emit the replacement text
                    if let Some(actual) = ctx.actual_text.take() {
                        ctx.text.push_str(&actual);
                    }
                    ctx.actual_text_depth = 0;
                }
                marked_depth = marked_depth.saturating_sub(1);
            }

            // Invoke XObject (Form XObject recursion)
            b"Do" => {
                if let Some(xobj_name) = op.operands.first().and_then(|v| v.as_name()) {
                    handle_do_operator(ctx, resources, xobj_name, depth);
                }
            }

            _ => {}
        }
    }
}

/// Decode bytes using the current font encoding and append to text.
fn decode_and_append(ctx: &mut TextContext, bytes: &[u8]) {
    if let Some(enc) = ctx.current_font {
        let decoded = encoding::decode_text(enc, bytes);
        ctx.text.push_str(&decoded);
    }
}

/// Handle the `Do` operator — invoke a Form XObject.
fn handle_do_operator(
    ctx: &mut TextContext,
    resources: &[&BTreeMap<Vec<u8>, PdfVal>],
    xobj_name: &[u8],
    depth: u32,
) {
    // Find the XObject in resources
    let xobj_ref = resources.iter().find_map(|res| {
        let xobjects = dict_get(res, b"XObject")?;
        let xobj_dict = ctx.doc.resolve(xobjects).as_dict()?;
        Some(dict_get(xobj_dict, xobj_name)?.clone())
    });

    let xobj_ref = match xobj_ref {
        Some(r) => r,
        None => return,
    };

    let xobj = ctx.doc.resolve(&xobj_ref);

    // Check it's a Form XObject
    let dict = match xobj.as_dict() {
        Some(d) => d,
        None => return,
    };

    let is_form = dict_get(dict, b"Subtype")
        .and_then(|v| v.as_name())
        .is_some_and(|n| n == b"Form");
    if !is_form {
        return;
    }

    // Get the Form XObject's content data
    let form_data = match ctx.doc.stream_data(xobj) {
        Ok(d) => d,
        Err(_) => return,
    };

    // Build font map from the Form XObject's own Resources
    let form_resources = dict_get(dict, b"Resources")
        .map(|v| ctx.doc.resolve(v));
    let form_res_dict = form_resources.and_then(|v| v.as_dict());

    // Combine form resources with inherited page resources
    let mut combined_resources: Vec<&BTreeMap<Vec<u8>, PdfVal>> = Vec::new();
    if let Some(frd) = form_res_dict {
        combined_resources.push(frd);
    }
    combined_resources.extend_from_slice(resources);

    let form_fonts = build_font_map(ctx.doc, &combined_resources);

    // Create a new context for the Form XObject (avoids lifetime issues)
    let mut form_ctx = TextContext {
        doc: ctx.doc,
        fonts: &form_fonts,
        text: String::new(),
        current_font: None,
        actual_text: None,
        actual_text_depth: 0,
    };

    interpret_content(&form_data, &mut form_ctx, &combined_resources, depth + 1);

    // Append form text to parent context
    if !form_ctx.text.is_empty() {
        if !ctx.text.is_empty() && !ctx.text.ends_with('\n') {
            ctx.text.push('\n');
        }
        ctx.text.push_str(&form_ctx.text);
    }
}

/// Extract /ActualText from BDC operands.
fn extract_actual_text(operands: &[PdfVal], doc: &PdfDoc) -> Option<String> {
    // BDC operands: /Tag <dict> or /Tag <name>
    // We want the dict with /ActualText
    for operand in operands {
        let dict = match operand {
            PdfVal::Dict(d) => d,
            PdfVal::Ref(_, _) => doc.resolve(operand).as_dict()?,
            _ => continue,
        };
        if let Some(val) = dict_get(dict, b"ActualText")
            && let Some(bytes) = val.as_str_bytes()
        {
            return Some(encoding::decode_pdf_string(bytes));
        }
    }
    None
}

/// A parsed content stream operation.
struct ContentOp {
    operator: Vec<u8>,
    operands: Vec<PdfVal>,
}

/// Parse a content stream into a sequence of operations.
/// This is a lenient parser that handles inline images (BI/ID/EI) by skipping them.
fn parse_content_ops(data: &[u8]) -> Option<Vec<ContentOp>> {
    let mut ops = Vec::new();
    let mut operand_stack: Vec<PdfVal> = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        pos = super::parser::skip_whitespace(data, pos);
        if pos >= data.len() {
            break;
        }

        let b = data[pos];

        // Try to parse as a value (number, string, name, array, dict)
        match b {
            b'(' | b'<' | b'[' | b'+' | b'-' | b'.' | b'0'..=b'9' | b'/' => {
                match super::parser::parse_value(data, pos) {
                    Ok((val, end)) => {
                        operand_stack.push(val);
                        pos = end;
                    }
                    Err(_) => {
                        pos += 1; // Skip unparseable byte
                    }
                }
            }
            b't' | b'f' | b'n' => {
                // Could be true/false/null or an operator
                if let Ok((val, end)) = super::parser::parse_value(data, pos) {
                    operand_stack.push(val);
                    pos = end;
                } else {
                    // It's an operator
                    let (op, end) = read_operator(data, pos);
                    let operands = std::mem::take(&mut operand_stack);
                    ops.push(ContentOp {
                        operator: op,
                        operands,
                    });
                    pos = end;
                }
            }
            _ => {
                // Must be an operator
                let (op, end) = read_operator(data, pos);

                // Handle inline images: BI ... ID <data> EI
                if op == b"BI" {
                    pos = skip_inline_image(data, end);
                    operand_stack.clear();
                    continue;
                }

                let operands = std::mem::take(&mut operand_stack);
                ops.push(ContentOp {
                    operator: op,
                    operands,
                });
                pos = end;
            }
        }
    }

    Some(ops)
}

/// Read an operator keyword (sequence of non-whitespace, non-delimiter bytes).
fn read_operator(data: &[u8], pos: usize) -> (Vec<u8>, usize) {
    let mut end = pos;
    while end < data.len()
        && !data[end].is_ascii_whitespace()
        && !matches!(data[end], b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'/' | b'%')
    {
        end += 1;
    }
    if end == pos {
        // Single byte operator or unexpected
        return (vec![data[pos]], pos + 1);
    }
    (data[pos..end].to_vec(), end)
}

/// Skip past an inline image (BI ... ID <data> EI).
fn skip_inline_image(data: &[u8], mut pos: usize) -> usize {
    // Find "ID" keyword (image data start)
    while pos + 1 < data.len() {
        if data[pos] == b'I' && data[pos + 1] == b'D' {
            pos += 2;
            // Skip the single whitespace after ID
            if pos < data.len() && (data[pos] == b' ' || data[pos] == b'\n') {
                pos += 1;
            }
            break;
        }
        pos += 1;
    }

    // Find "EI" keyword (preceded by whitespace)
    while pos + 2 < data.len() {
        if (data[pos] == b'\n' || data[pos] == b'\r' || data[pos] == b' ')
            && data[pos + 1] == b'E'
            && data[pos + 2] == b'I'
            && (pos + 3 >= data.len() || data[pos + 3].is_ascii_whitespace())
        {
            return pos + 3;
        }
        pos += 1;
    }

    data.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_ops() {
        let data = b"BT /F1 12 Tf (Hello) Tj ET";
        let ops = parse_content_ops(data).unwrap();

        assert_eq!(ops.len(), 4); // BT, Tf, Tj, ET
        assert_eq!(ops[0].operator, b"BT");
        assert_eq!(ops[1].operator, b"Tf");
        assert_eq!(ops[1].operands.len(), 2); // /F1 and 12
        assert_eq!(ops[2].operator, b"Tj");
        assert_eq!(ops[2].operands[0].as_str_bytes(), Some(b"Hello".as_slice()));
        assert_eq!(ops[3].operator, b"ET");
    }

    #[test]
    fn test_parse_tj_array() {
        let data = b"[( Hello ) -100 ( World )] TJ";
        let ops = parse_content_ops(data).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operator, b"TJ");
        let arr = ops[0].operands[0].as_array().unwrap();
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn test_skip_inline_image() {
        let data = b"q BI /W 10 /H 10 /CS /G /BPC 8 ID\x00\x01\x02\x03\nEI Q";
        let ops = parse_content_ops(data).unwrap();
        // Should have q and Q, with the inline image skipped
        let op_names: Vec<_> = ops.iter().map(|op| op.operator.clone()).collect();
        assert!(op_names.contains(&b"q".to_vec()));
        assert!(op_names.contains(&b"Q".to_vec()));
    }

    // Local-file tests removed — PDF parser is tested hermetically via
    // minimal_pdf() in document.rs and synthetic content streams above.
    // TODO: Check in small test PDFs to crates/grymoire-core/testdata/ for
    // real-world regression testing without hardcoded paths.
}
