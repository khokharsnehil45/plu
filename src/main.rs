use anyhow::{bail, Context, Result};
use clap::Parser;
use plu::loader::PdfLoader;
use plu::types::{DumpFormat, PluDocument};
use plu::unloader::PdfUnloader;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "plu",
    author = "High Performance PDF Systems",
    version = "1.0.0",
    about = "High-performance PDF Loader and Unloader with page-by-page extraction"
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

/// Executes the full loader -> unloader pipeline
fn handle_load_and_unload(load_path: &Path, unload_path: &Path, args: &Args) -> Result<()> {
    println!("\x1b[1;36m========================================================\x1b[0m");
    println!("\x1b[1;36m  PLU: High-Performance PDF Loader & Unloader\x1b[0m");
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

    // 2. Page-by-Page Extraction
    let extract_start = Instant::now();
    println!(
        "[Loader] Starting parallel page-by-page extraction (threads: {})...",
        args.threads
            .map(|t| t.to_string())
            .unwrap_or_else(|| "auto".to_string())
    );

    let pages = if let Some(single_page) = args.page {
        if single_page < 1 || single_page > total_pages {
            bail!("Requested page {single_page} is out of range (1..={total_pages})");
        }
        vec![loader.extract_page(single_page)?]
    } else {
        loader.extract_all_parallel(args.threads)?
    };

    let extract_duration = extract_start.elapsed();
    let total_chars: usize = pages.iter().map(|p| p.char_count).sum();
    let total_words: usize = pages.iter().map(|p| p.word_count).sum();
    let pages_count = pages.len();

    let pps = if extract_duration.as_secs_f64() > 0.0 {
        pages_count as f64 / extract_duration.as_secs_f64()
    } else {
        pages_count as f64
    };

    println!(
        "[Loader] Page-by-page extraction complete: \x1b[1;32m{} pages\x1b[0m ({:.1} pages/sec) in {:.2?}",
        pages_count, pps, extract_duration
    );
    println!(
        "[Loader] Extracted \x1b[1m{} characters\x1b[0m, \x1b[1m{} words\x1b[0m",
        total_chars, total_words
    );

    if args.verbose {
        println!("\n[Loader] Page Breakdown:");
        for p in &pages {
            println!(
                "  • Page {:>4}: {:>6.1}x{:<6.1} pt | {:>6} chars | {:>6} words",
                p.page_num, p.width, p.height, p.char_count, p.word_count
            );
        }
        println!();
    }

    // 3. Initialize Unloader Component and Dump
    println!("[Unloader] Initializing dump to: \x1b[1m{}\x1b[0m", unload_path.display());
    let doc = PluDocument::new(
        loader.source_path().to_string_lossy().to_string(),
        doc_meta.title,
        doc_meta.author,
        pages,
    );

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

    let stats = PdfUnloader::dump_with_format(&doc, unload_path, dump_format)
        .with_context(|| format!("Unloader failed to write dump to {}", unload_path.display()))?;

    let total_elapsed = overall_start.elapsed();

    println!("\x1b[1;32m========================================================\x1b[0m");
    println!("\x1b[1;32m  PLU Pipeline Complete Successfully!\x1b[0m");
    println!("\x1b[1;32m========================================================\x1b[0m");
    println!("  Output File     : {}", unload_path.display());
    println!("  Pages Dumped    : {}", stats.pages_processed);
    println!("  Payload Size    : {:.2} KB ({} bytes)", stats.bytes_written as f64 / 1024.0, stats.bytes_written);
    println!("  Unloader Time   : {} ms", stats.duration_ms);
    println!("  Total Time      : {:.2?}", total_elapsed);
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
