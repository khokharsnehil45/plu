use anyhow::{bail, Context, Result};
use clap::Parser;
use plu::loader::PdfLoader;
use plu::ops::PdfOps;
use plu::types::{DumpFormat, PageData, PluDocument};
use plu::ui::{
    print_banner, print_divider, print_divider_color, print_kv, print_kv_colored,
    run_interactive_ui, BRIGHT_CYAN, BRIGHT_GREEN, BRIGHT_WHITE, RED, RESET, YELLOW,
};
use plu::unloader::PdfUnloader;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "plu",
    author = "High Performance PDF Systems",
    version = "1.0.0",
    about = "High-performance PDF Processor: Load, Extract, Unload, Compress, Split, and Merge"
)]
struct Args {
    /// Path to input PDF file to load (page-by-page extraction)
    #[arg(short = 'l', long = "load", value_name = "FILE_PATH")]
    load: Option<PathBuf>,

    /// Path to file to dump / unload to, or .plu file to unload from
    #[arg(short = 'u', long = "unload", value_name = "FILE_PATH")]
    unload: Option<PathBuf>,

    /// Compress a PDF by re-encoding stream objects
    #[arg(long = "compress", value_name = "FILE_PATH")]
    compress: Option<PathBuf>,

    /// Split a PDF by page ranges or unpack pages
    #[arg(long = "split", value_name = "FILE_PATH")]
    split: Option<PathBuf>,

    /// Merge multiple PDF files into one
    #[arg(long = "merge", num_args = 2.., value_name = "FILE_PATHS")]
    merge: Option<Vec<PathBuf>>,

    /// Output destination file for operations
    #[arg(short = 'o', long = "output", value_name = "FILE_PATH")]
    output: Option<PathBuf>,

    /// Specific page number to inspect or extract (1-based)
    #[arg(short = 'p', long = "page", value_name = "PAGE_NUM")]
    page: Option<u32>,

    /// Page range for split or extraction, e.g. "1-5"
    #[arg(long = "pages", value_name = "RANGE")]
    pages: Option<String>,

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

    /// Launch interactive terminal UI
    #[arg(long = "ui")]
    ui: bool,
}

fn main() {
    let args = Args::parse();

    if let Err(err) = run(args) {
        println!();
        print_divider_color(RED);
        println!(" {RED}ERROR: {err:#}{RESET}");
        print_divider_color(RED);
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    if args.ui {
        return run_interactive_ui();
    }

    // Compress mode
    if let Some(ref in_pdf) = args.compress {
        let out_pdf = args.output.clone().unwrap_or_else(|| {
            let stem = in_pdf.file_stem().unwrap_or_default().to_string_lossy();
            PathBuf::from(format!("{}_compressed.pdf", stem))
        });
        return handle_compress(in_pdf, &out_pdf);
    }

    // Split mode
    if let Some(ref in_pdf) = args.split {
        return handle_split(in_pdf, &args);
    }

    // Merge mode
    if let Some(ref merge_files) = args.merge {
        let out_pdf = args.output.clone().unwrap_or_else(|| PathBuf::from("merged.pdf"));
        return handle_merge(merge_files, &out_pdf);
    }

    // Load and Unload pipeline
    match (&args.load, &args.unload) {
        (Some(load_path), Some(unload_path)) => {
            handle_load_and_unload(load_path, unload_path, &args)
        }

        (Some(load_path), None) => {
            let default_unload = load_path
                .file_stem()
                .map(|s| PathBuf::from(format!("{}.plu", s.to_string_lossy())))
                .unwrap_or_else(|| PathBuf::from("plu"));
            handle_load_and_unload(load_path, &default_unload, &args)
        }

        (None, Some(unload_path)) => handle_unload_only(unload_path, &args),

        (None, None) => run_interactive_ui(),
    }
}

fn handle_compress(in_path: &Path, out_path: &Path) -> Result<()> {
    println!();
    print_banner();
    println!(" {YELLOW}Action{RESET}               : Compress PDF Streams");
    print_kv("Input PDF", &in_path.display().to_string());
    print_kv("Output PDF", &out_path.display().to_string());
    print_divider();

    let start = Instant::now();
    let (orig, compressed) = PdfOps::compress_pdf(in_path, out_path)?;
    let elapsed = start.elapsed();

    let saved = orig.saturating_sub(compressed);
    let ratio = if orig > 0 {
        (saved as f64 / orig as f64) * 100.0
    } else {
        0.0
    };

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}PDF COMPRESSED SUCCESSFULLY{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Original Size", &format!("{:.2} KB ({} bytes)", orig as f64 / 1024.0, orig), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Compressed Size", &format!("{:.2} KB ({} bytes)", compressed as f64 / 1024.0, compressed), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Reduction", &format!("{:.1}% saved ({} bytes)", ratio, saved), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

fn handle_split(in_path: &Path, args: &Args) -> Result<()> {
    println!();
    print_banner();
    println!(" {YELLOW}Action{RESET}               : Split PDF");
    print_kv("Input PDF", &in_path.display().to_string());

    if let Some(ref range_str) = args.pages {
        let mut pages = Vec::new();
        for part in range_str.split(',') {
            let part = part.trim();
            if part.contains('-') {
                let bounds: Vec<&str> = part.split('-').collect();
                if bounds.len() == 2 {
                    let start: u32 = bounds[0].trim().parse().unwrap_or(1);
                    let end: u32 = bounds[1].trim().parse().unwrap_or(start);
                    for p in start..=end {
                        pages.push(p);
                    }
                }
            } else if let Ok(p) = part.parse::<u32>() {
                pages.push(p);
            }
        }

        let out_path = args.output.clone().unwrap_or_else(|| PathBuf::from("split.pdf"));
        print_kv("Target Output", &out_path.display().to_string());
        print_divider();

        let start = Instant::now();
        let count = PdfOps::split_pages(in_path, &pages, &out_path)?;
        let elapsed = start.elapsed();

        println!();
        print_divider_color(BRIGHT_GREEN);
        println!(" {BRIGHT_GREEN}SPLIT COMPLETED{RESET}");
        print_divider_color(BRIGHT_GREEN);
        print_kv_colored("Extracted Pages", &count.to_string(), YELLOW, BRIGHT_GREEN);
        print_kv_colored("Output File", &out_path.display().to_string(), YELLOW, BRIGHT_WHITE);
        print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
        print_divider_color(BRIGHT_GREEN);
    } else {
        let out_dir = args.output.clone().unwrap_or_else(|| {
            let stem = in_path.file_stem().unwrap_or_default().to_string_lossy();
            PathBuf::from(format!("{}_pages", stem))
        });
        print_kv("Output Folder", &out_dir.display().to_string());
        print_divider();

        let start = Instant::now();
        let count = PdfOps::split_all(in_path, &out_dir)?;
        let elapsed = start.elapsed();

        println!();
        print_divider_color(BRIGHT_GREEN);
        println!(" {BRIGHT_GREEN}SPLIT COMPLETED{RESET}");
        print_divider_color(BRIGHT_GREEN);
        print_kv_colored("Single-Page PDFs", &count.to_string(), YELLOW, BRIGHT_GREEN);
        print_kv_colored("Output Folder", &out_dir.display().to_string(), YELLOW, BRIGHT_WHITE);
        print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
        print_divider_color(BRIGHT_GREEN);
    }

    Ok(())
}

fn handle_merge(inputs: &[PathBuf], output: &Path) -> Result<()> {
    println!();
    print_banner();
    println!(" {YELLOW}Action{RESET}               : Merge PDF Documents");
    print_kv("Inputs Count", &inputs.len().to_string());
    print_kv("Output PDF", &output.display().to_string());
    print_divider();

    let start = Instant::now();
    let total_pages = PdfOps::merge_pdfs(inputs, output)?;
    let elapsed = start.elapsed();

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}MERGE COMPLETED SUCCESSFULLY{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Files Merged", &inputs.len().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Total Pages", &total_pages.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Output File", &output.display().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

/// Executes the concurrent loader -> unloader streaming pipeline
fn handle_load_and_unload(load_path: &Path, unload_path: &Path, args: &Args) -> Result<()> {
    println!();
    print_banner();
    println!(" {YELLOW}Action{RESET}               : Concurrent Page-by-Page Extraction");
    print_kv("Input PDF", &load_path.display().to_string());

    let overall_start = Instant::now();

    // 1. Initialize Loader Component
    let loader_start = Instant::now();
    let loader = PdfLoader::load_file(load_path)
        .with_context(|| format!("Loader failed to open PDF: {}", load_path.display()))?;

    let total_pages = loader.page_count();
    let doc_meta = loader.metadata().clone();
    let loader_init_time = loader_start.elapsed();

    print_kv_colored(
        "Discovered Pages",
        &format!("{} (in {:.2?})", total_pages, loader_init_time),
        YELLOW,
        BRIGHT_GREEN,
    );
    if let Some(ref title) = doc_meta.title {
        print_kv("Document Title", title);
    }
    if let Some(ref author) = doc_meta.author {
        print_kv("Document Author", author);
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

    print_kv("Target Format", &dump_format.to_string());
    print_kv("Target File", &unload_path.display().to_string());

    let effective_threads = args
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
    print_kv("Worker Threads", &effective_threads.to_string());
    print_kv("Unloader Thread", "1 (Dedicated OS Thread)");
    print_divider();

    let stats = if let Some(single_page) = args.page {
        if single_page < 1 || single_page > total_pages {
            bail!("Requested page {single_page} is out of range (1..={total_pages})");
        }
        println!(" Extracting single page: {}", single_page);
        let page = loader.extract_page(single_page)?;
        let doc = PluDocument::new(
            loader.source_path().to_string_lossy().to_string(),
            doc_meta.title,
            doc_meta.author,
            vec![page],
        );
        PdfUnloader::dump_with_format(&doc, unload_path, dump_format)?
    } else {
        let channel_cap = (effective_threads * 4).max(32);
        let (tx, rx) = crossbeam_channel::bounded::<PageData>(channel_cap);

        let unloader_path = unload_path.to_path_buf();
        let unloader_meta = doc_meta.clone();

        let unloader_handle = std::thread::Builder::new()
            .name("unloader".to_string())
            .spawn(move || {
                PdfUnloader::dump_stream(rx, &unloader_meta, total_pages, unloader_path, dump_format)
            })
            .context("Failed to spawn Unloader thread")?;

        loader.stream_pages_parallel(tx, args.threads)?;

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

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}PIPELINE COMPLETED SUCCESSFULLY{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv("Output File", &unload_path.display().to_string());
    print_kv_colored("Pages Processed", &stats.pages_processed.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv("Total Chars", &stats.total_chars.to_string());
    print_kv("Total Words", &stats.total_words.to_string());
    print_kv(
        "Payload Size",
        &format!("{:.2} KB ({} bytes)", stats.bytes_written as f64 / 1024.0, stats.bytes_written),
    );
    print_kv_colored("Throughput", &format!("{:.1} pages/sec", pps), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Total Elapsed", &format!("{:.2?}", total_elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

/// Handles unloading / inspecting an existing .plu container
fn handle_unload_only(plu_path: &Path, args: &Args) -> Result<()> {
    println!();
    print_banner();
    println!(" {YELLOW}Action{RESET}               : Unload & Inspect .plu Container");
    print_kv("Reading File", &plu_path.display().to_string());

    let start = Instant::now();

    if let Some(ref out_dir) = args.unpack {
        print_kv("Unpack Target", &out_dir.display().to_string());
        print_divider();
        let count = PdfUnloader::unpack_to_directory(plu_path, out_dir)?;
        println!();
        print_divider_color(BRIGHT_GREEN);
        println!(" {BRIGHT_GREEN}UNPACK COMPLETED{RESET}");
        print_divider_color(BRIGHT_GREEN);
        print_kv_colored("Unpacked Pages", &count.to_string(), YELLOW, BRIGHT_GREEN);
        print_kv("Destination", &out_dir.display().to_string());
        print_kv_colored("Elapsed Time", &format!("{:.2?}", start.elapsed()), YELLOW, BRIGHT_GREEN);
        print_divider_color(BRIGHT_GREEN);
        return Ok(());
    }

    if let Some(page_num) = args.page {
        let page = PdfUnloader::unload_single_page(plu_path, page_num)?;
        let elapsed = start.elapsed();
        println!();
        print_divider_color(BRIGHT_GREEN);
        println!(" {BRIGHT_GREEN}PAGE {} CONTENT (O(1) RANDOM ACCESS LOOKUP){RESET}", page.page_num);
        print_divider_color(BRIGHT_GREEN);
        print_kv("Dimensions", &format!("{}x{} pt", page.width, page.height));
        print_kv("Characters", &page.char_count.to_string());
        print_kv("Words", &page.word_count.to_string());
        print_kv_colored("Lookup Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
        print_divider();
        println!("{}", page.text.trim());
        print_divider();
        return Ok(());
    }

    let doc = PdfUnloader::unload_file(plu_path)?;
    let elapsed = start.elapsed();

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}CONTAINER VERIFIED AND LOADED{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv("Source PDF", &doc.meta.source_path);
    if let Some(ref title) = doc.meta.title {
        print_kv("Title", title);
    }
    if let Some(ref author) = doc.meta.author {
        print_kv("Author", author);
    }
    print_kv("Total Pages", &doc.meta.page_count.to_string());
    print_kv("Total Characters", &doc.meta.total_chars.to_string());
    print_kv("Total Words", &doc.meta.total_words.to_string());
    print_kv_colored("Verification Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    if args.verbose {
        println!();
        print_divider();
        println!(" {BRIGHT_CYAN}PAGE INVENTORY{RESET}");
        print_divider();
        for p in &doc.pages {
            println!(
                " Page {:>4} : {:>6.1}x{:<6.1} pt | {:>6} chars | {:>6} words",
                p.page_num, p.width, p.height, p.char_count, p.word_count
            );
        }
        print_divider();
    } else {
        println!(" {YELLOW}Tip:{RESET} Use `--verbose` for full inventory, or `--page <N>` for single page.");
        print_divider();
    }

    Ok(())
}
