use crate::format::PluFormat;
use crate::types::{DumpFormat, OperationStats, PageData, PluDocument};
use anyhow::{bail, Context, Result};
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
                let mut writer = BufWriter::with_capacity(64 * 1024, file);
                PluFormat::write(&mut writer, doc)?
            }
            DumpFormat::Text => {
                let file = File::create(path)
                    .with_context(|| format!("Failed to create output text file: {}", path.display()))?;
                let mut writer = BufWriter::with_capacity(64 * 1024, file);
                let mut total_bytes = 0u64;

                // Write header summary
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
                let mut writer = BufWriter::with_capacity(64 * 1024, file);
                serde_json::to_writer_pretty(&mut writer, doc)
                    .with_context(|| "Failed to serialize document to JSON")?;
                writer.flush()?;
                fs::metadata(path)?.len()
            }
            DumpFormat::JsonLines => {
                let file = File::create(path)
                    .with_context(|| format!("Failed to create output JSONL file: {}", path.display()))?;
                let mut writer = BufWriter::with_capacity(64 * 1024, file);
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

    /// Unloads / reads a `.plu` container file into memory.
    pub fn unload_file<P: AsRef<Path>>(input_path: P) -> Result<PluDocument> {
        let path = input_path.as_ref();
        if !path.exists() {
            bail!("Input .plu file not found: {}", path.display());
        }

        let file = File::open(path)
            .with_context(|| format!("Failed to open .plu file: {}", path.display()))?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);

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
