use crate::types::{DocumentMeta, PageData};
use anyhow::{bail, Context, Result};
use lopdf::content::Content;
use lopdf::{Document, Encoding, Object, ObjectId};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The Loader component responsible for opening PDF files and extracting page-by-page content.
pub struct PdfLoader {
    source_path: PathBuf,
    doc: Arc<Document>,
    pages_map: BTreeMap<u32, ObjectId>,
    meta: DocumentMeta,
}

impl PdfLoader {
    /// Loads a PDF document from a file path with strict validation.
    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        if !path_buf.exists() {
            bail!("Input PDF file not found: {}", path_buf.display());
        }
        if !path_buf.is_file() {
            bail!("Input path is not a file: {}", path_buf.display());
        }

        let doc = Document::load(&path_buf)
            .with_context(|| format!("Failed to parse PDF document at {}", path_buf.display()))?;

        let doc = Arc::new(doc);
        let pages_map = doc.get_pages();
        let page_count = pages_map.len() as u32;

        if page_count == 0 {
            bail!("PDF contains 0 pages: {}", path_buf.display());
        }

        let (title, author) = Self::extract_doc_info(&doc);

        let meta = DocumentMeta {
            source_path: path_buf.to_string_lossy().to_string(),
            title,
            author,
            page_count,
            total_chars: 0,
            total_words: 0,
        };

        Ok(Self {
            source_path: path_buf,
            doc,
            pages_map,
            meta,
        })
    }

    /// Total number of pages in the PDF.
    pub fn page_count(&self) -> u32 {
        self.meta.page_count
    }

    /// Document metadata.
    pub fn metadata(&self) -> &DocumentMeta {
        &self.meta
    }

    /// Source file path.
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Extracts a single page by its 1-based page number.
    pub fn extract_page(&self, page_num: u32) -> Result<PageData> {
        let page_id = self
            .pages_map
            .get(&page_num)
            .copied()
            .with_context(|| format!("Page {page_num} not found (total: {})", self.meta.page_count))?;

        let (width, height) = Self::get_page_dimensions(&self.doc, page_id);

        let text = match self.doc.extract_text(&[page_num]) {
            Ok(t) => t,
            Err(_) => {
                // Fallback to manual content stream decoding
                Self::extract_page_content_manual(&self.doc, page_id)
                    .unwrap_or_default()
            }
        };

        Ok(PageData::new(page_num, width, height, text))
    }

    /// Extracts all pages sequentially.
    pub fn extract_all_sequential(&self) -> Result<Vec<PageData>> {
        let mut pages = Vec::with_capacity(self.meta.page_count as usize);
        for page_num in 1..=self.meta.page_count {
            pages.push(self.extract_page(page_num)?);
        }
        Ok(pages)
    }

    /// Extracts all pages in parallel using a thread pool.
    /// Results are guaranteed to be sorted by page number in ascending order.
    pub fn extract_all_parallel(&self, num_threads: Option<usize>) -> Result<Vec<PageData>> {
        let page_nums: Vec<u32> = (1..=self.meta.page_count).collect();

        let pool = match num_threads {
            Some(threads) if threads > 0 => rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .with_context(|| "Failed to initialize Rayon thread pool")?,
            _ => rayon::ThreadPoolBuilder::new()
                .build()
                .with_context(|| "Failed to initialize default Rayon thread pool")?,
        };

        let results: Result<Vec<PageData>> = pool.install(|| {
            page_nums
                .par_iter()
                .map(|&page_num| self.extract_page(page_num))
                .collect()
        });

        results
    }

    /// Streams extracted pages concurrently through a bounded crossbeam channel.
    /// Each page is sent to the channel immediately upon completion by parallel worker threads.
    pub fn stream_pages_parallel(
        &self,
        sender: crossbeam_channel::Sender<PageData>,
        num_threads: Option<usize>,
    ) -> Result<()> {
        let pool = match num_threads {
            Some(threads) if threads > 0 => rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .with_context(|| "Failed to initialize Rayon thread pool")?,
            _ => rayon::ThreadPoolBuilder::new()
                .build()
                .with_context(|| "Failed to initialize default Rayon thread pool")?,
        };

        let page_nums: Vec<u32> = (1..=self.meta.page_count).collect();
        pool.install(|| {
            page_nums.par_iter().for_each(|&page_num| {
                if let Ok(page) = self.extract_page(page_num) {
                    let _ = sender.send(page);
                }
            });
        });
        Ok(())
    }

    /// Inspects the page dictionary for MediaBox or CropBox dimensions.
    fn get_page_dimensions(doc: &Document, page_id: ObjectId) -> (f32, f32) {
        if let Ok(page_dict) = doc.get_object(page_id).and_then(Object::as_dict) {
            let box_obj = page_dict.get(b"MediaBox").or_else(|_| page_dict.get(b"CropBox"));
            if let Ok(Object::Array(arr)) = box_obj {
                if arr.len() >= 4 {
                    let x0 = arr[0].as_float().or_else(|_| arr[0].as_i64().map(|v| v as f32)).unwrap_or(0.0);
                    let y0 = arr[1].as_float().or_else(|_| arr[1].as_i64().map(|v| v as f32)).unwrap_or(0.0);
                    let x1 = arr[2].as_float().or_else(|_| arr[2].as_i64().map(|v| v as f32)).unwrap_or(0.0);
                    let y1 = arr[3].as_float().or_else(|_| arr[3].as_i64().map(|v| v as f32)).unwrap_or(0.0);
                    let width = (x1 - x0).abs();
                    let height = (y1 - y0).abs();
                    return (width, height);
                }
            }
        }
        (0.0, 0.0)
    }

    /// Extracts title and author from the PDF trailer Info dictionary if present.
    fn extract_doc_info(doc: &Document) -> (Option<String>, Option<String>) {
        let mut title = None;
        let mut author = None;

        if let Ok(info_obj) = doc.trailer.get(b"Info") {
            let info_dict = match info_obj {
                Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()),
                Object::Dictionary(dict) => Some(dict),
                _ => None,
            };

            if let Some(dict) = info_dict {
                if let Ok(Object::String(bytes, _)) = dict.get(b"Title") {
                    title = Some(Self::decode_pdf_string(bytes));
                }
                if let Ok(Object::String(bytes, _)) = dict.get(b"Author") {
                    author = Some(Self::decode_pdf_string(bytes));
                }
            }
        }

        (title, author)
    }

    /// Decodes a PDF string handling UTF-16BE BOM (\xfe\xff), UTF-16LE, and UTF-8.
    fn decode_pdf_string(bytes: &[u8]) -> String {
        if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
            let u16_chars: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16_lossy(&u16_chars).trim().to_string()
        } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
            let u16_chars: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16_lossy(&u16_chars).trim().to_string()
        } else {
            String::from_utf8_lossy(bytes).trim().to_string()
        }
    }

    /// Manual fallback extractor that parses PDF content stream operations.
    fn extract_page_content_manual(doc: &Document, page_id: ObjectId) -> Result<String> {
        let fonts = doc.get_page_fonts(page_id).unwrap_or_default();
        let encodings: BTreeMap<Vec<u8>, Encoding> = fonts
            .into_iter()
            .filter_map(|(name, font)| match font.get_font_encoding(doc) {
                Ok(enc) => Some((name, enc)),
                Err(_) => None,
            })
            .collect();

        let content_data = match doc.get_page_content(page_id) {
            Ok(d) if !d.is_empty() => d,
            _ => return Ok(String::new()),
        };

        let content = Content::decode(&content_data)
            .with_context(|| "Failed to decode page content stream")?;

        let mut current_encoding: Option<&Encoding> = None;
        let mut current_text = String::new();
        let mut result = String::new();

        for operation in &content.operations {
            match operation.operator.as_ref() {
                "Tf" => {
                    let font_name = operation
                        .operands
                        .first()
                        .and_then(|op| op.as_name().ok());
                    current_encoding = font_name.and_then(|name| encodings.get(name));

                    if !current_text.is_empty() {
                        result.push_str(&current_text);
                        current_text.clear();
                    }
                }
                "Tj" | "TJ" => {
                    Self::collect_operands_text(&mut current_text, current_encoding, &operation.operands);
                }
                "'" => {
                    if !current_text.ends_with('\n') {
                        current_text.push('\n');
                    }
                    Self::collect_operands_text(&mut current_text, current_encoding, &operation.operands);
                }
                "\"" => {
                    if !current_text.ends_with('\n') {
                        current_text.push('\n');
                    }
                    if let Some(str_op) = operation.operands.get(2) {
                        Self::collect_operands_text(&mut current_text, current_encoding, std::slice::from_ref(str_op));
                    }
                }
                "ET" if !current_text.ends_with('\n') => {
                    current_text.push('\n');
                }
                _ => {}
            }
        }

        if !current_text.is_empty() {
            result.push_str(&current_text);
        }

        Ok(result)
    }

    fn collect_operands_text(
        text: &mut String,
        encoding: Option<&Encoding>,
        operands: &[Object],
    ) {
        for operand in operands {
            match operand {
                Object::String(bytes, _) => {
                    if let Some(decoded) = encoding.and_then(|enc| Document::decode_text(enc, bytes).ok()) {
                        text.push_str(&decoded);
                        continue;
                    }
                    text.push_str(&String::from_utf8_lossy(bytes));
                }
                Object::Array(arr) => {
                    Self::collect_operands_text(text, encoding, arr);
                    text.push(' ');
                }
                Object::Integer(i) if *i < -100 => {
                    text.push(' ');
                }
                _ => {}
            }
        }
    }
}
