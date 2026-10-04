use anyhow::Result;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use crate::loader::PdfLoader;
use crate::ops::PdfOps;
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

/// Prints the exact signature welcome banner requested
pub fn print_banner() {
    println!("{BRIGHT_CYAN}PLU{RESET} {YELLOW}v1.0.0{RESET} {BRIGHT_MAGENTA}•{RESET} {BRIGHT_WHITE}PDF Processor{RESET}");
    println!("{CYAN}{}{RESET}", DIVIDER);
    println!(" {BRIGHT_GREEN}Welcome to PLU Engine!{RESET}");
    println!(" {BRIGHT_WHITE}Ready to compress, merge, split, and extract text from your documents.{RESET}");
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
        " {BRIGHT_YELLOW}[{}]{RESET} {BRIGHT_WHITE}{:<16}{RESET} {GRAY}{}{RESET}",
        tag, title, desc
    );
}

/// Runs the interactive CLI UI
pub fn run_interactive_ui() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        println!();
        print_banner();
        print_menu_item("1", "Extract Text", "Extract page-by-page text & dump (.plu, .txt, .json)");
        print_menu_item("2", "Compress PDF", "Re-compress PDF streams with FlateDecode optimization");
        print_menu_item("3", "Split PDF", "Split document by page ranges or into single pages");
        print_menu_item("4", "Merge PDFs", "Combine multiple PDF files into a single document");
        print_menu_item("5", "Inspect .plu", "Unload & verify high-speed .plu container (O(1) lookup)");
        print_menu_item("6", "Unpack .plu", "Unpack all pages from .plu into a folder");
        print_menu_item("7", "Exit", "Quit PLU Engine");
        print_divider();
        print!(" {BRIGHT_MAGENTA}Select an option [1-7]:{RESET} ");
        io::stdout().flush()?;

        let mut choice = String::new();
        if reader.read_line(&mut choice)? == 0 {
            break;
        }
        let choice = choice.trim();

        match choice {
            "1" => ui_load_and_dump(&mut reader)?,
            "2" => ui_compress_pdf(&mut reader)?,
            "3" => ui_split_pdf(&mut reader)?,
            "4" => ui_merge_pdfs(&mut reader)?,
            "5" => ui_unload_inspect(&mut reader)?,
            "6" => ui_unpack_directory(&mut reader)?,
            "7" | "q" | "exit" => {
                println!();
                print_divider_color(BRIGHT_GREEN);
                println!(" {BRIGHT_GREEN}Exiting PLU. Have a great day!{RESET}");
                print_divider_color(BRIGHT_GREEN);
                break;
            }
            _ => {
                println!();
                print_divider_color(RED);
                println!(" {RED}Invalid choice. Please select an option from 1 to 7.{RESET}");
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

/// Option 1: Extract Text & Dump
fn ui_load_and_dump<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}PAGE-BY-PAGE EXTRACTION & DUMP PIPELINE{RESET}");
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
    println!(" {BRIGHT_CYAN}STARTING EXTRACTION{RESET}");
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
    print_kv_colored("Pages Processed", &stats.pages_processed.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Total Chars", &stats.total_chars.to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Total Words", &stats.total_words.to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Payload Size", &format!("{:.2} KB ({} bytes)", stats.bytes_written as f64 / 1024.0, stats.bytes_written), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Throughput", &format!("{:.1} pages/sec", pps), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

/// Option 2: Compress PDF
fn ui_compress_pdf<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}COMPRESS PDF STREAMS{RESET}");
    print_divider();

    let pdf_input = prompt_input(reader, "Enter input PDF path:")?;
    if pdf_input.is_empty() {
        return Ok(());
    }
    let in_path = PathBuf::from(&pdf_input);
    if !in_path.exists() {
        println!(" {RED}Error: File does not exist: {}{RESET}", pdf_input);
        return Ok(());
    }

    let default_output = in_path
        .file_stem()
        .map(|s| format!("{}_compressed.pdf", s.to_string_lossy()))
        .unwrap_or_else(|| "compressed.pdf".to_string());

    let out_prompt = format!("Enter destination path [default: {}]:", default_output);
    let out_input = prompt_input(reader, &out_prompt)?;
    let out_path = if out_input.is_empty() {
        PathBuf::from(default_output)
    } else {
        PathBuf::from(out_input)
    };

    println!();
    println!(" {CYAN}Compressing PDF stream objects...{RESET}");
    let start = Instant::now();
    let (orig_size, new_size) = PdfOps::compress_pdf(&in_path, &out_path)?;
    let elapsed = start.elapsed();

    let saved_bytes = orig_size.saturating_sub(new_size);
    let ratio = if orig_size > 0 {
        (saved_bytes as f64 / orig_size as f64) * 100.0
    } else {
        0.0
    };

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}PDF COMPRESSED SUCCESSFULLY{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Original Size", &format!("{:.2} KB ({} bytes)", orig_size as f64 / 1024.0, orig_size), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Compressed Size", &format!("{:.2} KB ({} bytes)", new_size as f64 / 1024.0, new_size), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Reduction", &format!("{:.1}% saved ({} bytes)", ratio, saved_bytes), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Output File", &out_path.display().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

/// Option 3: Split PDF
fn ui_split_pdf<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}SPLIT PDF DOCUMENT{RESET}");
    print_divider();

    let pdf_input = prompt_input(reader, "Enter input PDF path:")?;
    if pdf_input.is_empty() {
        return Ok(());
    }
    let in_path = PathBuf::from(&pdf_input);
    if !in_path.exists() {
        println!(" {RED}Error: File does not exist: {}{RESET}", pdf_input);
        return Ok(());
    }

    println!(" [1] Extract specific page range (e.g. 1-3, 5)");
    println!(" [2] Split entire PDF into individual page files");
    let mode = prompt_input(reader, "Choose mode [1/2]:")?;

    match mode.trim() {
        "1" => {
            let pages_str = prompt_input(reader, "Enter pages (e.g. 1,2,3 or 1-4):")?;
            let mut pages = Vec::new();
            for part in pages_str.split(',') {
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

            let out_prompt = "Enter output PDF path [default: split.pdf]:";
            let out_input = prompt_input(reader, out_prompt)?;
            let out_path = if out_input.is_empty() {
                PathBuf::from("split.pdf")
            } else {
                PathBuf::from(out_input)
            };

            let start = Instant::now();
            let count = PdfOps::split_pages(&in_path, &pages, &out_path)?;
            let elapsed = start.elapsed();

            println!();
            print_divider_color(BRIGHT_GREEN);
            println!(" {BRIGHT_GREEN}SPLIT COMPLETED{RESET}");
            print_divider_color(BRIGHT_GREEN);
            print_kv_colored("Extracted Pages", &count.to_string(), YELLOW, BRIGHT_GREEN);
            print_kv_colored("Output File", &out_path.display().to_string(), YELLOW, BRIGHT_WHITE);
            print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
            print_divider_color(BRIGHT_GREEN);
        }
        "2" => {
            let default_dir = in_path
                .file_stem()
                .map(|s| format!("{}_pages", s.to_string_lossy()))
                .unwrap_or_else(|| "split_pages".to_string());
            let dir_prompt = format!("Enter destination folder [default: {}]:", default_dir);
            let dir_input = prompt_input(reader, &dir_prompt)?;
            let out_dir = if dir_input.is_empty() {
                PathBuf::from(default_dir)
            } else {
                PathBuf::from(dir_input)
            };

            let start = Instant::now();
            let count = PdfOps::split_all(&in_path, &out_dir)?;
            let elapsed = start.elapsed();

            println!();
            print_divider_color(BRIGHT_GREEN);
            println!(" {BRIGHT_GREEN}SPLIT COMPLETED{RESET}");
            print_divider_color(BRIGHT_GREEN);
            print_kv_colored("Total Single-Page PDFs", &count.to_string(), YELLOW, BRIGHT_GREEN);
            print_kv_colored("Output Directory", &out_dir.display().to_string(), YELLOW, BRIGHT_WHITE);
            print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
            print_divider_color(BRIGHT_GREEN);
        }
        _ => println!(" {YELLOW}Invalid split mode selected.{RESET}"),
    }

    Ok(())
}

/// Option 4: Merge PDFs
fn ui_merge_pdfs<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}MERGE PDF DOCUMENTS{RESET}");
    print_divider();

    println!(" {GRAY}Enter PDF paths separated by spaces or commas:{RESET}");
    let paths_input = prompt_input(reader, "PDF paths to merge:")?;
    let inputs: Vec<PathBuf> = paths_input
        .split([',', ' '])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();

    if inputs.len() < 2 {
        println!(" {YELLOW}Please provide at least 2 PDF files to merge.{RESET}");
        return Ok(());
    }

    let out_input = prompt_input(reader, "Enter output path [default: merged.pdf]:")?;
    let out_path = if out_input.is_empty() {
        PathBuf::from("merged.pdf")
    } else {
        PathBuf::from(out_input)
    };

    println!();
    println!(" {CYAN}Merging {} documents...{RESET}", inputs.len());
    let start = Instant::now();
    let total_pages = PdfOps::merge_pdfs(&inputs, &out_path)?;
    let elapsed = start.elapsed();

    println!();
    print_divider_color(BRIGHT_GREEN);
    println!(" {BRIGHT_GREEN}PDFS MERGED SUCCESSFULLY{RESET}");
    print_divider_color(BRIGHT_GREEN);
    print_kv_colored("Files Merged", &inputs.len().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Total Merged Pages", &total_pages.to_string(), YELLOW, BRIGHT_GREEN);
    print_kv_colored("Output File", &out_path.display().to_string(), YELLOW, BRIGHT_WHITE);
    print_kv_colored("Elapsed Time", &format!("{:.2?}", elapsed), YELLOW, BRIGHT_GREEN);
    print_divider_color(BRIGHT_GREEN);

    Ok(())
}

/// Option 5: Inspect .plu Container
fn ui_unload_inspect<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}UNLOAD & INSPECT .PLU CONTAINER{RESET}");
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
    println!(" {BRIGHT_GREEN}CONTAINER METADATA VERIFIED{RESET}");
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

    let show_pages = prompt_input(reader, "Display per-page breakdown? [y/N]:")?;
    if show_pages.eq_ignore_ascii_case("y") {
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
    }

    Ok(())
}

/// Option 6: Unpack .plu to Directory
fn ui_unpack_directory<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_divider();
    println!(" {BRIGHT_CYAN}UNPACK .PLU TO DIRECTORY{RESET}");
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
