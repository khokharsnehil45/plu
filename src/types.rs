use serde::{Deserialize, Serialize};
use std::fmt;

/// Extracted content and metadata for an individual PDF page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageData {
    /// 1-based page index
    pub page_num: u32,
    /// Page width in points (default 72 DPI)
    pub width: f32,
    /// Page height in points (default 72 DPI)
    pub height: f32,
    /// Total character count
    pub char_count: usize,
    /// Total word count
    pub word_count: usize,
    /// Extracted text content
    pub text: String,
}

impl PageData {
    pub fn new(page_num: u32, width: f32, height: f32, text: String) -> Self {
        let char_count = text.chars().count();
        let word_count = text.split_whitespace().count();
        Self {
            page_num,
            width,
            height,
            char_count,
            word_count,
            text,
        }
    }
}

/// Metadata about the loaded PDF document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentMeta {
    pub source_path: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub page_count: u32,
    pub total_chars: usize,
    pub total_words: usize,
}

/// In-memory representation of an entire loaded or unloaded document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluDocument {
    pub meta: DocumentMeta,
    pub pages: Vec<PageData>,
}

impl PluDocument {
    pub fn new(source_path: String, title: Option<String>, author: Option<String>, pages: Vec<PageData>) -> Self {
        let page_count = pages.len() as u32;
        let total_chars = pages.iter().map(|p| p.char_count).sum();
        let total_words = pages.iter().map(|p| p.word_count).sum();

        Self {
            meta: DocumentMeta {
                source_path,
                title,
                author,
                page_count,
                total_chars,
                total_words,
            },
            pages,
        }
    }
}

/// Output format for dumping content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpFormat {
    /// Markdown document with YAML frontmatter (.md)
    Markdown,
    /// UTF-8 Plain text with page headers (.txt)
    Text,
    /// Native high-performance binary container (.plu)
    Plu,
    /// Full JSON document (.json)
    Json,
    /// JSON Lines format (one page per line) (.jsonl)
    JsonLines,
}

impl DumpFormat {
    /// Returns the standard file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Text => "txt",
            Self::Plu => "plu",
            Self::Json => "json",
            Self::JsonLines => "jsonl",
        }
    }
}

impl fmt::Display for DumpFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Markdown => write!(f, "Markdown (.md)"),
            Self::Text => write!(f, "Plain Text (.txt)"),
            Self::Plu => write!(f, "PLU Binary Container (.plu)"),
            Self::Json => write!(f, "JSON (.json)"),
            Self::JsonLines => write!(f, "JSON Lines (.jsonl)"),
        }
    }
}

/// Summary statistics reported after loading or unloading.
#[derive(Debug, Clone, Default)]
pub struct OperationStats {
    pub pages_processed: u32,
    pub total_chars: usize,
    pub total_words: usize,
    pub bytes_written: u64,
    pub duration_ms: u128,
}
