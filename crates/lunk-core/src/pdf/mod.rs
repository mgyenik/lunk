//! PDF text extraction module.
//!
//! A focused, lenient PDF parser for extracting searchable text from PDFs.
//! Not a general-purpose PDF library — only supports reading, and only
//! extracts text content and metadata.

#[allow(dead_code)]
pub(crate) mod parser;
#[allow(dead_code)]
pub(crate) mod stream;
#[allow(dead_code)]
pub(crate) mod xref;
#[allow(dead_code)]
pub(crate) mod document;
#[allow(dead_code)]
pub(crate) mod encodings_data;
#[allow(dead_code)]
pub(crate) mod glyphlist;
#[allow(dead_code)]
pub(crate) mod cmap;
#[allow(dead_code)]
pub(crate) mod encoding;
pub(crate) mod dehyphenate;
pub(crate) mod text;

/// Current version of the processing/indexing pipeline.
/// Bump this when the extraction logic changes materially.
pub const INDEX_VERSION: i32 = 3;

/// Errors from PDF parsing and extraction.
#[derive(Debug)]
pub(crate) enum PdfError {
    Parse(String),
    Xref(String),
    Decompress(String),
    #[allow(dead_code)]
    Encrypted,
    NoRoot,
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfError::Parse(s) => write!(f, "parse error: {s}"),
            PdfError::Xref(s) => write!(f, "xref error: {s}"),
            PdfError::Decompress(s) => write!(f, "decompress error: {s}"),
            PdfError::Encrypted => write!(f, "encrypted PDF (not supported)"),
            PdfError::NoRoot => write!(f, "no document root found"),
        }
    }
}

// -- Public API (unchanged from the old pdf.rs) --

/// Extract text from a PDF byte slice, returning (page_number, text) pairs.
/// Page numbers are 1-indexed. Pages with no extractable text are omitted.
pub fn extract_pages(data: &[u8]) -> Vec<(i32, String)> {
    let doc = match document::PdfDoc::load(data) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("failed to load PDF: {e}");
            return Vec::new();
        }
    };
    text::extract_all_pages(&doc)
}

/// Extract the title from PDF metadata (Info dictionary), if present.
pub fn extract_title(data: &[u8]) -> Option<String> {
    let doc = document::PdfDoc::load(data).ok()?;
    let info_ref = doc.info_ref?;
    let info = doc.get(info_ref)?;
    let info_dict = info.as_dict()?;
    let title_val = parser::dict_get(info_dict, b"Title")?;
    let bytes = title_val.as_str_bytes()?;
    let title = encoding::decode_pdf_string(bytes);
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
