use lopdf::Document;

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
