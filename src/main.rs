use anyhow::{bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use plu::batch::{self, print_batch_summary};
use plu::loader::PdfLoader;
use plu::types::DumpFormat;
use plu::ui::{
    print_banner, print_divider, print_divider_color, print_kv, print_kv_colored,
    run_interactive_ui, BRIGHT_GREEN, RED, RESET, YELLOW,
};
use plu::unloader::PdfUnloader;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "plu",
    author = "High Performance PDF Systems",
    version = "1.0.0",
    about = "High-performance PDF Loader & Unloader: Page-by-page extraction, .plu container reader, and batch processing"
)]
struct Args {
    /// Path to input PDF file or directory to load (page-by-page extraction)
    #[arg(short = 'l', long = "load", value_name = "PATH")]
    load: Option<PathBuf>,

    /// Path to file/directory to unload/dump to (default: <stem>.plu or <dir>_plu)
    #[arg(short = 'u', long = "unload", value_name = "PATH")]
    unload: Option<PathBuf>,

    /// Explicitly enable batch processing mode
    #[arg(short = 'b', long = "batch")]
    batch: bool,

    /// Number of worker threads for parallel extraction (default: CPU cores)
    #[arg(short = 't', long = "threads", value_name = "N")]
    threads: Option<usize>,

    /// Force output format when dumping (plu, txt, json, jsonl)
    #[arg(short = 'f', long = "format", value_name = "FORMAT")]
    format: Option<String>,

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
            if load_path.is_dir() || args.batch {
                let fmt = parse_format(args.format.as_deref())?;
                let stats = batch::run_batch_load(load_path, unload_path, args.threads, fmt)?;
                print_batch_summary(&stats, "LOAD & UNLOAD", unload_path);
                Ok(())
            } else {
                handle_load_and_unload(load_path, unload_path, &args)
            }
        }

        // Case 2: Only --load provided (defaults unload target to <stem>.plu or <dir>_plu):
        (Some(load_path), None) => {
            if load_path.is_dir() || args.batch {
                let default_out = load_path
                    .parent()
                    .unwrap_or(load_path)
                    .join(format!("{}_plu", load_path.file_name().unwrap().to_string_lossy()));
                let fmt = parse_format(args.format.as_deref())?;
                let stats = batch::run_batch_load(load_path, &default_out, args.threads, fmt)?;
                print_batch_summary(&stats, "LOAD & UNLOAD", &default_out);
                Ok(())
            } else {
                let default_unload = load_path
                    .file_stem()
                    .map(|s| PathBuf::from(format!("{}.plu", s.to_string_lossy())))
                    .unwrap_or_else(|| PathBuf::from("output.plu"));
                handle_load_and_unload(load_path, &default_unload, &args)
            }
        }

        // Case 3: Only --unload provided without --load:
        (None, Some(_)) => {
            bail!("PLU is a unified pipeline holding Load & Unload together. Please specify --load:\n  plu --load input.pdf --unload output.plu\n  plu --load ./input_pdfs/ --unload ./output_plus/");
        }

        // Case 4: Neither provided -> Launch interactive CLI UI
        (None, None) => run_interactive_ui(),
    }
}

fn parse_format(fmt: Option<&str>) -> Result<DumpFormat> {
    if let Some(f) = fmt {
        match f.to_ascii_lowercase().as_str() {
            "plu" => Ok(DumpFormat::Plu),
            "txt" => Ok(DumpFormat::Text),
            "json" => Ok(DumpFormat::Json),
            "jsonl" => Ok(DumpFormat::JsonLines),
            other => bail!("Unknown format: '{other}'. Choose from: plu, txt, json, jsonl"),
        }
    } else {
        Ok(DumpFormat::Plu)
    }
}

/// Executes the concurrent loader -> unloader streaming pipeline with real-time progress bar
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
        parse_format(Some(fmt))?
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

    // Progress Bar
    let pb = ProgressBar::new(total_pages as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} pages ({per_sec}) {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("█▓▒░ "),
    );
    pb.set_message("Extracting pages");

    let channel_cap = (effective_threads * 4).max(32);
    let (tx, rx) = crossbeam_channel::bounded(channel_cap);

    let unloader_path = unload_path.to_path_buf();
    let unloader_meta = doc_meta.clone();
    let pb_clone = pb.clone();

    let unloader_handle = std::thread::Builder::new()
        .name("unloader".to_string())
        .spawn(move || {
            PdfUnloader::dump_stream_with_progress(
                rx,
                &unloader_meta,
                total_pages,
                unloader_path,
                dump_format,
                Some(pb_clone),
            )
        })
        .context("Failed to spawn Unloader thread")?;

    loader.stream_pages_parallel(tx, args.threads)?;

    let stats = unloader_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Unloader thread panicked"))??;

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
