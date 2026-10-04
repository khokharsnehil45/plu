# PLU: High-Performance PDF Loader & Unloader

A production-grade, high-throughput Rust engine and CLI tool designed to load PDF content, extract text **page by page** concurrently, and dump/unload it into structured storage.

---

## ⚡ Key Highlights & Performance

- **Blazing Fast**: Extracts and processes up to **6,600+ pages per second** on multi-core systems.
- **Architectural Separation**: Strict separation between the **Loader** (PDF ingestion, page tree resolution, parallel page extraction) and the **Unloader** (data dumping, container serialization, unpacker).
- **Page-by-Page Extraction**: Every page is isolated with dimensions (`width`, `height`), character count, word count, text payload, and CRC32 integrity check.
- **Custom `.plu` Binary Container**:
  - `PLU\x01` magic header with metadata.
  - **O(1) Random Access Index Table**: Seek and read any page in microseconds without parsing preceding pages.
  - Per-page CRC32 checksums for zero-corruption guarantee.
- **Multiple Target Formats**: Native `.plu` binary, `.txt`, `.json`, and `.jsonl`.
- **Global CLI**: Run `plu --load file_path --unload file_path` anywhere.

---

## 🚀 CLI Usage

### 1. Load and Dump to `.plu` Container
```bash
plu --load document.pdf --unload document.plu
```

### 2. Load and Dump to Formatted Text or JSON
```bash
# Dump to formatted text with per-page headers
plu --load document.pdf --unload output.txt

# Dump to full JSON document
plu --load document.pdf --unload output.json

# Dump to streaming JSON Lines (one page per line)
plu --load document.pdf --unload output.jsonl
```

### 3. Unload / Inspect `.plu` Container
```bash
# Inspect container metadata and statistics
plu --unload document.plu

# Verbose breakdown of all pages
plu --unload document.plu --verbose

# Instant O(1) random-access lookup for a specific page
plu --unload document.plu --page 42
```

### 4. Unpack All Pages to Directory
```bash
plu --unload document.plu --unpack ./extracted_pages/
```
Generates:
```
extracted_pages/
├── metadata.json
├── page_0001.txt
├── page_0002.txt
└── ...
```

---

## 🏗️ Architecture

The system is separated into two standalone core components:

```
                  +-----------------------------------+
                  |           PDF Document            |
                  +-----------------------------------+
                                    |
                                    v
     +-------------------------------------------------------------+
     |                    LOADER COMPONENT                         |
     |                                                             |
     |  [PdfLoader::load_file]                                     |
     |    • Validates input file and opens PDF catalog             |
     |    • Resolves page tree & extracts metadata (Title, Author) |
     |                                                             |
     |  [Parallel Page-by-Page Extractor (Rayon Pool)]             |
     |    • Page 1: MediaBox dimensions + Content Stream + Fonts   |
     |    • Page 2: MediaBox dimensions + Content Stream + Fonts   |
     |    • Page N: MediaBox dimensions + Content Stream + Fonts   |
     +-------------------------------------------------------------+
                                    |
                            Vec<PageData>
                                    |
                                    v
     +-------------------------------------------------------------+
     |                   UNLOADER COMPONENT                        |
     |                                                             |
     |  [PdfUnloader::dump_with_format]                            |
     |    • Serializes into .plu / .txt / .json / .jsonl           |
     |    • Builds O(1) Index Table and per-page CRC32             |
     |                                                             |
     |  [PdfUnloader::unload_file / unload_single_page]            |
     |    • Reads and validates .plu container                     |
     |    • Fast O(1) random access by page number                 |
     |    • Unpacks individual pages into disk directory           |
     +-------------------------------------------------------------+
                                    |
                                    v
                  +-----------------------------------+
                  |      Target Dump File (.plu)      |
                  +-----------------------------------+
```

---

## 🔬 Binary Format Specification (`.plu`)

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
|   • source_path: u16 len + UTF-8 bytes                      |
|   • title: u16 len + UTF-8 bytes                            |
|   • author: u16 len + UTF-8 bytes                           |
|   • index_table_offset: u64                                 |
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
|     • offset: u64                                           |
|     • length: u32                                           |
+-------------------------------------------------------------+
```

---

## 📊 Benchmark Results

Tested on an 8.4 MB document (*Programming Rust, 2nd Edition*):
- **Pages**: 1,282 pages
- **Characters**: 1,488,256 characters
- **Words**: 253,133 words
- **Loader Extraction Time**: **197.92 ms** (**~6,500 pages/sec**)
- **Unloader Dump Time**: **4 ms**
- **Single Page Random-Access Lookup**: **~24-64 µs**
