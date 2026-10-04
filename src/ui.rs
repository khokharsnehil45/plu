use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use crate::batch::{self, print_batch_summary};
use crate::loader::PdfLoader;
use crate::unloader::PdfUnloader;

// ANSI Color Escape Sequences
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const CYAN: &str = "\x1b[36m";
pub const BRIGHT_CYAN: &str = "\x1b[1;36m";
pub const GREEN: &str = "\x1b[32m";
pub const BRIGHT_GREEN: &str = "\x1b[1;32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BRIGHT_YELLOW: &str = "\x1b[1;33m";
pub const BRIGHT_MAGENTA: &str = "\x1b[1;35m";
pub const RED: &str = "\x1b[1;31m";
pub const BRIGHT_WHITE: &str = "\x1b[1;37m";
pub const GRAY: &str = "\x1b[90m";

const DIVIDER: &str = "────────────────────────────────────────────────────────────";

/// Prints the signature welcome banner for PLU Loader and Unloader
pub fn print_banner() {
    println!("{BRIGHT_CYAN}PLU{RESET} {YELLOW}v1.0.0{RESET} {BRIGHT_MAGENTA}•{RESET} {BRIGHT_WHITE}PDF Loader & Unloader{RESET}");
    println!("{CYAN}{}{RESET}", DIVIDER);
    println!(" {BRIGHT_GREEN}Welcome to PLU Engine!{RESET}");
    println!(" {BRIGHT_WHITE}Ready to load PDF content and unload .txt files page by page.{RESET}");
    println!("{CYAN}{}{RESET}", DIVIDER);
}

/// Prints a horizontal divider line
pub fn print_divider() {
    println!("{CYAN}{}{RESET}", DIVIDER);
}

/// Prints a colored horizontal divider line
pub fn print_divider_color(color: &str) {
    println!("{}{}{}", color, DIVIDER, RESET);
}

/// Prints a key-value row with clean alignment
pub fn print_kv(key: &str, value: &str) {
    print_kv_colored(key, value, YELLOW, BRIGHT_GREEN);
}

/// Prints a key-value row with custom colors
pub fn print_kv_colored(key: &str, value: &str, key_color: &str, val_color: &str) {
    println!(" {key_color}{:<20}{RESET} : {val_color}{}{RESET}", key, value);
}

/// Prints a formatted menu option with title and description
pub fn print_menu_item(tag: &str, title: &str, desc: &str) {
    println!(
        " {BRIGHT_YELLOW}[{}]{RESET} {BRIGHT_WHITE}{:<20}{RESET} {GRAY}{}{RESET}",
        tag, title, desc
    );
}

/// Runs the interactive CLI UI focused strictly on Loading and Unloading
pub fn run_interactive_ui() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        println!();
        print_banner();
        print_menu_item("1", "Load & Unload PDF", "Extract page-by-page & unload into .txt (Single / Batch)");
        print_menu_item("2", "Exit", "Quit PLU Engine");
        print_divider();
        print!(" {BRIGHT_MAGENTA}Select an option [1-2]:{RESET} ");
        io::stdout().flush()?;

        let mut choice = String::new();
        if reader.read_line(&mut choice)? == 0 {
            break;
        }
        let choice = choice.trim();

        match choice {
            "1" => ui_load_and_unload(&mut reader)?,
            "2" | "q" | "exit" => {
                println!();
                print_divider_color(BRIGHT_GREEN);
                println!(" {BRIGHT_GREEN}Exiting PLU. Have a great day!{RESET}");
                print_divider_color(BRIGHT_GREEN);
                break;
            }
            _ => {
                println!();
                print_divider_color(RED);
                println!(" {RED}Invalid choice. Please select 1 or 2.{RESET}");
                print_divider_color(RED);
            }
        }
    }

    Ok(())
}

fn prompt_input<R: BufRead>(reader: &mut R, prompt: &str) -> Result<String> {
    print!(" {BRIGHT_MAGENTA}{}{RESET} ", prompt);
    io::stdout().flush()?;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Single unified feature: Load & Unload PDF (Single File or Batch Directory)
fn ui_load_and_unload<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}LOAD & UNLOAD PDF (PAGE-BY-PAGE STREAMING){RESET}");
    print_divider();

    let input_str = prompt_input(reader, "Enter input PDF path (file or directory):")?;
    if input_str.is_empty() {
        println!(" {YELLOW}Operation cancelled.{RESET}");
        return Ok(());
    }

    let input_path = PathBuf::from(&input_str);
    if !input_path.exists() {
        println!(" {RED}Error: Path does not exist: {}{RESET}", input_str);
        return Ok(());
    }

    let threads_input = prompt_input(reader, "Enter worker threads [Enter for auto]:")?;
    let threads: Option<usize> = threads_input.parse().ok();

    if input_path.is_dir() {
        // Batch Load & Unload Directory
        let default_out = input_path
            .file_name()
            .map(|s| format!("{}_txt", s.to_string_lossy()))
            .unwrap_or_else(|| "batch_txt_output".to_string());

        let out_prompt = format!("Enter unload output directory [default: {}]:", default_out);
        let out_str = prompt_input(reader, &out_prompt)?;
        let output_dir = if out_str.is_empty() {
            input_path.parent().unwrap_or(&input_path).join(default_out)
        } else {
            PathBuf::from(out_str)
        };

        println!();
        print_divider();
        println!(" {BRIGHT_CYAN}STARTING BATCH EXTRACTION & UNLOAD PIPELINE{RESET}");
        print_divider();
        print_kv("Input Directory", &input_path.display().to_string());
        print_kv("Unload Output Dir", &output_dir.display().to_string());
        print_divider();

        match batch::run_batch_load(&input_path, &output_dir, threads, crate::types::DumpFormat::Text) {
            Ok(stats) => {
                print_batch_summary(&stats, "LOAD & UNLOAD", &output_dir);
            }
            Err(e) => {
                println!(" {RED}Batch Error: {e}{RESET}");
            }
        }
    } else {
        // Single File Load & Unload
        let default_output = input_path
            .file_stem()
            .map(|s| format!("{}.txt", s.to_string_lossy()))
            .unwrap_or_else(|| "output.txt".to_string());

        let out_prompt = format!("Enter unload destination path [default: {}]:", default_output);
        let out_input = prompt_input(reader, &out_prompt)?;
        let output_path = if out_input.is_empty() {
            input_path.parent().unwrap_or(&input_path).join(default_output)
        } else {
            PathBuf::from(out_input)
        };

        println!();
        print_divider();
        println!(" {BRIGHT_CYAN}STARTING EXTRACTION & UNLOAD PIPELINE{RESET}");
        print_divider();
        print_kv("Input PDF", &input_path.display().to_string());
        print_kv("Unload Output File", &output_path.display().to_string());

        let start_time = Instant::now();
        let loader = match PdfLoader::load_file(&input_path) {
            Ok(l) => l,
            Err(e) => {
                println!(" {RED}Loader Error: {}{RESET}", e);
                return Ok(());
            }
        };

        let total_pages = loader.page_count();
        let meta = loader.metadata().clone();
        let format = PdfUnloader::detect_format(&output_path);

        print_kv("Target Format", &format.to_string());
        print_kv("Discovered Pages", &total_pages.to_string());
        if let Some(ref title) = meta.title {
            print_kv("Document Title", title);
        }
        if let Some(ref author) = meta.author {
            print_kv("Document Author", author);
        }
        print_divider();

        let pb = ProgressBar::new(total_pages as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} pages ({per_sec}) {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("█▓▒░ "),
        );
        pb.set_message("Extracting pages");

        let effective_threads = threads
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        let channel_cap = (effective_threads * 4).max(32);
        let (tx, rx) = crossbeam_channel::bounded(channel_cap);
        let unloader_path = output_path.clone();
        let unloader_meta = meta.clone();
        let pb_clone = pb.clone();

        let unloader_handle = std::thread::spawn(move || {
            PdfUnloader::dump_stream_with_progress(
                rx,
                &unloader_meta,
                total_pages,
                unloader_path,
                format,
                Some(pb_clone),
            )
        });

        loader.stream_pages_parallel(tx, threads)?;

        let stats = match unloader_handle.join() {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                println!(" {RED}Unloader Error: {}{RESET}", e);
                return Ok(());
            }
            Err(_) => {
                println!(" {RED}Unloader thread panicked!{RESET}");
                return Ok(());
            }
        };

        let elapsed = start_time.elapsed();
        let pps = if elapsed.as_secs_f64() > 0.0 {
            stats.pages_processed as f64 / elapsed.as_secs_f64()
        } else {
            stats.pages_processed as f64
        };

        println!();
        print_divider_color(BRIGHT_GREEN);
        println!(" {BRIGHT_GREEN}PIPELINE COMPLETED SUCCESSFULLY{RESET}");
        print_divider_color(BRIGHT_GREEN);
        print_kv_colored("Output File", &output_path.display().to_string(), YELLOW, BRIGHT_WHITE);
        print_kv_colored("Pages Dumped", &stats.pages_processed.to_string(), YELLOW, BRIGHT_GREEN);
        print_kv_colored("Total Chars", &stats.total_chars.to_string(), YELLOW, BRIGHT_WHITE);
        print_kv_colored("Total Words", &stats.total_words.to_string(), YELLOW, BRIGHT_WHITE);
        print_kv_colored(
            "Payload Size",
            &format!("{:.2} KB ({} bytes)", stats.bytes_written as f64 / 1024.0, stats.bytes_written),
            YELLOW,
            BRIGHT_WHITE,
        );
        print_kv_colored("Throughput", &format!("{:.1} pages/sec", pps), YELLOW, BRIGHT_GREEN);
        print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
        print_divider_color(BRIGHT_GREEN);
    }

    Ok(())
}
