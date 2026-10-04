use crate::format::PluFormat;
use crate::types::{DumpFormat, OperationStats, PageData, PluDocument, DocumentMeta};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

/// The Unloader component responsible for dumping extracted pages into storage formats,
/// and reading/unpacking .plu container archives.
pub struct PdfUnloader;

impl PdfUnloader {
    /// Detects format from file extension or defaults to .plu format.
    pub fn detect_format(path: &Path) -> DumpFormat {
        match path.extension().and_then(|ext| ext.to_str()).map(|s| s.to_ascii_lowercase()) {
            Some(ref s) if s == "txt" => DumpFormat::Text,
            Some(ref s) if s == "json" => DumpFormat::Json,
            Some(ref s) if s == "jsonl" => DumpFormat::JsonLines,
            _ => DumpFormat::Plu,
        }
    }

    /// Dumps an in-memory `PluDocument` into the target file using the appropriate format.
    pub fn dump_to_file<P: AsRef<Path>>(doc: &PluDocument, output_path: P) -> Result<OperationStats> {
        let path = output_path.as_ref();
        let format = Self::detect_format(path);
        Self::dump_with_format(doc, path, format)
    }

    /// Dumps an in-memory `PluDocument` with explicit format choice.
    pub fn dump_with_format<P: AsRef<Path>>(
        doc: &PluDocument,
        output_path: P,
        format: DumpFormat,
    ) -> Result<OperationStats> {
        let path = output_path.as_ref();
        let start = Instant::now();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
            }
        }

        let bytes_written = match format {
            DumpFormat::Plu => {
                let file = File::create(path)
                    .with_context(|| format!("Failed to create output .plu file: {}", path.display()))?;
                let mut writer = BufWriter::with_capacity(128 * 1024, file);
                PluFormat::write(&mut writer, doc)?
            }
            DumpFormat::Text => {
                let file = File::create(path)
                    .with_context(|| format!("Failed to create output text file: {}", path.display()))?;
                let mut writer = BufWriter::with_capacity(128 * 1024, file);
                let mut total_bytes = 0u64;

                let header = format!(
                    "================================================================================\n\
                     PDF DUMP: {}\n\
                     Pages: {} | Characters: {} | Words: {}\n\
                     ================================================================================\n\n",
                    doc.meta.source_path, doc.meta.page_count, doc.meta.total_chars, doc.meta.total_words
                );
                writer.write_all(header.as_bytes())?;
                total_bytes += header.len() as u64;

                for page in &doc.pages {
                    let page_hdr = format!(
                        "--- PAGE {} ({}x{} pt | {} chars | {} words) ---\n",
                        page.page_num, page.width, page.height, page.char_count, page.word_count
                    );
                    writer.write_all(page_hdr.as_bytes())?;
                    writer.write_all(page.text.as_bytes())?;
                    writer.write_all(b"\n\n")?;
                    total_bytes += (page_hdr.len() + page.text.len() + 2) as u64;
                }
                writer.flush()?;
                total_bytes
            }
            DumpFormat::Json => {
                let file = File::create(path)
                    .with_context(|| format!("Failed to create output JSON file: {}", path.display()))?;
                let mut writer = BufWriter::with_capacity(128 * 1024, file);
                serde_json::to_writer_pretty(&mut writer, doc)
                    .with_context(|| "Failed to serialize document to JSON")?;
                writer.flush()?;
                fs::metadata(path)?.len()
            }
            DumpFormat::JsonLines => {
                let file = File::create(path)
                    .with_context(|| format!("Failed to create output JSONL file: {}", path.display()))?;
                let mut writer = BufWriter::with_capacity(128 * 1024, file);
                let mut total_bytes = 0u64;

                for page in &doc.pages {
                    let line = serde_json::to_string(page)
                        .with_context(|| format!("Failed to serialize page {}", page.page_num))?;
                    writer.write_all(line.as_bytes())?;
                    writer.write_all(b"\n")?;
                    total_bytes += (line.len() + 1) as u64;
                }
                writer.flush()?;
                total_bytes
            }
        };

        let duration_ms = start.elapsed().as_millis();

        Ok(OperationStats {
            pages_processed: doc.pages.len() as u32,
            total_chars: doc.meta.total_chars,
            total_words: doc.meta.total_words,
            bytes_written,
            duration_ms,
        })
    }

    /// Streams pages directly from a concurrent receiver channel and dumps them to disk in real time.
    /// Uses an internal reorder buffer to guarantee sequential page ordering on disk even when
    /// worker threads extract pages out-of-order concurrently.
    pub fn dump_stream<P: AsRef<Path>>(
        receiver: crossbeam_channel::Receiver<PageData>,
        meta: &DocumentMeta,
        page_count: u32,
        output_path: P,
        format: DumpFormat,
    ) -> Result<OperationStats> {
        let path = output_path.as_ref();
        let start = Instant::now();

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
            }
        }

        let file = File::create(path)
            .with_context(|| format!("Failed to create output file: {}", path.display()))?;
        let mut writer = BufWriter::with_capacity(128 * 1024, file);

        let mut next_expected = 1u32;
        let mut reorder_buffer = BTreeMap::<u32, PageData>::new();
        let mut total_chars = 0usize;
        let mut total_words = 0usize;
        let mut pages_processed = 0u32;

        let bytes_written = match format {
            DumpFormat::Plu => {
                let stream_state = PluFormat::start_stream(&mut writer, meta, page_count)?;
                let mut index_entries = Vec::with_capacity(page_count as usize);

                while let Ok(page) = receiver.recv() {
                    reorder_buffer.insert(page.page_num, page);
                    while let Some(ready_page) = reorder_buffer.remove(&next_expected) {
                        total_chars += ready_page.char_count;
                        total_words += ready_page.word_count;
                        pages_processed += 1;
                        let entry = PluFormat::write_page_record(&mut writer, &ready_page)?;
                        index_entries.push(entry);
                        next_expected += 1;
                    }
                }

                while let Some((_, ready_page)) = reorder_buffer.pop_first() {
                    total_chars += ready_page.char_count;
                    total_words += ready_page.word_count;
                    pages_processed += 1;
                    let entry = PluFormat::write_page_record(&mut writer, &ready_page)?;
                    index_entries.push(entry);
                }

                PluFormat::finish_stream(&mut writer, stream_state, &index_entries, total_chars, total_words)?
            }
            DumpFormat::Text => {
                let mut total_bytes = 0u64;
                let header = format!(
                    "================================================================================\n\
                     PDF DUMP: {}\n\
                     Pages: {} | Title: {}\n\
                     ================================================================================\n\n",
                    meta.source_path, page_count, meta.title.as_deref().unwrap_or("(anonymous)")
                );
                writer.write_all(header.as_bytes())?;
                total_bytes += header.len() as u64;

                while let Ok(page) = receiver.recv() {
                    reorder_buffer.insert(page.page_num, page);
                    while let Some(ready_page) = reorder_buffer.remove(&next_expected) {
                        total_chars += ready_page.char_count;
                        total_words += ready_page.word_count;
                        pages_processed += 1;

                        let page_hdr = format!(
                            "--- PAGE {} ({}x{} pt | {} chars | {} words) ---\n",
                            ready_page.page_num, ready_page.width, ready_page.height, ready_page.char_count, ready_page.word_count
                        );
                        writer.write_all(page_hdr.as_bytes())?;
                        writer.write_all(ready_page.text.as_bytes())?;
                        writer.write_all(b"\n\n")?;
                        total_bytes += (page_hdr.len() + ready_page.text.len() + 2) as u64;
                        next_expected += 1;
                    }
                }

                while let Some((_, ready_page)) = reorder_buffer.pop_first() {
                    total_chars += ready_page.char_count;
                    total_words += ready_page.word_count;
                    pages_processed += 1;

                    let page_hdr = format!(
                        "--- PAGE {} ({}x{} pt | {} chars | {} words) ---\n",
                        ready_page.page_num, ready_page.width, ready_page.height, ready_page.char_count, ready_page.word_count
                    );
                    writer.write_all(page_hdr.as_bytes())?;
                    writer.write_all(ready_page.text.as_bytes())?;
                    writer.write_all(b"\n\n")?;
                    total_bytes += (page_hdr.len() + ready_page.text.len() + 2) as u64;
                }

                writer.flush()?;
                total_bytes
            }
            DumpFormat::JsonLines => {
                let mut total_bytes = 0u64;
                while let Ok(page) = receiver.recv() {
                    reorder_buffer.insert(page.page_num, page);
                    while let Some(ready_page) = reorder_buffer.remove(&next_expected) {
                        total_chars += ready_page.char_count;
                        total_words += ready_page.word_count;
                        pages_processed += 1;

                        let line = serde_json::to_string(&ready_page)?;
                        writer.write_all(line.as_bytes())?;
                        writer.write_all(b"\n")?;
                        total_bytes += (line.len() + 1) as u64;
                        next_expected += 1;
                    }
                }
                while let Some((_, ready_page)) = reorder_buffer.pop_first() {
                    total_chars += ready_page.char_count;
                    total_words += ready_page.word_count;
                    pages_processed += 1;

                    let line = serde_json::to_string(&ready_page)?;
                    writer.write_all(line.as_bytes())?;
                    writer.write_all(b"\n")?;
                    total_bytes += (line.len() + 1) as u64;
                }
                writer.flush()?;
                total_bytes
            }
            DumpFormat::Json => {
                let mut all_pages = Vec::with_capacity(page_count as usize);
                while let Ok(page) = receiver.recv() {
                    reorder_buffer.insert(page.page_num, page);
                    while let Some(ready_page) = reorder_buffer.remove(&next_expected) {
                        total_chars += ready_page.char_count;
                        total_words += ready_page.word_count;
                        pages_processed += 1;
                        all_pages.push(ready_page);
                        next_expected += 1;
                    }
                }
                while let Some((_, ready_page)) = reorder_buffer.pop_first() {
                    total_chars += ready_page.char_count;
                    total_words += ready_page.word_count;
                    pages_processed += 1;
                    all_pages.push(ready_page);
                }
                let mut full_meta = meta.clone();
                full_meta.total_chars = total_chars;
                full_meta.total_words = total_words;
                let doc = PluDocument { meta: full_meta, pages: all_pages };
                serde_json::to_writer_pretty(&mut writer, &doc)?;
                writer.flush()?;
                fs::metadata(path)?.len()
            }
        };

        let duration_ms = start.elapsed().as_millis();

        Ok(OperationStats {
            pages_processed,
            total_chars,
            total_words,
            bytes_written,
            duration_ms,
        })
    }

    /// Unloads / reads a `.plu` container file into memory.
    pub fn unload_file<P: AsRef<Path>>(input_path: P) -> Result<PluDocument> {
        let path = input_path.as_ref();
        if !path.exists() {
            bail!("Input .plu file not found: {}", path.display());
        }

        let file = File::open(path)
            .with_context(|| format!("Failed to open .plu file: {}", path.display()))?;
        let mut reader = BufReader::with_capacity(128 * 1024, file);

        PluFormat::read(&mut reader)
    }

    /// Reads single page from a `.plu` container using random-access index table.
    pub fn unload_single_page<P: AsRef<Path>>(input_path: P, page_num: u32) -> Result<PageData> {
        let path = input_path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open .plu file: {}", path.display()))?;
        let mut reader = BufReader::with_capacity(32 * 1024, file);

        PluFormat::read_single_page(&mut reader, page_num)
    }

    /// Unpacks all pages from a `.plu` container into separate text files inside `output_dir`.
    pub fn unpack_to_directory<P: AsRef<Path>, Q: AsRef<Path>>(
        plu_path: P,
        output_dir: Q,
    ) -> Result<usize> {
        let doc = Self::unload_file(plu_path)?;
        let out_dir = output_dir.as_ref();
        fs::create_dir_all(out_dir)
            .with_context(|| format!("Failed to create directory: {}", out_dir.display()))?;

        // Write metadata
        let meta_path = out_dir.join("metadata.json");
        let meta_file = File::create(&meta_path)?;
        serde_json::to_writer_pretty(meta_file, &doc.meta)?;

        // Write each page
        for page in &doc.pages {
            let page_filename = format!("page_{:04}.txt", page.page_num);
            let page_path = out_dir.join(page_filename);
            let mut file = File::create(page_path)?;
            file.write_all(page.text.as_bytes())?;
        }

        Ok(doc.pages.len())
    }
}
