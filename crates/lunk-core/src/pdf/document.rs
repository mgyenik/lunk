//! PDF document model: object store, reference resolution, page tree traversal.

use std::collections::BTreeMap;

use super::parser::{self, PdfVal, dict_get};
use super::stream;
use super::xref::{self, XrefEntry};
use super::PdfError;

/// A parsed PDF document — an object store with metadata.
pub(crate) struct PdfDoc {
    /// Raw file buffer (kept for loading objects by offset).
    buf: Vec<u8>,
    /// All loaded objects, keyed by (object_number, generation).
    pub objects: BTreeMap<(u32, u16), PdfVal>,
    /// Reference to the document catalog (/Root).
    pub root_ref: Option<(u32, u16)>,
    /// Reference to the Info dictionary.
    pub info_ref: Option<(u32, u16)>,
}

/// Info about a single page, collected during page tree traversal.
pub(crate) struct PageInfo {
    /// 1-based page number.
    pub page_num: i32,
    /// Object ID of the page.
    pub page_ref: (u32, u16),
    /// Resource dict references inherited from parent Pages nodes.
    /// Ordered from nearest parent to farthest. The page's own Resources
    /// (if any) should be checked first, then these.
    pub inherited_resources: Vec<(u32, u16)>,
}

impl PdfDoc {
    /// Parse a PDF from raw bytes.
    pub fn load(data: &[u8]) -> Result<Self, PdfError> {
        let xref_result = xref::parse_xref(data)?;

        let mut doc = PdfDoc {
            buf: data.to_vec(),
            objects: BTreeMap::new(),
            root_ref: xref_result.root_ref,
            info_ref: xref_result.info_ref,
        };

        // Load all normal (uncompressed) objects first
        for (&obj_num, entry) in &xref_result.entries {
            if let XrefEntry::Normal { offset } = entry
                && *offset < data.len()
            {
                match parser::parse_indirect_object(data, *offset) {
                    Ok((parsed_num, generation, val, _)) => {
                        let _ = parsed_num;
                        doc.objects.insert((obj_num, generation), val);
                    }
                    Err(e) => {
                        tracing::debug!(obj_num, offset, "failed to parse object: {e}");
                    }
                }
            }
        }

        // Now unpack object streams for compressed entries
        let compressed: Vec<(u32, u32, u32)> = xref_result
            .entries
            .iter()
            .filter_map(|(&obj_num, entry)| {
                if let XrefEntry::Compressed { container, index } = entry {
                    Some((obj_num, *container, *index))
                } else {
                    None
                }
            })
            .collect();

        // Group compressed entries by container
        let mut by_container: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
        for (obj_num, container, index) in compressed {
            by_container
                .entry(container)
                .or_default()
                .push((obj_num, index));
        }

        for (container_num, entries) in by_container {
            if let Err(e) = doc.unpack_object_stream(container_num, &entries) {
                tracing::debug!(container_num, "failed to unpack object stream: {e}");
            }
        }

        Ok(doc)
    }

    /// Unpack objects from an object stream (ObjStm).
    fn unpack_object_stream(
        &mut self,
        container_num: u32,
        entries: &[(u32, u32)], // (obj_num, index_in_stream)
    ) -> Result<(), PdfError> {
        // Get the container stream
        let container = self
            .objects
            .get(&(container_num, 0))
            .ok_or_else(|| PdfError::Parse(format!("object stream {container_num} not found")))?
            .clone();

        let (dict, raw_data) = container
            .as_stream()
            .ok_or_else(|| PdfError::Parse("object stream is not a stream".into()))?;

        // Verify type
        let is_objstm = dict_get(dict, b"Type")
            .and_then(|v| v.as_name())
            .is_some_and(|n| n == b"ObjStm");
        if !is_objstm {
            return Err(PdfError::Parse("container is not /Type /ObjStm".into()));
        }

        let n = dict_get(dict, b"N")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| PdfError::Parse("ObjStm missing /N".into()))? as usize;

        let first = dict_get(dict, b"First")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| PdfError::Parse("ObjStm missing /First".into()))? as usize;

        // Decompress the stream
        let filters = stream::get_filters(dict);
        let filter_refs: Vec<&[u8]> = filters.iter().map(|f| f.as_slice()).collect();
        let decode_parms = dict_get(dict, b"DecodeParms");
        let data = if filter_refs.is_empty() {
            raw_data.to_vec()
        } else {
            stream::decompress(raw_data, &filter_refs, decode_parms)?
        };

        // Parse header: N pairs of (obj_num, offset)
        let mut header = Vec::new();
        let mut pos = 0;
        for _ in 0..n {
            pos = parser::skip_whitespace(&data, pos);
            let (num_val, end) = parser::parse_value(&data, pos)
                .map_err(|e| PdfError::Parse(format!("ObjStm header: {e}")))?;
            let obj_num = num_val.as_i64().unwrap_or(0) as u32;
            pos = parser::skip_whitespace(&data, end);
            let (off_val, end) = parser::parse_value(&data, pos)
                .map_err(|e| PdfError::Parse(format!("ObjStm header offset: {e}")))?;
            let offset = off_val.as_i64().unwrap_or(0) as usize;
            pos = end;
            header.push((obj_num, offset));
        }

        // Parse each requested object
        for &(obj_num, index) in entries {
            let idx = index as usize;
            if idx >= header.len() {
                continue;
            }
            let (_, obj_offset) = header[idx];
            let abs_offset = first + obj_offset;
            if abs_offset >= data.len() {
                continue;
            }

            match parser::parse_value(&data, abs_offset) {
                Ok((val, _)) => {
                    self.objects.insert((obj_num, 0), val);
                }
                Err(e) => {
                    tracing::debug!(obj_num, index, "failed to parse object in ObjStm: {e}");
                }
            }
        }

        Ok(())
    }

    /// Resolve a value: if it's a Ref, follow the chain (up to 10 levels).
    pub fn resolve<'a>(&'a self, val: &'a PdfVal) -> &'a PdfVal {
        let mut current = val;
        for _ in 0..10 {
            match current.as_ref() {
                Some(id) => match self.objects.get(&id) {
                    Some(obj) => current = obj,
                    None => return current,
                },
                None => return current,
            }
        }
        current
    }

    /// Get an object by ID, returning None if not found.
    pub fn get(&self, id: (u32, u16)) -> Option<&PdfVal> {
        self.objects.get(&id)
    }

    /// Resolve a dict entry: get the value and follow any indirect reference.
    pub fn resolve_dict_val<'a>(
        &'a self,
        dict: &'a BTreeMap<Vec<u8>, PdfVal>,
        key: &[u8],
    ) -> Option<&'a PdfVal> {
        let val = dict_get(dict, key)?;
        Some(self.resolve(val))
    }

    /// Get decompressed stream data for a stream object.
    pub fn stream_data(&self, val: &PdfVal) -> Result<Vec<u8>, PdfError> {
        let val = self.resolve(val);
        let (dict, raw) = val
            .as_stream()
            .ok_or_else(|| PdfError::Parse("expected stream".into()))?;

        let filters = stream::get_filters(dict);
        if filters.is_empty() {
            return Ok(raw.to_vec());
        }
        let filter_refs: Vec<&[u8]> = filters.iter().map(|f| f.as_slice()).collect();
        let decode_parms = dict_get(dict, b"DecodeParms");
        stream::decompress(raw, &filter_refs, decode_parms)
    }

    /// Walk the page tree and return info about each page in order.
    pub fn pages(&self) -> Vec<PageInfo> {
        let root_ref = match self.root_ref {
            Some(r) => r,
            None => return Vec::new(),
        };

        let catalog = match self.get(root_ref) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let pages_val = match catalog.as_dict().and_then(|d| dict_get(d, b"Pages")) {
            Some(v) => v,
            None => return Vec::new(),
        };

        let pages_ref = match self.resolve(pages_val).as_dict() {
            Some(_) => pages_val.as_ref().unwrap_or(root_ref),
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        let mut page_num = 1i32;
        self.collect_pages(pages_ref, &mut Vec::new(), &mut page_num, &mut result);
        result
    }

    /// Recursively collect pages from the page tree.
    fn collect_pages(
        &self,
        node_ref: (u32, u16),
        inherited_resources: &mut Vec<(u32, u16)>,
        page_num: &mut i32,
        result: &mut Vec<PageInfo>,
    ) {
        let node = match self.get(node_ref) {
            Some(n) => n,
            None => return,
        };

        let dict = match node.as_dict() {
            Some(d) => d,
            None => return,
        };

        // Check if this node has Resources — if so, track for inheritance
        let has_resources = dict_get(dict, b"Resources").is_some();
        if has_resources {
            inherited_resources.push(node_ref);
        }

        let node_type = dict_get(dict, b"Type").and_then(|v| v.as_name());
        match node_type {
            Some(b"Pages") => {
                // Intermediate node — recurse into /Kids
                if let Some(kids) = dict_get(dict, b"Kids").and_then(|v| v.as_array()) {
                    for kid in kids {
                        if let Some(kid_ref) = self.resolve(kid).as_dict().and_then(|_| kid.as_ref()) {
                            self.collect_pages(kid_ref, inherited_resources, page_num, result);
                        } else if let Some(kid_ref) = kid.as_ref() {
                            self.collect_pages(kid_ref, inherited_resources, page_num, result);
                        }
                    }
                }
            }
            Some(b"Page") | None => {
                // Leaf page node (Some PDFs omit /Type on pages)
                result.push(PageInfo {
                    page_num: *page_num,
                    page_ref: node_ref,
                    inherited_resources: inherited_resources.clone(),
                });
                *page_num += 1;
            }
            _ => {}
        }

        if has_resources {
            inherited_resources.pop();
        }
    }

    /// Get all resource dicts for a page (the page's own + inherited).
    /// Returns them in priority order: page's own first, then parents.
    pub fn page_resources(&self, page: &PageInfo) -> Vec<&BTreeMap<Vec<u8>, PdfVal>> {
        let mut resources = Vec::new();

        // Page's own resources
        if let Some(page_dict) = self.get(page.page_ref).and_then(|v| v.as_dict())
            && let Some(res_val) = dict_get(page_dict, b"Resources")
            && let Some(d) = self.resolve(res_val).as_dict()
        {
            resources.push(d);
        }

        // Inherited resources (from parent Pages nodes)
        for &res_ref in page.inherited_resources.iter().rev() {
            if let Some(node_dict) = self.get(res_ref).and_then(|v| v.as_dict())
                && let Some(res_val) = dict_get(node_dict, b"Resources")
                && let Some(d) = self.resolve(res_val).as_dict()
            {
                resources.push(d);
            }
        }

        resources
    }

    /// Get the concatenated content stream data for a page.
    pub fn page_content(&self, page: &PageInfo) -> Result<Vec<u8>, PdfError> {
        let page_dict = self
            .get(page.page_ref)
            .and_then(|v| v.as_dict())
            .ok_or_else(|| PdfError::Parse("page object is not a dict".into()))?;

        let contents = match dict_get(page_dict, b"Contents") {
            Some(c) => c,
            None => return Ok(Vec::new()), // Page with no content
        };

        match contents {
            PdfVal::Ref(n, g) => {
                let obj = self
                    .get((*n, *g))
                    .ok_or_else(|| PdfError::Parse(format!("content ref {n} {g} not found")))?;
                self.stream_data(obj)
            }
            PdfVal::Array(refs) => {
                let mut combined = Vec::new();
                for r in refs {
                    let resolved = self.resolve(r);
                    match self.stream_data(resolved) {
                        Ok(data) => {
                            if !combined.is_empty() {
                                combined.push(b'\n');
                            }
                            combined.extend_from_slice(&data);
                        }
                        Err(e) => {
                            tracing::debug!("skipping content stream: {e}");
                        }
                    }
                }
                Ok(combined)
            }
            PdfVal::Stream { .. } => self.stream_data(contents),
            _ => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid PDF in memory for testing.
    fn minimal_pdf() -> Vec<u8> {
        // A minimal PDF with 1 page containing "Hello"
        let mut buf = Vec::new();
        buf.extend_from_slice(b"%PDF-1.4\n");

        // Object 1: Catalog
        let obj1_offset = buf.len();
        buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

        // Object 2: Pages
        let obj2_offset = buf.len();
        buf.extend_from_slice(
            b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        );

        // Object 3: Page
        let obj3_offset = buf.len();
        buf.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
        );

        // Object 4: Content stream (uncompressed)
        let content = b"BT /F1 12 Tf (Hello) Tj ET";
        let obj4_offset = buf.len();
        let obj4 = format!(
            "4 0 obj\n<< /Length {} >>\nstream\n",
            content.len()
        );
        buf.extend_from_slice(obj4.as_bytes());
        buf.extend_from_slice(content);
        buf.extend_from_slice(b"\nendstream\nendobj\n");

        // Object 5: Font
        let obj5_offset = buf.len();
        buf.extend_from_slice(
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n",
        );

        // xref table
        let xref_offset = buf.len();
        buf.extend_from_slice(b"xref\n");
        buf.extend_from_slice(b"0 6\n");
        buf.extend_from_slice(b"0000000000 65535 f \n");
        buf.extend_from_slice(format!("{:010} 00000 n \n", obj1_offset).as_bytes());
        buf.extend_from_slice(format!("{:010} 00000 n \n", obj2_offset).as_bytes());
        buf.extend_from_slice(format!("{:010} 00000 n \n", obj3_offset).as_bytes());
        buf.extend_from_slice(format!("{:010} 00000 n \n", obj4_offset).as_bytes());
        buf.extend_from_slice(format!("{:010} 00000 n \n", obj5_offset).as_bytes());

        // trailer
        buf.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n");
        buf.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF\n").as_bytes());

        buf
    }

    #[test]
    fn test_load_minimal_pdf() {
        let data = minimal_pdf();
        let doc = PdfDoc::load(&data).unwrap();
        assert!(doc.root_ref.is_some());
        assert_eq!(doc.objects.len(), 5); // objects 1-5 (object 0 is free)
    }

    #[test]
    fn test_page_tree() {
        let data = minimal_pdf();
        let doc = PdfDoc::load(&data).unwrap();
        let pages = doc.pages();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_num, 1);
    }

    #[test]
    fn test_page_content() {
        let data = minimal_pdf();
        let doc = PdfDoc::load(&data).unwrap();
        let pages = doc.pages();
        let content = doc.page_content(&pages[0]).unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("Hello"));
    }

    #[test]
    fn test_resolve_ref() {
        let data = minimal_pdf();
        let doc = PdfDoc::load(&data).unwrap();

        // Resolve a reference to the catalog
        let ref_val = PdfVal::Ref(1, 0);
        let resolved = doc.resolve(&ref_val);
        assert!(resolved.as_dict().is_some());
        let dict = resolved.as_dict().unwrap();
        assert_eq!(
            dict_get(dict, b"Type").unwrap().as_name(),
            Some(b"Catalog".as_slice())
        );
    }

    #[test]
    fn test_page_resources() {
        let data = minimal_pdf();
        let doc = PdfDoc::load(&data).unwrap();
        let pages = doc.pages();
        let resources = doc.page_resources(&pages[0]);
        assert!(!resources.is_empty());
        // Should have a Font entry
        assert!(dict_get(resources[0], b"Font").is_some());
    }

    #[test]
    #[ignore] // Uses local file — not hermetic. Run with --ignored.
    fn test_load_real_pdf() {
        let path = std::path::Path::new(
            "/home/m/Downloads/Second_Order_Digital_Filters_Done_Right.pdf",
        );
        if !path.exists() {
            return; // Skip if file not available
        }
        let data = std::fs::read(path).unwrap();
        let doc = PdfDoc::load(&data);
        assert!(doc.is_ok(), "should handle broken-trailer PDF");
        let doc = doc.unwrap();
        let pages = doc.pages();
        assert_eq!(pages.len(), 10, "should find all 10 pages");
    }

    #[test]
    #[ignore] // Uses local file — not hermetic. Run with --ignored.
    fn test_load_form_xobject_pdf() {
        let path = std::path::Path::new("/home/m/Downloads/MP6002.pdf");
        if !path.exists() {
            return;
        }
        let data = std::fs::read(path).unwrap();
        let doc = PdfDoc::load(&data);
        assert!(doc.is_ok());
        let doc = doc.unwrap();
        let pages = doc.pages();
        assert_eq!(pages.len(), 15);
    }

    // --- Error path tests ---

    #[test]
    fn test_load_empty_file() {
        let result = PdfDoc::load(b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_not_pdf() {
        let result = PdfDoc::load(b"<html><body>not a pdf</body></html>");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_truncated_pdf() {
        let result = PdfDoc::load(b"%PDF-1.4\n%truncated");
        assert!(result.is_err());
    }
}
