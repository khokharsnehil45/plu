use plu::format::PluFormat;
use plu::loader::PdfLoader;
use plu::types::{DumpFormat, PageData, PluDocument};
use plu::unloader::PdfUnloader;
use std::io::Cursor;
use tempfile::tempdir;

#[test]
fn test_plu_binary_serialization_roundtrip() {
    let pages = vec![
        PageData::new(1, 612.0, 792.0, "Hello World from Page 1".to_string()),
        PageData::new(2, 612.0, 792.0, "Second page text with numbers 12345.".to_string()),
        PageData::new(3, 595.0, 842.0, "Third page A4 dimensions.".to_string()),
    ];

    let doc = PluDocument::new(
        "/dummy/path/doc.pdf".to_string(),
        Some("Test Document".to_string()),
        Some("Test Author".to_string()),
        pages.clone(),
    );

    let mut buffer = Cursor::new(Vec::new());
    let bytes_written = PluFormat::write(&mut buffer, &doc).expect("Failed to serialize");
    assert!(bytes_written > 0);

    // Read back
    buffer.set_position(0);
    let decoded = PluFormat::read(&mut buffer).expect("Failed to deserialize");

    assert_eq!(decoded.meta.source_path, "/dummy/path/doc.pdf");
    assert_eq!(decoded.meta.title.as_deref(), Some("Test Document"));
    assert_eq!(decoded.meta.author.as_deref(), Some("Test Author"));
    assert_eq!(decoded.meta.page_count, 3);
    assert_eq!(decoded.pages.len(), 3);
    assert_eq!(decoded.pages[0].page_num, 1);
    assert_eq!(decoded.pages[0].text, "Hello World from Page 1");
    assert_eq!(decoded.pages[1].page_num, 2);
    assert_eq!(decoded.pages[2].page_num, 3);
    assert_eq!(decoded.pages[2].width, 595.0);
}

#[test]
fn test_random_access_single_page_read() {
    let pages = vec![
        PageData::new(1, 612.0, 792.0, "Page 1 content".to_string()),
        PageData::new(2, 612.0, 792.0, "Page 2 specific content for lookup".to_string()),
        PageData::new(3, 612.0, 792.0, "Page 3 trailing content".to_string()),
    ];

    let doc = PluDocument::new("doc.pdf".to_string(), None, None, pages);
    let mut buffer = Cursor::new(Vec::new());
    PluFormat::write(&mut buffer, &doc).unwrap();

    // Query Page 2 directly
    buffer.set_position(0);
    let page2 = PluFormat::read_single_page(&mut buffer, 2).expect("Failed to read page 2");
    assert_eq!(page2.page_num, 2);
    assert_eq!(page2.text, "Page 2 specific content for lookup");

    // Non-existent page should fail
    buffer.set_position(0);
    let err = PluFormat::read_single_page(&mut buffer, 99);
    assert!(err.is_err());
}

#[test]
fn test_pdf_loader_and_unloader_e2e() {
    let pdf_path = "/home/kevin/sample_bank_receipt.pdf";
    if !std::path::Path::new(pdf_path).exists() {
        return; // skip if not present
    }

    // 1. Loader test
    let loader = PdfLoader::load_file(pdf_path).expect("Failed to load PDF");
    assert_eq!(loader.page_count(), 1);

    let pages = loader.extract_all_parallel(Some(2)).expect("Parallel extraction failed");
    assert_eq!(pages.len(), 1);
    assert!(pages[0].text.contains("SILICON VALLEY COMMERCE BANK"));
    assert!(pages[0].char_count > 500);

    // 2. Unloader test to tempdir
    let dir = tempdir().unwrap();
    let plu_path = dir.path().join("dump.plu");

    let doc = PluDocument::new(
        loader.source_path().to_string_lossy().to_string(),
        loader.metadata().title.clone(),
        loader.metadata().author.clone(),
        pages,
    );

    let stats = PdfUnloader::dump_with_format(&doc, &plu_path, DumpFormat::Plu).unwrap();
    assert_eq!(stats.pages_processed, 1);
    assert!(plu_path.exists());

    // 3. Unloader read back
    let unloaded_doc = PdfUnloader::unload_file(&plu_path).expect("Failed to unload .plu");
    assert_eq!(unloaded_doc.pages.len(), 1);
    assert!(unloaded_doc.pages[0].text.contains("SILICON VALLEY COMMERCE BANK"));

    // 4. Unpack directory test
    let unpack_dir = dir.path().join("unpacked");
    let count = PdfUnloader::unpack_to_directory(&plu_path, &unpack_dir).unwrap();
    assert_eq!(count, 1);
    assert!(unpack_dir.join("metadata.json").exists());
    assert!(unpack_dir.join("page_0001.txt").exists());
}
