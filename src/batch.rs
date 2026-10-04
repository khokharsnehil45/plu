use crate::loader::PdfLoader;
use crate::types::DumpFormat;
use crate::ui::{
    print_divider_color, print_kv, print_kv_colored, BRIGHT_GREEN, RED, RESET, YELLOW,
};
use crate::unloader::PdfUnloader;
use anyhow::{bail, Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Default, Clone)]
pub struct BatchStats {
    pub files_processed: usize,
    pub total_pages: u32,
    pub total_chars: usize,
    pub total_words: usize,
    pub bytes_written: u64,
    pub duration_ms: u128,
    pub failed_files: Vec<(PathBuf, String)>,
}

/// Discovers files with the specified extension in a directory (case-insensitive).
pub fn discover_files<P: AsRef<Path>>(dir: P, ext: &str) -> Result<Vec<PathBuf>> {
    let dir_path = dir.as_ref();
    if !dir_path.exists() {
        bail!("Directory not found: {}", dir_path.display());
    }
    if !dir_path.is_dir() {
        bail!("Path is not a directory: {}", dir_path.display());
    }

    let mut files = Vec::new();
    collect_files_recursive(dir_path, ext, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_recursive(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read directory: {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, ext, out)?;
        } else if path.is_file() {
            if let Some(file_ext) = path.extension().and_then(|e| e.to_str()) {
                if file_ext.eq_ignore_ascii_case(ext) {
                    out.push(path);
                }
            }
        }
    }
    Ok(())
}

pub fn create_page_progress_bar(total_pages: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_pages);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} pages ({per_sec}) {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("█▓▒░ "),
    );
    pb.set_message(message.to_string());
    pb
}

pub fn create_batch_file_progress_bar(total_files: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_files);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.yellow} [{elapsed_precise}] [{bar:30.green/white}] {pos}/{len} files ({per_sec}) {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("█▓▒░ "),
    );
    pb.set_message(message.to_string());
    pb
}

/// Executes batch loading of all PDF files in `input_dir` and dumps them into `output_dir`.
pub fn run_batch_load(
    input_dir: &Path,
    output_dir: &Path,
    threads: Option<usize>,
    format: DumpFormat,
) -> Result<BatchStats> {
    run_batch_load_with_ocr(input_dir, output_dir, threads, format, true, "eng")
}

/// Executes batch loading with explicit OCR settings.
pub fn run_batch_load_with_ocr(
    input_dir: &Path,
    output_dir: &Path,
    threads: Option<usize>,
    format: DumpFormat,
    ocr_enabled: bool,
    ocr_lang: &str,
) -> Result<BatchStats> {
    let start = Instant::now();
    let pdf_files = discover_files(input_dir, "pdf")?;

    if pdf_files.is_empty() {
        bail!("No .pdf files found in directory: {}", input_dir.display());
    }

    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    let ext = format.extension();

    let mp = MultiProgress::new();
    let file_pb = mp.add(create_batch_file_progress_bar(
        pdf_files.len() as u64,
        "Batch Loading PDFs",
    ));

    let mut batch_stats = BatchStats::default();

    for pdf_path in &pdf_files {
        let file_stem = pdf_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".to_string());
        let target_path = output_dir.join(format!("{}.{}", file_stem, ext));

        file_pb.set_message(format!("Loading {}", file_stem));

        // Load PDF with OCR configuration
        let loader = match PdfLoader::load_file_with_ocr(pdf_path, ocr_enabled, ocr_lang) {
            Ok(l) => l,
            Err(e) => {
                batch_stats.failed_files.push((pdf_path.clone(), e.to_string()));
                file_pb.inc(1);
                continue;
            }
        };

        let total_pages = loader.page_count();
        let meta = loader.metadata().clone();

        let page_pb = mp.add(create_page_progress_bar(
            total_pages as u64,
            &format!("{}.pdf", file_stem),
        ));

        let effective_threads = threads
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        let channel_cap = (effective_threads * 4).max(32);
        let (tx, rx) = crossbeam_channel::bounded(channel_cap);

        let unloader_path = target_path.clone();
        let unloader_meta = meta.clone();
        let progress_clone = page_pb.clone();

        let unloader_handle = std::thread::spawn(move || {
            PdfUnloader::dump_stream_with_progress(
                rx,
                &unloader_meta,
                total_pages,
                unloader_path,
                format,
                Some(progress_clone),
            )
        });

        if let Err(e) = loader.stream_pages_parallel(tx, threads) {
            batch_stats.failed_files.push((pdf_path.clone(), e.to_string()));
            let _ = mp.remove(&page_pb);
            file_pb.inc(1);
            continue;
        }

        match unloader_handle.join() {
            Ok(Ok(stats)) => {
                batch_stats.files_processed += 1;
                batch_stats.total_pages += stats.pages_processed;
                batch_stats.total_chars += stats.total_chars;
                batch_stats.total_words += stats.total_words;
                batch_stats.bytes_written += stats.bytes_written;
            }
            Ok(Err(e)) => {
                batch_stats.failed_files.push((pdf_path.clone(), e.to_string()));
            }
            Err(_) => {
                batch_stats.failed_files.push((pdf_path.clone(), "Thread panicked".to_string()));
            }
        }

        let _ = mp.remove(&page_pb);
        file_pb.inc(1);
    }

    file_pb.finish_with_message("Batch Load Complete");
    batch_stats.duration_ms = start.elapsed().as_millis();

    Ok(batch_stats)
}

/// Executes batch unloading of all .plu files in `input_dir` and dumps them as text in `output_dir`.
pub fn run_batch_unload(input_dir: &Path, output_dir: &Path) -> Result<BatchStats> {
    let start = Instant::now();
    let plu_files = discover_files(input_dir, "plu")?;

    if plu_files.is_empty() {
        bail!("No .plu files found in directory: {}", input_dir.display());
    }

    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    let mp = MultiProgress::new();
    let file_pb = mp.add(create_batch_file_progress_bar(
        plu_files.len() as u64,
        "Batch Unloading .plu Containers",
    ));

    let mut batch_stats = BatchStats::default();

    for plu_path in &plu_files {
        let file_stem = plu_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".to_string());
        let target_path = output_dir.join(format!("{}.txt", file_stem));

        file_pb.set_message(format!("Unloading {}", file_stem));

        let page_pb = mp.add(create_page_progress_bar(1, &format!("{}.plu", file_stem)));

        match PdfUnloader::unload_to_file(plu_path, &target_path, Some(page_pb.clone())) {
            Ok(stats) => {
                batch_stats.files_processed += 1;
                batch_stats.total_pages += stats.pages_processed;
                batch_stats.total_chars += stats.total_chars;
                batch_stats.total_words += stats.total_words;
                batch_stats.bytes_written += stats.bytes_written;
            }
            Err(e) => {
                batch_stats.failed_files.push((plu_path.clone(), e.to_string()));
            }
        }

        let _ = mp.remove(&page_pb);
        file_pb.inc(1);
    }

    file_pb.finish_with_message("Batch Unload Complete");
    batch_stats.duration_ms = start.elapsed().as_millis();

    Ok(batch_stats)
}

pub fn print_batch_summary(stats: &BatchStats, action: &str, output_path: &Path) {
    let total_secs = stats.duration_ms as f64 / 1000.0;
    let pps = if total_secs > 0.0 {
        stats.total_pages as f64 / total_secs
    } else {
        stats.total_pages as f64
    };

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}BATCH {action} COMPLETED SUCCESSFULLY{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Output Directory", &output_path.display().to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Files Processed", &stats.files_processed.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Total Pages", &stats.total_pages.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv("Total Characters", &stats.total_chars.to_string());
    print_kv("Total Words", &stats.total_words.to_string());
    print_kv(
        "Payload Written",
        &format!("{:.2} KB ({} bytes)", stats.bytes_written as f64 / 1024.0, stats.bytes_written),
    );
    print_kv_colored("Throughput", &format!("{:.1} pages/sec", pps), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Total Elapsed", &format!("{:.2}s", total_secs), YELLOW, BRIGHT_GREEN);

    if !stats.failed_files.is_empty() {
        println!();
        print_divider_color(RED);
        println!(" {RED}FAILED FILES ({}) : {RESET}", stats.failed_files.len());
        for (f, err) in &stats.failed_files {
            println!("   {RED}• {}: {err}{RESET}", f.display());
        }
        print_divider_color(RED);
    } else {
        print_divider_color(BRIGHT_GREEN);
    }
}
