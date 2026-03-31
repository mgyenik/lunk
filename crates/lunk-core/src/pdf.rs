use std::collections::BTreeMap;

use lopdf::content::Content;
use lopdf::{Document, Encoding, Object, ObjectId};

/// Current version of the processing/indexing pipeline.
/// Bump this when the extraction logic changes materially (new library,
/// new fields extracted, OCR added, etc.). Entries with index_version
/// below this value are candidates for reindexing.
pub const INDEX_VERSION: i32 = 2;

/// Extract the title from PDF metadata (Info dictionary), if present.
pub fn extract_title(data: &[u8]) -> Option<String> {
    let doc = Document::load_mem(data).ok()?;
    let info_id = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    let info = doc.get_dictionary(info_id).ok()?;
    let title_obj = info.get(b"Title").ok()?;
    let title = match title_obj {
        Object::String(bytes, _) => String::from_utf8_lossy(bytes).to_string(),
        _ => return None,
    };
    let trimmed = title.trim().to_string();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("untitled") {
        None
    } else {
        Some(trimmed)
    }
}

/// Check if a title looks generic/useless and should be replaced.
pub fn is_generic_title(title: &str) -> bool {
    let t = title.trim().to_lowercase();
    matches!(
        t.as_str(),
        "" | "pdf" | "untitled" | "untitled pdf" | "document" | "download"
    ) || t.len() <= 3
}

/// Extract text from a PDF byte slice, returning (page_number, text) pairs.
/// Page numbers are 1-indexed. Pages with no extractable text are omitted.
///
/// Tries lopdf's built-in extraction first. If a page yields no text,
/// falls back to extracting from Form XObjects referenced via `Do` operators.
pub fn extract_pages(data: &[u8]) -> Vec<(i32, String)> {
    let doc = match Document::load_mem(data) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("failed to load PDF: {e}");
            return Vec::new();
        }
    };

    let page_ids = doc.get_pages();
    let mut pages = Vec::new();

    for (&page_num, &page_id) in &page_ids {
        // Try lopdf's built-in extraction first
        let text = doc
            .extract_text(&[page_num])
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());

        let text = text.unwrap_or_else(|| extract_text_with_xobjects(&doc, page_id));

        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            pages.push((page_num as i32, trimmed));
        }
    }

    // lopdf's get_pages() returns a BTreeMap, so pages are already sorted by number
    pages
}

/// Extract text from a page by recursing into Form XObjects.
///
/// Many PDFs (especially those from CAD/publishing tools) wrap all page
/// content inside Form XObjects. The page's own content stream is just
/// a `Do` operator invoking the form. lopdf's `extract_text` doesn't
/// follow these references, so we handle it here.
fn extract_text_with_xobjects(doc: &Document, page_id: ObjectId) -> String {
    let mut full_text = String::new();

    // Get the page's content stream to find Do operators
    let content_data = match doc.get_page_content(page_id) {
        Ok(d) => d,
        Err(_) => return full_text,
    };
    let content = match Content::decode(&content_data) {
        Ok(c) => c,
        Err(_) => return full_text,
    };

    // Collect XObject names referenced by Do operators
    let xobject_names: Vec<Vec<u8>> = content
        .operations
        .iter()
        .filter(|op| op.operator == "Do")
        .filter_map(|op| op.operands.first().and_then(|o| o.as_name().ok()))
        .map(|n| n.to_vec())
        .collect();

    if xobject_names.is_empty() {
        return full_text;
    }

    // Get the XObject dictionary from page resources
    let xobject_dict = get_page_xobjects(doc, page_id);
    let xobject_dict = match xobject_dict {
        Some(d) => d,
        None => return full_text,
    };

    for name in &xobject_names {
        let obj_ref = match xobject_dict.get(name).and_then(Object::as_reference) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let stream = match doc.get_object(obj_ref).and_then(Object::as_stream) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Only process Form XObjects
        let is_form = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|s| s == b"Form");
        if !is_form {
            continue;
        }

        if let Ok(text) = extract_text_from_stream(doc, &stream.dict, stream)
            && !text.is_empty()
        {
            if !full_text.is_empty() {
                full_text.push('\n');
            }
            full_text.push_str(&text);
        }
    }

    full_text
}

/// Get the XObject dictionary from a page's resources (following references).
fn get_page_xobjects(doc: &Document, page_id: ObjectId) -> Option<&lopdf::Dictionary> {
    let (res_dict, res_ids) = doc.get_page_resources(page_id).ok()?;

    // Check inline resource dict first
    if let Some(res) = res_dict
        && let Some(d) = resolve_dict_entry(doc, res, b"XObject")
    {
        return Some(d);
    }

    // Check referenced resource dicts
    for res_id in res_ids {
        if let Ok(res) = doc.get_dictionary(res_id)
            && let Some(d) = resolve_dict_entry(doc, res, b"XObject")
        {
            return Some(d);
        }
    }

    None
}

/// Look up a key in a dictionary, following indirect references to get a Dictionary.
fn resolve_dict_entry<'a>(
    doc: &'a Document,
    dict: &'a lopdf::Dictionary,
    key: &[u8],
) -> Option<&'a lopdf::Dictionary> {
    match dict.get(key).ok()? {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// Extract text from a Form XObject's content stream using its own Resources.
fn extract_text_from_stream(
    doc: &Document,
    stream_dict: &lopdf::Dictionary,
    stream: &lopdf::Stream,
) -> lopdf::Result<String> {
    // Build font encodings from the Form XObject's own Resources
    let encodings = build_encodings_from_resources(doc, stream_dict);

    // Decode the form's content stream
    let content_data = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let content = Content::decode(&content_data)?;

    // Walk operators, same logic as lopdf's extract_text but with our encodings
    let mut text = String::new();
    let mut current_encoding: Option<&Encoding> = None;

    for op in &content.operations {
        match op.operator.as_ref() {
            "Tf" => {
                if let Some(font_name) = op.operands.first().and_then(|o| o.as_name().ok()) {
                    current_encoding = encodings.get(font_name);
                }
            }
            "Tj" | "TJ" => {
                if let Some(encoding) = current_encoding {
                    collect_text(&mut text, encoding, &op.operands);
                }
            }
            "ET" => {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            // Recurse into nested Form XObjects
            "Do" => {
                if let Some(name) = op.operands.first().and_then(|o| o.as_name().ok())
                    && let Some(nested) = extract_nested_xobject_text(doc, stream_dict, name)
                    && !nested.is_empty()
                {
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                    text.push_str(&nested);
                }
            }
            _ => {}
        }
    }

    Ok(text)
}

/// Build a map of font name → Encoding from a stream/page's Resources dict.
fn build_encodings_from_resources<'a>(
    doc: &'a Document,
    container_dict: &'a lopdf::Dictionary,
) -> BTreeMap<Vec<u8>, Encoding<'a>> {
    let mut encodings = BTreeMap::new();

    let res = match container_dict.get(b"Resources") {
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let res = match res {
        Some(r) => r,
        None => return encodings,
    };

    let font_dict = match res.get(b"Font") {
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let font_dict = match font_dict {
        Some(d) => d,
        None => return encodings,
    };

    for (name, value) in font_dict.iter() {
        let font = match value {
            Object::Reference(id) => doc.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        };
        if let Some(font) = font
            && let Ok(enc) = font.get_font_encoding(doc)
        {
            encodings.insert(name.clone(), enc);
        }
    }

    encodings
}

/// Collect text from Tj/TJ operands using the given encoding.
fn collect_text(text: &mut String, encoding: &Encoding, operands: &[Object]) {
    for operand in operands {
        match operand {
            Object::String(bytes, _) => {
                if let Ok(decoded) = Document::decode_text(encoding, bytes) {
                    text.push_str(&decoded);
                }
            }
            Object::Array(arr) => {
                collect_text(text, encoding, arr);
                text.push(' ');
            }
            Object::Integer(i) => {
                if *i < -100 {
                    text.push(' ');
                }
            }
            _ => {}
        }
    }
}

/// Try to extract text from a nested Form XObject referenced by name.
fn extract_nested_xobject_text(
    doc: &Document,
    parent_dict: &lopdf::Dictionary,
    xobject_name: &[u8],
) -> Option<String> {
    let res = match parent_dict.get(b"Resources") {
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    }?;

    let xobjects = match res.get(b"XObject") {
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    }?;

    let obj_ref = xobjects.get(xobject_name).ok()?.as_reference().ok()?;
    let stream = doc.get_object(obj_ref).ok()?.as_stream().ok()?;

    let is_form = stream
        .dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_ok_and(|s| s == b"Form");
    if !is_form {
        return None;
    }

    extract_text_from_stream(doc, &stream.dict, stream).ok()
}
