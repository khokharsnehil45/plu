use anyhow::Result;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

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
    println!(" {BRIGHT_WHITE}Ready to load PDF content and unload .plu files page by page.{RESET}");
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
        " {BRIGHT_YELLOW}[{}]{RESET} {BRIGHT_WHITE}{:<18}{RESET} {GRAY}{}{RESET}",
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
        print_menu_item("1", "Load & Dump PDF", "Extract page-by-page text & dump into .plu");
        print_menu_item("2", "Unload .plu File", "Read & inspect .plu container page by page");
        print_menu_item("3", "Read Single Page", "O(1) random-access page reader from .plu");
        print_menu_item("4", "Unpack .plu Pages", "Unpack all pages from .plu into a folder");
        print_menu_item("5", "Exit", "Quit PLU Engine");
        print_divider();
        print!(" {BRIGHT_MAGENTA}Select an option [1-5]:{RESET} ");
        io::stdout().flush()?;

        let mut choice = String::new();
        if reader.read_line(&mut choice)? == 0 {
            break;
        }
        let choice = choice.trim();

        match choice {
            "1" => ui_load_and_dump(&mut reader)?,
            "2" => ui_unload_inspect(&mut reader)?,
            "3" => ui_read_single_page(&mut reader)?,
            "4" => ui_unpack_directory(&mut reader)?,
            "5" | "q" | "exit" => {
                println!();
                print_divider_color(BRIGHT_GREEN);
                println!(" {BRIGHT_GREEN}Exiting PLU. Have a great day!{RESET}");
                print_divider_color(BRIGHT_GREEN);
                break;
            }
            _ => {
                println!();
                print_divider_color(RED);
                println!(" {RED}Invalid choice. Please select an option from 1 to 5.{RESET}");
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

/// Option 1: Load PDF & Dump (Concurrent Page-by-Page Extraction)
fn ui_load_and_dump<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}LOAD PDF & DUMP PAGE-BY-PAGE{RESET}");
    print_divider();

    let pdf_input = prompt_input(reader, "Enter input PDF path:")?;
    if pdf_input.is_empty() {
        println!(" {YELLOW}Operation cancelled.{RESET}");
        return Ok(());
    }
    let pdf_path = PathBuf::from(&pdf_input);
    if !pdf_path.exists() {
        println!(" {RED}Error: File does not exist: {}{RESET}", pdf_input);
        return Ok(());
    }

    let default_output = pdf_path
        .file_stem()
        .map(|s| format!("{}.plu", s.to_string_lossy()))
        .unwrap_or_else(|| "output.plu".to_string());

    let out_prompt = format!("Enter output path [default: {}]:", default_output);
    let out_input = prompt_input(reader, &out_prompt)?;
    let output_path = if out_input.is_empty() {
        PathBuf::from(default_output)
    } else {
        PathBuf::from(out_input)
    };

    let threads_input = prompt_input(reader, "Enter worker threads [Enter for auto]:")?;
    let threads: Option<usize> = threads_input.parse().ok();

    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}STARTING EXTRACTION PIPELINE{RESET}");
    print_divider();
    print_kv("Input PDF", &pdf_path.display().to_string());
    print_kv("Output File", &output_path.display().to_string());

    let start_time = Instant::now();
    let loader = match PdfLoader::load_file(&pdf_path) {
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

    let channel_cap = (threads.unwrap_or(8) * 4).max(32);
    let (tx, rx) = crossbeam_channel::bounded(channel_cap);
    let unloader_path = output_path.clone();
    let unloader_meta = meta.clone();

    let unloader_handle = std::thread::spawn(move || {
        PdfUnloader::dump_stream(rx, &unloader_meta, total_pages, unloader_path, format)
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
    print_kv_colored("Payload Size", &format!("{:.2} KB ({} bytes)", stats.bytes_written as f64 / 1024.0, stats.bytes_written), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Throughput", &format!("{:.1} pages/sec", pps), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

/// Option 2: Unload & Inspect .plu Container Page-by-Page
fn ui_unload_inspect<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}UNLOAD & INSPECT .PLU CONTAINER PAGE-BY-PAGE{RESET}");
    print_divider();

    let plu_input = prompt_input(reader, "Enter .plu container path:")?;
    if plu_input.is_empty() {
        return Ok(());
    }
    let plu_path = PathBuf::from(&plu_input);
    if !plu_path.exists() {
        println!(" {RED}Error: File does not exist: {}{RESET}", plu_input);
        return Ok(());
    }

    let start = Instant::now();
    let doc = match PdfUnloader::unload_file(&plu_path) {
        Ok(d) => d,
        Err(e) => {
            println!(" {RED}Error reading .plu: {}{RESET}", e);
            return Ok(());
        }
    };
    let elapsed = start.elapsed();

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}CONTAINER VERIFIED AND UNLOADED{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Container File", &plu_path.display().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Original Source", &doc.meta.source_path, YELLOW, BRIGHT_WHITE);
    print_kv_colored("Total Pages", &doc.meta.page_count.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Total Characters", &doc.meta.total_chars.to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Total Words", &doc.meta.total_words.to_string(), YELLOW, BRIGHT_WHITE);
    if let Some(ref t) = doc.meta.title {
        print_kv("Title", t);
    }
    if let Some(ref a) = doc.meta.author {
        print_kv("Author", a);
    }
    print_kv_colored("Verification Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    let show_pages = prompt_input(reader, "Display page-by-page inventory? [y/N]:")?;
    if show_pages.eq_ignore_ascii_case("y") {
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
    }

    Ok(())
}

/// Option 3: Read Single Page (O(1) Random Access)
fn ui_read_single_page<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}READ PAGE FROM .PLU (O(1) RANDOM ACCESS){RESET}");
    print_divider();

    let plu_input = prompt_input(reader, "Enter .plu container path:")?;
    let plu_path = PathBuf::from(&plu_input);
    if !plu_path.exists() {
        println!(" {RED}Error: File does not exist: {}{RESET}", plu_input);
        return Ok(());
    }

    let page_str = prompt_input(reader, "Enter page number to read:")?;
    let page_num: u32 = match page_str.parse() {
        Ok(n) if n >= 1 => n,
        _ => {
            println!(" {RED}Invalid page number.{RESET}");
            return Ok(());
        }
    };

    let start = Instant::now();
    let page = match PdfUnloader::unload_single_page(&plu_path, page_num) {
        Ok(p) => p,
        Err(e) => {
            println!(" {RED}Error: {}{RESET}", e);
            return Ok(());
        }
    };
    let elapsed = start.elapsed();

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}PAGE {} CONTENT{RESET}", page.page_num);
    print_divider_color(BRIGHT_GREEN);
    print_kv("Dimensions", &format!("{}x{} pt", page.width, page.height));
    print_kv("Characters", &page.char_count.to_string());
    print_kv("Words", &page.word_count.to_string());
    print_kv_colored("O(1) Lookup Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider();
    println!("{}", page.text.trim());
    print_divider();

    Ok(())
}

/// Option 4: Unpack .plu to Directory
fn ui_unpack_directory<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}UNPACK .PLU PAGES TO DIRECTORY{RESET}");
    print_divider();

    let plu_input = prompt_input(reader, "Enter .plu container path:")?;
    let plu_path = PathBuf::from(&plu_input);
    if !plu_path.exists() {
        println!(" {RED}Error: File does not exist: {}{RESET}", plu_input);
        return Ok(());
    }

    let default_dir = plu_path
        .file_stem()
        .map(|s| format!("{}_pages", s.to_string_lossy()))
        .unwrap_or_else(|| "extracted_pages".to_string());

    let dir_prompt = format!("Enter destination folder [default: {}]:", default_dir);
    let dir_input = prompt_input(reader, &dir_prompt)?;
    let out_dir = if dir_input.is_empty() {
        PathBuf::from(default_dir)
    } else {
        PathBuf::from(dir_input)
    };

    let start = Instant::now();
    let count = match PdfUnloader::unpack_to_directory(&plu_path, &out_dir) {
        Ok(c) => c,
        Err(e) => {
            println!(" {RED}Unpack Error: {}{RESET}", e);
            return Ok(());
        }
    };
    let elapsed = start.elapsed();

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}UNPACK COMPLETED{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Unpacked Pages", &count.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Destination", &out_dir.display().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}
