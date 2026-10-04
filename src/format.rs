use crate::types::{DocumentMeta, PageData, PluDocument};
use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PLU_MAGIC: &[u8; 4] = b"PLU\x01";
pub const PLU_VERSION: u16 = 1;

/// Index entry for fast O(1) random access to pages within a .plu file
#[derive(Debug, Clone, Copy)]
pub struct PageIndexEntry {
    pub page_num: u32,
    pub offset: u64,
    pub length: u32,
}

/// State tracking positions for backpatching during streaming writes
pub struct StreamState {
    pub header_stats_pos: u64,
    pub index_offset_pos: u64,
}

pub struct PluFormat;

impl PluFormat {
    /// Starts a streaming `.plu` container write. Writes magic and header with placeholders.
    pub fn start_stream<W: Write + Seek>(
        writer: &mut W,
        meta: &DocumentMeta,
        page_count: u32,
    ) -> Result<StreamState> {
        // 1. Write Magic
        writer.write_all(PLU_MAGIC)?;

        // 2. Write Header
        writer.write_u16::<LittleEndian>(PLU_VERSION)?;
        writer.write_u16::<LittleEndian>(0)?; // flags
        writer.write_u32::<LittleEndian>(page_count)?;

        // Save position for total_chars and total_words
        let header_stats_pos = writer.stream_position()?;
        writer.write_u64::<LittleEndian>(0)?; // total_chars placeholder
        writer.write_u64::<LittleEndian>(0)?; // total_words placeholder

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        writer.write_u64::<LittleEndian>(timestamp)?;

        // Source path
        let src_bytes = meta.source_path.as_bytes();
        let src_len = src_bytes.len().min(u16::MAX as usize) as u16;
        writer.write_u16::<LittleEndian>(src_len)?;
        writer.write_all(&src_bytes[..src_len as usize])?;

        // Title
        let title_bytes = meta.title.as_deref().unwrap_or("").as_bytes();
        let title_len = title_bytes.len().min(u16::MAX as usize) as u16;
        writer.write_u16::<LittleEndian>(title_len)?;
        writer.write_all(&title_bytes[..title_len as usize])?;

        // Author
        let author_bytes = meta.author.as_deref().unwrap_or("").as_bytes();
        let author_len = author_bytes.len().min(u16::MAX as usize) as u16;
        writer.write_u16::<LittleEndian>(author_len)?;
        writer.write_all(&author_bytes[..author_len as usize])?;

        // 3. Write Placeholder for Index Table Offset (8 bytes)
        let index_offset_pos = writer.stream_position()?;
        writer.write_u64::<LittleEndian>(0)?;

        Ok(StreamState {
            header_stats_pos,
            index_offset_pos,
        })
    }

    /// Writes a single page record into the stream, returning its index entry.
    pub fn write_page_record<W: Write + Seek>(
        writer: &mut W,
        page: &PageData,
    ) -> Result<PageIndexEntry> {
        let page_offset = writer.stream_position()?;

        // Compute CRC32 of text payload
        let text_bytes = page.text.as_bytes();
        let mut hasher = Hasher::new();
        hasher.update(text_bytes);
        let crc = hasher.finalize();

        writer.write_u32::<LittleEndian>(page.page_num)?;
        writer.write_f32::<LittleEndian>(page.width)?;
        writer.write_f32::<LittleEndian>(page.height)?;
        writer.write_u64::<LittleEndian>(page.char_count as u64)?;
        writer.write_u64::<LittleEndian>(page.word_count as u64)?;
        writer.write_u32::<LittleEndian>(crc)?;
        writer.write_u32::<LittleEndian>(text_bytes.len() as u32)?;
        writer.write_all(text_bytes)?;

        let record_end = writer.stream_position()?;
        let record_len = (record_end - page_offset) as u32;

        Ok(PageIndexEntry {
            page_num: page.page_num,
            offset: page_offset,
            length: record_len,
        })
    }

    /// Finalizes the streaming write: writes index table and backpatches placeholders.
    pub fn finish_stream<W: Write + Seek>(
        writer: &mut W,
        state: StreamState,
        index_entries: &[PageIndexEntry],
        total_chars: usize,
        total_words: usize,
    ) -> Result<u64> {
        // 1. Write Index Table at the current position
        let index_table_offset = writer.stream_position()?;
        for entry in index_entries {
            writer.write_u32::<LittleEndian>(entry.page_num)?;
            writer.write_u64::<LittleEndian>(entry.offset)?;
            writer.write_u32::<LittleEndian>(entry.length)?;
        }

        let file_end = writer.stream_position()?;

        // 2. Backpatch total_chars and total_words
        writer.seek(SeekFrom::Start(state.header_stats_pos))?;
        writer.write_u64::<LittleEndian>(total_chars as u64)?;
        writer.write_u64::<LittleEndian>(total_words as u64)?;

        // 3. Backpatch index table offset
        writer.seek(SeekFrom::Start(state.index_offset_pos))?;
        writer.write_u64::<LittleEndian>(index_table_offset)?;

        // 4. Return to end and flush
        writer.seek(SeekFrom::Start(file_end))?;
        writer.flush()?;

        Ok(file_end)
    }

    /// Serializes a complete in-memory `PluDocument` into a `.plu` binary stream.
    pub fn write<W: Write + Seek>(writer: &mut W, doc: &PluDocument) -> Result<u64> {
        let state = Self::start_stream(writer, &doc.meta, doc.pages.len() as u32)?;

        let mut index_entries = Vec::with_capacity(doc.pages.len());
        for page in &doc.pages {
            let entry = Self::write_page_record(writer, page)?;
            index_entries.push(entry);
        }

        Self::finish_stream(
            writer,
            state,
            &index_entries,
            doc.meta.total_chars,
            doc.meta.total_words,
        )
    }

    /// Reads and deserializes a full `PluDocument` from a reader.
    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<PluDocument> {
        let (meta, index_offset, page_count) = Self::read_header_and_index_pos(reader)?;

        // Seek to index table
        reader
            .seek(SeekFrom::Start(index_offset))
            .with_context(|| "Failed to seek to index table in .plu file")?;

        let mut index_entries = Vec::with_capacity(page_count as usize);
        for _ in 0..page_count {
            let page_num = reader.read_u32::<LittleEndian>()?;
            let offset = reader.read_u64::<LittleEndian>()?;
            let length = reader.read_u32::<LittleEndian>()?;
            index_entries.push(PageIndexEntry {
                page_num,
                offset,
                length,
            });
        }

        // Read all pages using the index
        let mut pages = Vec::with_capacity(page_count as usize);
        for entry in index_entries {
            reader
                .seek(SeekFrom::Start(entry.offset))
                .with_context(|| format!("Failed to seek to page {}", entry.page_num))?;

            let page = Self::read_page_record(reader)?;
            if page.page_num != entry.page_num {
                bail!(
                    "Page index mismatch: expected page {}, found {}",
                    entry.page_num,
                    page.page_num
                );
            }
            pages.push(page);
        }

        Ok(PluDocument { meta, pages })
    }

    /// Reads only the document metadata without parsing all page contents.
    pub fn read_meta<R: Read + Seek>(reader: &mut R) -> Result<DocumentMeta> {
        let (meta, _, _) = Self::read_header_and_index_pos(reader)?;
        Ok(meta)
    }

    /// Reads a single page by its 1-based page number in O(1) time using the index table.
    pub fn read_single_page<R: Read + Seek>(
        reader: &mut R,
        target_page_num: u32,
    ) -> Result<PageData> {
        let (_, index_offset, page_count) = Self::read_header_and_index_pos(reader)?;

        reader.seek(SeekFrom::Start(index_offset))?;
        for _ in 0..page_count {
            let page_num = reader.read_u32::<LittleEndian>()?;
            let offset = reader.read_u64::<LittleEndian>()?;
            let _length = reader.read_u32::<LittleEndian>()?;

            if page_num == target_page_num {
                reader.seek(SeekFrom::Start(offset))?;
                return Self::read_page_record(reader);
            }
        }

        bail!("Page {} not found in .plu container", target_page_num);
    }

    fn read_header_and_index_pos<R: Read + Seek>(
        reader: &mut R,
    ) -> Result<(DocumentMeta, u64, u32)> {
        let mut magic = [0u8; 4];
        reader
            .read_exact(&mut magic)
            .with_context(|| "Failed to read PLU magic bytes")?;

        if &magic != PLU_MAGIC {
            bail!("Invalid PLU file: magic bytes mismatch. Expected 'PLU\\x01'");
        }

        let version = reader.read_u16::<LittleEndian>()?;
        if version != PLU_VERSION {
            bail!("Unsupported PLU version: {version}. Expected {PLU_VERSION}");
        }

        let _flags = reader.read_u16::<LittleEndian>()?;
        let page_count = reader.read_u32::<LittleEndian>()?;
        let total_chars = reader.read_u64::<LittleEndian>()? as usize;
        let total_words = reader.read_u64::<LittleEndian>()? as usize;
        let _timestamp = reader.read_u64::<LittleEndian>()?;

        // Read strings
        let src_len = reader.read_u16::<LittleEndian>()? as usize;
        let mut src_bytes = vec![0u8; src_len];
        reader.read_exact(&mut src_bytes)?;
        let source_path = String::from_utf8_lossy(&src_bytes).to_string();

        let title_len = reader.read_u16::<LittleEndian>()? as usize;
        let mut title_bytes = vec![0u8; title_len];
        reader.read_exact(&mut title_bytes)?;
        let title = if title_len > 0 {
            Some(String::from_utf8_lossy(&title_bytes).to_string())
        } else {
            None
        };

        let author_len = reader.read_u16::<LittleEndian>()? as usize;
        let mut author_bytes = vec![0u8; author_len];
        reader.read_exact(&mut author_bytes)?;
        let author = if author_len > 0 {
            Some(String::from_utf8_lossy(&author_bytes).to_string())
        } else {
            None
        };

        let index_offset = reader.read_u64::<LittleEndian>()?;

        let meta = DocumentMeta {
            source_path,
            title,
            author,
            page_count,
            total_chars,
            total_words,
        };

        Ok((meta, index_offset, page_count))
    }

    fn read_page_record<R: Read>(reader: &mut R) -> Result<PageData> {
        let page_num = reader.read_u32::<LittleEndian>()?;
        let width = reader.read_f32::<LittleEndian>()?;
        let height = reader.read_f32::<LittleEndian>()?;
        let char_count = reader.read_u64::<LittleEndian>()? as usize;
        let word_count = reader.read_u64::<LittleEndian>()? as usize;
        let expected_crc = reader.read_u32::<LittleEndian>()?;
        let text_len = reader.read_u32::<LittleEndian>()? as usize;

        let mut text_bytes = vec![0u8; text_len];
        reader
            .read_exact(&mut text_bytes)
            .with_context(|| format!("Failed to read page {page_num} payload"))?;

        // Verify CRC32 checksum
        let mut hasher = Hasher::new();
        hasher.update(&text_bytes);
        let actual_crc = hasher.finalize();

        if actual_crc != expected_crc {
            bail!(
                "CRC32 checksum mismatch for page {}: expected 0x{:08X}, got 0x{:08X}",
                page_num,
                expected_crc,
                actual_crc
            );
        }

        let text = String::from_utf8(text_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in page {page_num} payload: {e}"))?;

        Ok(PageData {
            page_num,
            width,
            height,
            char_count,
            word_count,
            text,
        })
    }
}
