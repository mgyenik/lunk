use lopdf::Document;

/// Current version of the processing/indexing pipeline.
/// Bump this when the extraction logic changes materially (new library,
/// new fields extracted, OCR added, etc.). Entries with index_version
/// below this value are candidates for reindexing.
pub const INDEX_VERSION: i32 = 1;

/// Extract the title from PDF metadata (Info dictionary), if present.
pub fn extract_title(data: &[u8]) -> Option<String> {
    let doc = Document::load_mem(data).ok()?;
    let info_id = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    let info = doc.get_dictionary(info_id).ok()?;
    let title_obj = info.get(b"Title").ok()?;
    let title = match title_obj {
        lopdf::Object::String(bytes, _) => String::from_utf8_lossy(bytes).to_string(),
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

    for &page_num in page_ids.keys() {
        match doc.extract_text(&[page_num]) {
            Ok(text) => {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    pages.push((page_num as i32, trimmed));
                }
            }
            Err(e) => {
                tracing::warn!("failed to extract text from page {page_num}: {e}");
            }
        }
    }

    // lopdf's get_pages() returns a BTreeMap, so pages are already sorted by number
    pages
}
