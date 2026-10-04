<div align="center">

# ⚡ PLU: High-Performance PDF Loader & Unloader

**A production-grade, multi-threaded PDF extraction and containerization engine written in Rust.**

[![CI](https://github.com/khokharsnehil45/plu/actions/workflows/ci.yml/badge.svg)](https://github.com/khokharsnehil45/plu/actions)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey.svg)]()

*Extract PDF content page-by-page concurrently across multi-core CPUs and stream into high-speed binary archives.*

</div>

---

## ⚡ 1-Command Installation

### 🐧 Linux & 🍎 macOS
Open your terminal and run:
```bash
curl -fsSL https://raw.githubusercontent.com/khokharsnehil45/plu/master/install.sh | bash
```

### 🪟 Windows (PowerShell)
Open PowerShell (as Administrator or standard user) and run:
```powershell
irm https://raw.githubusercontent.com/khokharsnehil45/plu/master/install.ps1 | iex
```

### 📦 Via Cargo (Any OS)
If you already have Rust / Cargo installed:
```bash
cargo install --git https://github.com/khokharsnehil45/plu.git --force
```

---

## 🖥️ Interactive CLI UI

Run `plu` with no arguments or pass `--ui` to launch the minimalist interactive menu styled purely with `=` and `|`:

```bash
plu --ui
```

```
==============================================================================
|                 PLU: PDF LOADER & UNLOADER INTERACTIVE UI                  |
==============================================================================
| [1] Load PDF & Dump (Concurrent Page-by-Page Extraction)                    |
| [2] Unload & Inspect .plu Container File                                    |
| [3] Read Single Page from .plu (O(1) Random Access)                         |
| [4] Unpack .plu Pages to Directory                                          |
| [5] Exit                                                                    |
==============================================================================
| Select an option [1-5]:
```

---

## 🚀 Quick Start

### 1. Load and Dump PDF
Extract page-by-page and dump directly into the native `.plu` binary format:
```bash
plu --load document.pdf --unload document.plu
```

Dump into other formats by specifying the file extension or `--format`:
```bash
# Formatted text with per-page delimiters
plu --load document.pdf --unload document.txt

# Structured JSON document
plu --load document.pdf --unload document.json

# Streaming JSON Lines (one line per page)
plu --load document.pdf --unload document.jsonl
```

### 2. Unload & Inspect `.plu` Containers
```bash
# Verify container and print header metadata
plu --unload document.plu

# Inspect all page dimensions, word & character counts
plu --unload document.plu --verbose

# Microsecond O(1) random-access lookup for an individual page
plu --unload document.plu --page 42

# Unpack all pages into discrete files in a directory
plu --unload document.plu --unpack ./extracted_pages/
```

---

## 🏗️ Architecture

PLU is engineered around a clean separation of concerns between two independent components connected via lock-free concurrent streaming:

```
                      +---------------------------------------+
                      |           Input PDF File              |
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
                                          v  (Real-time Lock-free Stream)
                      +---------------------------------------+
                      | Bounded Channel (crossbeam-channel)   |
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
                      +---------------------------------------+
                                          |
                                          v
                      +---------------------------------------+
                      |         Target Dump (.plu)            |
                      +---------------------------------------+
```

### Key Architectural Pillars:
1. **The Loader Component (`PdfLoader`)**:
   - Parses the document catalog and page object hierarchy.
   - Extracts page-level metadata (`MediaBox` / `CropBox` dimensions in points, title, author).
   - Distributes page extraction concurrently across CPU cores via Rayon worker threads.
   - Resolves font encodings, CMaps, CID fonts, and content streams with UTF-16BE BOM decoding.
2. **Concurrent Streaming Pipeline**:
   - As each page is parsed by any worker thread, it is immediately sent into a bounded lock-free channel.
   - Memory usage is bounded to $O(\text{queue\_capacity})$ rather than loading the whole PDF into RAM.
3. **The Unloader Component (`PdfUnloader`)**:
   - Runs concurrently on its own dedicated OS thread (`plu-unloader`).
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

### Advantages of `.plu`:
- **Microsecond $O(1)$ Page Seeks**: Direct random access to page $N$ in a 5,000-page archive without scanning previous pages.
- **Data Integrity**: Every page payload has an isolated CRC32 checksum.
- **Zero Serialization Bloat**: Binary little-endian layout is 5-10x smaller than raw JSON without sacrificing metadata.

---

## 📊 Benchmarks

Evaluated on an 8.4 MB document (*Programming Rust, 2nd Edition*):

| Metric | Result |
| :--- | :--- |
| **Total Pages** | **1,282 pages** |
| **Total Text Volume** | **1,488,256 characters** / **253,133 words** |
| **Parallel Extraction Throughput** | **~6,500+ pages/second** |
| **Total Concurrent Pipeline Time** | **592 ms** |
| **Single Page Random Lookup** | **24 – 64 microseconds** |

---

## 🛠️ CLI Options Reference

```
Usage: plu [OPTIONS]

Options:
  -l, --load <FILE_PATH>     Path to the input PDF file to load (page-by-page extraction)
  -u, --unload <FILE_PATH>   Path to the file to dump / unload to, or .plu file to unload from
  -p, --page <PAGE_NUM>      Specific page number to inspect or extract (1-based)
      --unpack <OUTPUT_DIR>  Unpack all pages from a .plu file into individual files
  -t, --threads <N>          Number of worker threads (default: available CPU cores)
  -f, --format <FORMAT>      Target output format (plu, txt, json, jsonl)
  -v, --verbose              Verbose output with per-page metrics breakdown
  -h, --help                 Print help
  -V, --version              Print version
```

---

## 💻 Rust API Integration

PLU can also be embedded directly in your Rust applications:

```rust
use plu::{PdfLoader, PdfUnloader, DumpFormat, load_and_unload};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    // 1. One-line concurrent pipeline
    load_and_unload("book.pdf", "output.plu", Some(8))?;

    // 2. Or use individual components directly:
    let loader = PdfLoader::load_file("book.pdf")?;
    println!("Pages: {}", loader.page_count());

    let page_1 = loader.extract_page(1)?;
    println!("Page 1 text: {}", page_1.text);

    // 3. Inspect a dumped container with O(1) random lookup
    let page_42 = PdfUnloader::unload_single_page("output.plu", 42)?;
    println!("Page 42 dimensions: {}x{}", page_42.width, page_42.height);

    Ok(())
}
```

---

## 📄 License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
