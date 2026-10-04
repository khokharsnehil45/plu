use anyhow::{bail, Context, Result};
use clap::Parser;
use plu::loader::PdfLoader;
use plu::types::{DumpFormat, PageData, PluDocument};
use plu::unloader::PdfUnloader;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "plu",
    author = "High Performance PDF Systems",
    version = "1.0.0",
    about = "High-performance PDF Loader and Unloader with concurrent page-by-page extraction"
)]
struct Args {
    /// Path to the input PDF file to load (page-by-page extraction)
    #[arg(short = 'l', long = "load", value_name = "FILE_PATH")]
    load: Option<PathBuf>,

    /// Path to the file to dump / unload to, or .plu file to unload from
    #[arg(short = 'u', long = "unload", value_name = "FILE_PATH")]
    unload: Option<PathBuf>,

    /// Specific page number to inspect or extract (1-based)
    #[arg(short = 'p', long = "page", value_name = "PAGE_NUM")]
    page: Option<u32>,

    /// Unpack all pages from a .plu file into individual text files in this directory
    #[arg(long = "unpack", value_name = "OUTPUT_DIR")]
    unpack: Option<PathBuf>,

    /// Number of worker threads for parallel extraction (default: CPU cores)
    #[arg(short = 't', long = "threads", value_name = "N")]
    threads: Option<usize>,

    /// Force output format (plu, txt, json, jsonl)
    #[arg(short = 'f', long = "format", value_name = "FORMAT")]
    format: Option<String>,

    /// Verbose output with detailed page breakdown
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
}

fn main() {
    let args = Args::parse();

    if let Err(err) = run(args) {
        eprintln!("\x1b[1;31m[ERROR]\x1b[0m {err:#}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    match (&args.load, &args.unload) {
        // Case 1: Both --load and --unload provided:
        // plu --load input.pdf --unload output.plu (or output.txt, output.json)
        (Some(load_path), Some(unload_path)) => {
            handle_load_and_unload(load_path, unload_path, &args)
        }

        // Case 2: Only --load provided:
        // Default unload target is `<file_stem>.plu` or `plu`
        (Some(load_path), None) => {
            let default_unload = load_path
                .file_stem()
                .map(|s| PathBuf::from(format!("{}.plu", s.to_string_lossy())))
                .unwrap_or_else(|| PathBuf::from("plu"));
            println!(
                "\x1b[1;33m[INFO]\x1b[0m No --unload path specified. Defaulting to: \x1b[1m{}\x1b[0m",
                default_unload.display()
            );
            handle_load_and_unload(load_path, &default_unload, &args)
        }

        // Case 3: Only --unload provided:
        // Unload an existing .plu dump file (inspect, unpack, or display)
        (None, Some(unload_path)) => handle_unload_only(unload_path, &args),

        // Case 4: Neither provided
        (None, None) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Missing arguments.");
            eprintln!("Usage examples:");
            eprintln!("  plu --load document.pdf --unload document.plu");
            eprintln!("  plu --load document.pdf --unload output.txt");
            eprintln!("  plu --unload document.plu");
            eprintln!("  plu --unload document.plu --unpack ./extracted_pages");
            eprintln!("Run `plu --help` for full usage documentation.");
            std::process::exit(1);
        }
    }
}

/// Executes the concurrent loader -> unloader streaming pipeline
fn handle_load_and_unload(load_path: &Path, unload_path: &Path, args: &Args) -> Result<()> {
    println!("\x1b[1;36m========================================================\x1b[0m");
    println!("\x1b[1;36m  PLU: High-Performance Concurrent PDF Pipeline\x1b[0m");
    println!("\x1b[1;36m========================================================\x1b[0m");
    println!("[Loader] Loading PDF: \x1b[1m{}\x1b[0m", load_path.display());

    let overall_start = Instant::now();

    // 1. Initialize Loader Component
    let loader_start = Instant::now();
    let loader = PdfLoader::load_file(load_path)
        .with_context(|| format!("Loader failed to open PDF: {}", load_path.display()))?;

    let total_pages = loader.page_count();
    let doc_meta = loader.metadata().clone();
    let loader_init_time = loader_start.elapsed();

    println!(
        "[Loader] Discovered \x1b[1;32m{} pages\x1b[0m in {:.2?}",
        total_pages, loader_init_time
    );
    if let Some(ref title) = doc_meta.title {
        println!("[Loader] Document Title: {title}");
    }
    if let Some(ref author) = doc_meta.author {
        println!("[Loader] Document Author: {author}");
    }

    let dump_format = if let Some(ref fmt) = args.format {
        match fmt.to_ascii_lowercase().as_str() {
            "plu" => DumpFormat::Plu,
            "txt" => DumpFormat::Text,
            "json" => DumpFormat::Json,
            "jsonl" => DumpFormat::JsonLines,
            other => bail!("Unknown format: '{other}'. Choose from: plu, txt, json, jsonl"),
        }
    } else {
        PdfUnloader::detect_format(unload_path)
    };

    println!("[Unloader] Target Format: \x1b[1;35m{}\x1b[0m", dump_format);
    println!("[Unloader] Target File  : \x1b[1m{}\x1b[0m", unload_path.display());

    let stats = if let Some(single_page) = args.page {
        // Single page extraction mode
        if single_page < 1 || single_page > total_pages {
            bail!("Requested page {single_page} is out of range (1..={total_pages})");
        }
        println!("[Loader] Extracting single page: {}", single_page);
        let page = loader.extract_page(single_page)?;
        let doc = PluDocument::new(
            loader.source_path().to_string_lossy().to_string(),
            doc_meta.title,
            doc_meta.author,
            vec![page],
        );
        PdfUnloader::dump_with_format(&doc, unload_path, dump_format)?
    } else {
        // Multi-threaded concurrent streaming mode:
        // - Loader thread pool extracts pages concurrently across all CPU cores
        // - Bounded channel streams finished pages in real time
        // - Unloader thread actively writes/dumps to disk overlapping I/O and CPU
        let effective_threads = args
            .threads
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));

        println!(
            "[Pipeline] Starting concurrent execution (Worker Threads: {}, Dedicated Unloader Thread: 1)...",
            effective_threads
        );

        let channel_cap = (effective_threads * 4).max(32);
        let (tx, rx) = crossbeam_channel::bounded::<PageData>(channel_cap);

        let unloader_path = unload_path.to_path_buf();
        let unloader_meta = doc_meta.clone();

        // Spawn Unloader on dedicated thread
        let unloader_handle = std::thread::Builder::new()
            .name("unloader".to_string())
            .spawn(move || {
                PdfUnloader::dump_stream(rx, &unloader_meta, total_pages, unloader_path, dump_format)
            })
            .context("Failed to spawn Unloader thread")?;

        // Parallel extraction on Rayon worker pool
        loader.stream_pages_parallel(tx, args.threads)?;

        // Wait for Unloader to flush and finish
        unloader_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Unloader thread panicked"))??
    };

    let total_elapsed = overall_start.elapsed();
    let pps = if total_elapsed.as_secs_f64() > 0.0 {
        stats.pages_processed as f64 / total_elapsed.as_secs_f64()
    } else {
        stats.pages_processed as f64
    };

    println!("\x1b[1;32m========================================================\x1b[0m");
    println!("\x1b[1;32m  PLU Pipeline Complete Successfully!\x1b[0m");
    println!("\x1b[1;32m========================================================\x1b[0m");
    println!("  Output File     : {}", unload_path.display());
    println!("  Pages Processed : {}", stats.pages_processed);
    println!("  Total Chars     : {}", stats.total_chars);
    println!("  Total Words     : {}", stats.total_words);
    println!(
        "  Payload Size    : {:.2} KB ({} bytes)",
        stats.bytes_written as f64 / 1024.0,
        stats.bytes_written
    );
    println!("  Throughput      : \x1b[1;32m{:.1} pages/sec\x1b[0m", pps);
    println!("  Total Time      : \x1b[1m{:.2?}\x1b[0m", total_elapsed);
    println!("\x1b[1;32m========================================================\x1b[0m");

    Ok(())
}

/// Handles unloading / inspecting an existing .plu container
fn handle_unload_only(plu_path: &Path, args: &Args) -> Result<()> {
    println!("\x1b[1;36m========================================================\x1b[0m");
    println!("\x1b[1;36m  PLU Unloader: Reading Container\x1b[0m");
    println!("\x1b[1;36m========================================================\x1b[0m");
    println!("[Unloader] Reading: \x1b[1m{}\x1b[0m", plu_path.display());

    let start = Instant::now();

    // Check if unpacking to a directory was requested
    if let Some(ref out_dir) = args.unpack {
        println!("[Unloader] Unpacking pages into: \x1b[1m{}\x1b[0m", out_dir.display());
        let count = PdfUnloader::unpack_to_directory(plu_path, out_dir)?;
        println!(
            "\x1b[1;32m[Unloader] Successfully unpacked {} pages into {} in {:.2?}\x1b[0m",
            count,
            out_dir.display(),
            start.elapsed()
        );
        return Ok(());
    }

    // Check if a single page was requested
    if let Some(page_num) = args.page {
        println!("[Unloader] Fast O(1) random access lookup for Page {}", page_num);
        let page = PdfUnloader::unload_single_page(plu_path, page_num)?;
        let elapsed = start.elapsed();
        println!(
            "\x1b[1;32m[Unloader] Loaded Page {} in {:.2?}\x1b[0m ({}x{} pt, {} chars, {} words)",
            page.page_num, elapsed, page.width, page.height, page.char_count, page.word_count
        );
        println!("\n--- Page {} Content ---", page.page_num);
        println!("{}", page.text.trim());
        return Ok(());
    }

    // Read full container
    let doc = PdfUnloader::unload_file(plu_path)?;
    let elapsed = start.elapsed();

    println!("\x1b[1;32m[Unloader] Verified and loaded container in {:.2?}\x1b[0m", elapsed);
    println!("  Source PDF      : {}", doc.meta.source_path);
    if let Some(ref title) = doc.meta.title {
        println!("  Title           : {title}");
    }
    if let Some(ref author) = doc.meta.author {
        println!("  Author          : {author}");
    }
    println!("  Total Pages     : {}", doc.meta.page_count);
    println!("  Total Characters: {}", doc.meta.total_chars);
    println!("  Total Words     : {}", doc.meta.total_words);

    if args.verbose {
        println!("\n[Unloader] Page Inventory:");
        for p in &doc.pages {
            println!(
                "  • Page {:>4}: {:>6.1}x{:<6.1} pt | {:>6} chars | {:>6} words",
                p.page_num, p.width, p.height, p.char_count, p.word_count
            );
        }
    } else {
        println!("\n[Tip] Use `--verbose` to view per-page metrics, `--page <N>` to inspect a page, or `--unpack <DIR>` to extract all pages to disk.");
    }

    Ok(())
}
