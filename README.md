<div align="center">

# ⚡ PLU: High-Performance PDF Loader & Unloader

**A production-grade, concurrent PDF extraction and containerization engine written in Rust.**

[![CI](https://github.com/khokharsnehil45/plu/actions/workflows/ci.yml/badge.svg)](https://github.com/khokharsnehil45/plu/actions)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey.svg)]()

*Pure loading and unloading of PDF documents page-by-page with real-time progress bars, batch directory processing, and zero bloat.*

</div>

---

## ⚡ 1-Command Installation

### 🐧 Linux & 🍎 macOS
Open your terminal and run:
```bash
curl -fsSL https://raw.githubusercontent.com/khokharsnehil45/plu/master/install.sh | bash
```

### 🪟 Windows (PowerShell)
Open PowerShell and run:
```powershell
irm https://raw.githubusercontent.com/khokharsnehil45/plu/master/install.ps1 | iex
```

### 📦 Via Cargo (Any OS)
If you have Rust / Cargo installed:
```bash
cargo install --git https://github.com/khokharsnehil45/plu.git --force
```

---

## 🖥️ Interactive CLI UI

Launch `plu` with no arguments or `--ui` to open the clean interactive terminal interface:

```bash
plu
# or
plu --ui
```

```text
PLU v1.0.0 • PDF Loader & Unloader
────────────────────────────────────────────────────────────
 Welcome to PLU Engine!
 Ready to load PDF content and unload .md / .txt / .json / .plu files.
────────────────────────────────────────────────────────────
 [1] Load & Unload PDF   Extract page-by-page & unload into .md, .txt, .json, .plu
 [2] Exit                Quit PLU Engine
────────────────────────────────────────────────────────────
 Select an option [1-2]: 
```

When you choose **[1] Load & Unload PDF**, you can pick your preferred unload format directly:
- **`[1] Markdown (.md)`**: Clean Markdown with YAML frontmatter, `# Page N` headers, and page metadata blockquotes.
- **`[2] Plain Text (.txt)`**: Standard UTF-8 plain text with page headers.
- **`[3] JSON (.json)`**: Complete structured JSON document.
- **`[4] PLU Binary (.plu)`**: High-performance compressed binary container with index tables and CRC32 checks.

---

## 🚀 Unified Pipeline: Load & Unload

PLU is engineered around a single unified pipeline holding **Load** and **Unload** together. The Loader extracts PDF content page-by-page concurrently across multi-core CPUs, and the Unloader writes it directly into your chosen format:

### Markdown (`.md`)
```bash
# Dump into clean Markdown with YAML frontmatter
plu --load document.pdf --unload document.md

# Or specify format flag (defaults to <file_stem>.md)
plu --load document.pdf -f md
```

### Other Formats (`.txt`, `.plu`, `.json`, `.jsonl`)
```bash
plu --load document.pdf --unload document.txt    # Plain text
plu --load document.pdf --unload document.plu    # High-speed binary container
plu --load document.pdf --unload document.json   # Structured JSON document
plu --load document.pdf --unload document.jsonl  # Streaming JSON Lines (one line per page)
```

---

## 🔍 Smart OCR Feature (Scanned & Image PDFs)

PLU includes an intelligent **hybrid extraction engine**:
- **Digital PDFs**: Extracted via sub-millisecond font/CMap stream parsing at **~6,500+ pages/second**.
- **Scanned & Image PDFs**: Automatically detected. When a page has no digital text or contains scanned images, PLU seamlessly activates an in-memory streaming OCR pipeline (`pdftoppm` + `tesseract`).
- **Zero Disk Writes**: The OCR pipeline streams rasterized pages directly through kernel memory pipes with zero temp files written to disk.

```bash
# Automatic OCR on image/scanned documents (enabled by default)
plu --load scanned_document.pdf

# Disable OCR (pure digital text extraction only)
plu --load document.pdf --no-ocr

# Specify OCR language (default: eng)
plu --load document.pdf --ocr-lang eng
```

---

## 🔄 Batch Processing Add-On

PLU includes built-in batch processing for handling entire directories of PDFs:

### Batch Load & Unload (Directory of PDFs ➔ Text Dumps)
```bash
# Automatically detects directory and batch-loads all .pdf files
plu --load ./documents_dir/ --unload ./text_output_dir/

# Or explicitly flag batch mode
plu --batch --load ./documents_dir/ --unload ./text_output_dir/
```

---

## 📊 Real-Time Progress Bar

During both single-file and batch operations, PLU displays an interactive progress bar showing:
- Real-time extraction and write status
- Processing speed (`pages/sec`)
- Elapsed time and progress percentage
- Multi-progress bars during batch runs for simultaneous file and page tracking

```text
⠋ [00:00:02] [██████████████████████████████] 1,282/1,282 pages (6,512.4 pages/s) Done
```

---

## 🏗️ Architecture

PLU separates data ingestion and persistence into two independent decoupled components connected by a bounded lock-free channel:

```
                      +---------------------------------------+
                      |           Input PDF File(s)           |
                      +---------------------------------------+
                                           |
                                           v
                      +---------------------------------------+
                      |           LOADER COMPONENT            |
                      |    (Rayon Multi-Core Worker Pool)     |
                      +---------------------------------------+
                        |           |           |           |
                     Worker 1    Worker 2    Worker 3    Worker N
                     [Page 1]    [Page 2]    [Page 3]    [Page N]
                        |           |           |           |
                        +-----------+-----+-----+-----------+
                                           |
                                           v  (Lock-Free Channel Stream)
                      +---------------------------------------+
                      | Bounded Queue (crossbeam-channel)     |
                      +---------------------------------------+
                                           |
                                           v
                      +---------------------------------------+
                      |          UNLOADER COMPONENT           |
                      |       (Dedicated OS Thread)           |
                      |                                       |
                      |   • Reorder Buffer (Page 1, 2, 3...)  |
                      |   • Direct Disk I/O Streaming         |
                      |   • In-flight CRC32 & Index Tracking  |
                      |   • Interactive Progress Bar Updates  |
                      +---------------------------------------+
                                           |
                                           v
                      +---------------------------------------+
                      |       Dumped Container (.plu / text)  |
                      +---------------------------------------+
```

### Key Architectural Pillars:
1. **The Loader Component (`PdfLoader`)**:
   - Parses the document catalog and object hierarchy.
   - Extracts page-level metadata (`MediaBox` / `CropBox` dimensions in points, title, author).
   - Distributes page extraction concurrently across CPU cores via Rayon worker threads.
   - Handles font encodings, CMaps, CID fonts, and content streams with UTF-16BE/LE BOM decoding.
2. **Concurrent Streaming Pipeline**:
   - As each page is parsed by any worker thread, it is immediately sent into a bounded lock-free channel.
   - Memory usage is bounded to $O(\text{queue\_capacity})$ rather than loading the whole PDF into RAM.
3. **The Unloader Component (`PdfUnloader`)**:
   - Runs concurrently on its own dedicated OS thread (`unloader`).
   - Maintains an internal $O(\log k)$ reorder buffer to guarantee deterministic sequential output (`Page 1, Page 2...`) even when worker threads complete out of order.
   - Streams page payloads straight to disk buffered I/O while generating index tables and verifying CRC32 checksums.

---

## 🔬 The `.plu` Binary Specification

The native `.plu` container is a high-performance binary format engineered for zero-copy deserialization, random-access seeking, and payload verification:

```
+-------------------------------------------------------------+
| MAGIC: b"PLU\x01" (4 bytes)                                 |
+-------------------------------------------------------------+
| HEADER (Little-Endian):                                     |
|   • version: u16                                            |
|   • flags: u16                                              |
|   • page_count: u32                                         |
|   • total_chars: u64                                        |
|   • total_words: u64                                        |
|   • timestamp_secs: u64                                     |
|   • source_path: u16 length + UTF-8 bytes                   |
|   • title: u16 length + UTF-8 bytes                         |
|   • author: u16 length + UTF-8 bytes                        |
|   • index_table_offset: u64 (byte offset to index table)    |
+-------------------------------------------------------------+
| PAGE RECORDS (Sequentially written):                        |
|   For each page (1..=N):                                    |
|     • page_num: u32                                         |
|     • width: f32 (points)                                   |
|     • height: f32 (points)                                  |
|     • char_count: u64                                       |
|     • word_count: u64                                       |
|     • crc32: u32 (checksum of text payload)                 |
|     • text_len: u32                                         |
|     • text_bytes: [u8; text_len]                            |
+-------------------------------------------------------------+
| INDEX TABLE (Located at index_table_offset):                |
|   For each page (1..=N):                                    |
|     • page_num: u32                                         |
|     • offset: u64 (absolute file offset of page record)     |
|     • length: u32 (record byte length)                      |
+-------------------------------------------------------------+
```

---

## 📊 Benchmarks

Evaluated on an 8.4 MB document (*Programming Rust, 2nd Edition*):

| Metric | Result |
| :--- | :--- |
| **Total Pages** | **1,282 pages** |
| **Total Text Volume** | **1,488,256 characters** / **253,133 words** |
| **Parallel Extraction Throughput** | **~6,500+ pages/second** |
| **Total Concurrent Pipeline Time** | **592 ms** |
| **Integrity Verification** | **Hardware-accelerated CRC32 per page** |

---

## 🛠️ CLI Options Reference

```
Usage: plu [OPTIONS]

Options:
  -l, --load <PATH>        Path to input PDF file or directory to load (page-by-page extraction)
  -u, --unload <PATH>      Path to file/directory to unload/dump to (default: <stem>.txt or <dir>_txt)
  -b, --batch              Explicitly enable batch processing mode
  -t, --threads <N>        Number of worker threads for parallel extraction (default: CPU cores)
  -f, --format <FORMAT>    Force output format when dumping (md, txt, plu, json, jsonl)
      --no-ocr             Disable automatic OCR fallback for scanned images
      --ocr-lang <LANG>    Language code for Tesseract OCR (default: eng)
      --ui                 Launch interactive terminal UI
  -h, --help               Print help
  -V, --version            Print version
```

---

## 💻 Rust API Integration

PLU can also be embedded directly in your Rust applications:

```rust
use plu::{load_and_unload, run_batch_load, DumpFormat};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    // 1. Concurrent single-file Load & Unload pipeline
    load_and_unload("book.pdf", "output.plu", Some(8))?;

    // 2. Batch Load & Unload directory of PDFs
    run_batch_load(Path::new("./pdfs"), Path::new("./plus"), Some(8), DumpFormat::Plu)?;

    Ok(())
}
```

---

## 📄 License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
