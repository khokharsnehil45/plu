use anyhow::Result;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use crate::loader::PdfLoader;
use crate::unloader::PdfUnloader;

const UI_WIDTH: usize = 78;

/// Prints a top or bottom boundary line using '='
pub fn print_bar() {
    println!("{}", "=".repeat(UI_WIDTH));
}

/// Prints a row framed with '|' on both ends
pub fn print_row(text: &str) {
    let inner_width = UI_WIDTH.saturating_sub(4);
    if text.len() <= inner_width {
        let padding = " ".repeat(inner_width - text.len());
        println!("| {} {} |", text, padding);
    } else {
        // Truncate or wrap safely
        let truncated = &text[..inner_width];
        println!("| {} |", truncated);
    }
}

/// Prints a centered header row framed with '|'
pub fn print_header(title: &str) {
    let inner_width = UI_WIDTH.saturating_sub(4);
    if title.len() >= inner_width {
        let truncated = &title[..inner_width];
        println!("| {} |", truncated);
    } else {
        let total_pad = inner_width - title.len();
        let left_pad = " ".repeat(total_pad / 2);
        let right_pad = " ".repeat(total_pad - left_pad.len());
        println!("| {}{}{} |", left_pad, title, right_pad);
    }
}

/// Prints a key-value row: | Key             | Value                         |
pub fn print_kv(key: &str, value: &str) {
    let col1_width = 20;
    let col2_width = UI_WIDTH.saturating_sub(col1_width + 7);

    let key_pad = if key.len() < col1_width {
        " ".repeat(col1_width - key.len())
    } else {
        String::new()
    };
    let key_str = if key.len() > col1_width {
        &key[..col1_width]
    } else {
        key
    };

    let val_str = if value.len() > col2_width {
        &value[..col2_width]
    } else {
        value
    };
    let val_pad = if val_str.len() < col2_width {
        " ".repeat(col2_width - val_str.len())
    } else {
        String::new()
    };

    println!("| {}{} | {}{} |", key_str, key_pad, val_str, val_pad);
}

/// Runs the interactive CLI UI using strictly '=' and '|'
pub fn run_interactive_ui() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        println!();
        print_bar();
        print_header("PLU: PDF LOADER & UNLOADER INTERACTIVE UI");
        print_bar();
        print_row("[1] Load PDF & Dump (Concurrent Page-by-Page Extraction)");
        print_row("[2] Unload & Inspect .plu Container File");
        print_row("[3] Read Single Page from .plu (O(1) Random Access)");
        print_row("[4] Unpack .plu Pages to Directory");
        print_row("[5] Exit");
        print_bar();
        print!("| Select an option [1-5]: ");
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
                print_bar();
                print_header("Exiting PLU. Goodbye!");
                print_bar();
                break;
            }
            _ => {
                println!();
                print_bar();
                print_row("Invalid selection. Please choose an option between 1 and 5.");
                print_bar();
            }
        }
    }

    Ok(())
}

fn prompt_input<R: BufRead>(reader: &mut R, prompt: &str) -> Result<String> {
    print!("| {} ", prompt);
    io::stdout().flush()?;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn ui_load_and_dump<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_bar();
    print_header("LOAD PDF & DUMP PIPELINE");
    print_bar();

    let pdf_input = prompt_input(reader, "Enter input PDF path:")?;
    if pdf_input.is_empty() {
        print_row("Operation cancelled: empty PDF path.");
        print_bar();
        return Ok(());
    }
    let pdf_path = PathBuf::from(&pdf_input);
    if !pdf_path.exists() {
        print_row(&format!("Error: File does not exist: {}", pdf_input));
        print_bar();
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
    print_bar();
    print_header("STARTING CONCURRENT PIPELINE");
    print_bar();
    print_kv("Input PDF", &pdf_path.display().to_string());
    print_kv("Output File", &output_path.display().to_string());

    let start_time = Instant::now();
    let loader = match PdfLoader::load_file(&pdf_path) {
        Ok(l) => l,
        Err(e) => {
            print_row(&format!("Loader Error: {}", e));
            print_bar();
            return Ok(());
        }
    };

    let total_pages = loader.page_count();
    let meta = loader.metadata().clone();
    let format = PdfUnloader::detect_format(&output_path);

    print_kv("Target Format", &format.to_string());
    print_kv("Discovered Pages", &total_pages.to_string());
    if let Some(ref title) = meta.title {
        print_kv("Title", title);
    }
    if let Some(ref author) = meta.author {
        print_kv("Author", author);
    }
    print_bar();

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
            print_row(&format!("Unloader Error: {}", e));
            print_bar();
            return Ok(());
        }
        Err(_) => {
            print_row("Unloader thread panicked!");
            print_bar();
            return Ok(());
        }
    };

    let elapsed = start_time.elapsed();
    let pps = if elapsed.as_secs_f64() > 0.0 {
        stats.pages_processed as f64 / elapsed.as_secs_f64()
    } else {
        stats.pages_processed as f64
    };

    print_bar();
    print_header("PIPELINE COMPLETED SUCCESSFULLY");
    print_bar();
    print_kv("Pages Processed", &stats.pages_processed.to_string());
    print_kv("Total Chars", &stats.total_chars.to_string());
    print_kv("Total Words", &stats.total_words.to_string());
    print_kv("Bytes Written", &format!("{} bytes", stats.bytes_written));
    print_kv("Throughput", &format!("{:.1} pages/sec", pps));
    print_kv("Elapsed Time", &format!("{:.2?}", elapsed));
    print_bar();

    Ok(())
}

fn ui_unload_inspect<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_bar();
    print_header("UNLOAD & INSPECT .PLU CONTAINER");
    print_bar();

    let plu_input = prompt_input(reader, "Enter .plu container path:")?;
    if plu_input.is_empty() {
        print_row("Operation cancelled: empty path.");
        print_bar();
        return Ok(());
    }

    let plu_path = PathBuf::from(&plu_input);
    if !plu_path.exists() {
        print_row(&format!("Error: File does not exist: {}", plu_input));
        print_bar();
        return Ok(());
    }

    let start = Instant::now();
    let doc = match PdfUnloader::unload_file(&plu_path) {
        Ok(d) => d,
        Err(e) => {
            print_row(&format!("Error reading .plu: {}", e));
            print_bar();
            return Ok(());
        }
    };
    let elapsed = start.elapsed();

    println!();
    print_bar();
    print_header("CONTAINER METADATA VERIFIED");
    print_bar();
    print_kv("Container File", &plu_path.display().to_string());
    print_kv("Original Source", &doc.meta.source_path);
    print_kv("Total Pages", &doc.meta.page_count.to_string());
    print_kv("Total Characters", &doc.meta.total_chars.to_string());
    print_kv("Total Words", &doc.meta.total_words.to_string());
    if let Some(ref t) = doc.meta.title {
        print_kv("Title", t);
    }
    if let Some(ref a) = doc.meta.author {
        print_kv("Author", a);
    }
    print_kv("Verification Time", &format!("{:.2?}", elapsed));
    print_bar();

    let show_pages = prompt_input(reader, "Display per-page breakdown? [y/N]:")?;
    if show_pages.eq_ignore_ascii_case("y") {
        print_bar();
        print_header("PAGE INVENTORY");
        print_bar();
        for p in &doc.pages {
            let row = format!(
                "Page {:>4} | {:>6.1}x{:<6.1} pt | {:>6} chars | {:>6} words",
                p.page_num, p.width, p.height, p.char_count, p.word_count
            );
            print_row(&row);
        }
        print_bar();
    }

    Ok(())
}

fn ui_read_single_page<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_bar();
    print_header("O(1) RANDOM ACCESS PAGE READER");
    print_bar();

    let plu_input = prompt_input(reader, "Enter .plu container path:")?;
    let plu_path = PathBuf::from(&plu_input);
    if !plu_path.exists() {
        print_row(&format!("Error: File does not exist: {}", plu_input));
        print_bar();
        return Ok(());
    }

    let page_str = prompt_input(reader, "Enter page number:")?;
    let page_num: u32 = match page_str.parse() {
        Ok(n) if n >= 1 => n,
        _ => {
            print_row("Invalid page number.");
            print_bar();
            return Ok(());
        }
    };

    let start = Instant::now();
    let page = match PdfUnloader::unload_single_page(&plu_path, page_num) {
        Ok(p) => p,
        Err(e) => {
            print_row(&format!("Error: {}", e));
            print_bar();
            return Ok(());
        }
    };
    let elapsed = start.elapsed();

    println!();
    print_bar();
    print_header(&format!("PAGE {} CONTENT", page.page_num));
    print_bar();
    print_kv("Dimensions", &format!("{}x{} pt", page.width, page.height));
    print_kv("Characters", &page.char_count.to_string());
    print_kv("Words", &page.word_count.to_string());
    print_kv("Lookup Time", &format!("{:.2?}", elapsed));
    print_bar();

    for line in page.text.lines() {
        print_row(line);
    }
    print_bar();

    Ok(())
}

fn ui_unpack_directory<R: BufRead>(reader: &mut R) -> Result<()> {
    println!();
    print_bar();
    print_header("UNPACK .PLU TO DIRECTORY");
    print_bar();

    let plu_input = prompt_input(reader, "Enter .plu container path:")?;
    let plu_path = PathBuf::from(&plu_input);
    if !plu_path.exists() {
        print_row(&format!("Error: File does not exist: {}", plu_input));
        print_bar();
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
            print_row(&format!("Unpack Error: {}", e));
            print_bar();
            return Ok(());
        }
    };
    let elapsed = start.elapsed();

    println!();
    print_bar();
    print_header("UNPACK COMPLETED");
    print_bar();
    print_kv("Unpacked Pages", &count.to_string());
    print_kv("Destination", &out_dir.display().to_string());
    print_kv("Elapsed Time", &format!("{:.2?}", elapsed));
    print_bar();

    Ok(())
}
