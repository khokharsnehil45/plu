use anyhow::{bail, Context, Result};
use clap::Parser;
use plu::loader::PdfLoader;
use plu::types::{DumpFormat, PageData, PluDocument};
use plu::ui::{
    print_banner, print_divider, print_divider_color, print_kv, print_kv_colored,
    run_interactive_ui, BRIGHT_CYAN, BRIGHT_GREEN, RED, RESET, YELLOW,
};
use plu::unloader::PdfUnloader;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "plu",
    author = "High Performance PDF Systems",
    version = "1.0.0",
    about = "High-performance PDF Loader & Unloader: Page-by-page extraction and .plu container reader"
)]
struct Args {
    /// Path to input PDF file to load (page-by-page extraction)
    #[arg(short = 'l', long = "load", value_name = "FILE_PATH")]
    load: Option<PathBuf>,

    /// Path to file to dump / unload to, or .plu file to unload from
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

    /// Verbose output with detailed page-by-page inventory
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

    match (&args.load, &args.unload) {
        // Case 1: Both --load and --unload provided:
        // plu --load input.pdf --unload output.plu
        (Some(load_path), Some(unload_path)) => {
            handle_load_and_unload(load_path, unload_path, &args)
        }

        // Case 2: Only --load provided:
        // Default unload target is `<file_stem>.plu`
        (Some(load_path), None) => {
            let default_unload = load_path
                .file_stem()
                .map(|s| PathBuf::from(format!("{}.plu", s.to_string_lossy())))
                .unwrap_or_else(|| PathBuf::from("output.plu"));
            handle_load_and_unload(load_path, &default_unload, &args)
        }

        // Case 3: Only --unload provided:
        // Read and unload existing .plu dump file page by page
        (None, Some(unload_path)) => handle_unload_only(unload_path, &args),

        // Case 4: Neither provided -> Launch interactive CLI UI
        (None, None) => run_interactive_ui(),
    }
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
    print_kv_colored("Pages Dumped", &stats.pages_processed.to_string(), YELLOW, BRIGHT_GREEN);
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

/// Handles unloading / inspecting an existing .plu container page by page
fn handle_unload_only(plu_path: &Path, args: &Args) -> Result<()> {
    println!();
    print_banner();
    println!(" {YELLOW}Action{RESET}               : Unload & Read .plu Container Page-by-Page");
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
    println!(" {BRIGHT_GREEN}CONTAINER VERIFIED AND UNLOADED{RESET}");
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
        println!(" {BRIGHT_CYAN}PAGE-BY-PAGE INVENTORY{RESET}");
        print_divider();
        for p in &doc.pages {
            println!(
                " Page {:>4} : {:>6.1}x{:<6.1} pt | {:>6} chars | {:>6} words",
                p.page_num, p.width, p.height, p.char_count, p.word_count
            );
        }
        print_divider();
    } else {
        println!(" {YELLOW}Tip:{RESET} Use `--verbose` for page-by-page inventory, or `--page <N>` to read a page.");
        print_divider();
    }

    Ok(())
}
