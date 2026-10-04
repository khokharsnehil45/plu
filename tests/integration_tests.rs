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

    // 4. Unload to text file test
    let text_path = dir.path().join("dump.txt");
    let unload_stats = PdfUnloader::unload_to_file(&plu_path, &text_path, None).expect("Failed to unload to file");
    assert_eq!(unload_stats.pages_processed, 1);
    assert!(text_path.exists());
    let text_content = std::fs::read_to_string(&text_path).unwrap();
    assert!(text_content.contains("SILICON VALLEY COMMERCE BANK"));
    assert!(text_content.contains("--- PAGE 1"));
}

#[test]
fn test_batch_load_and_unload() {
    let pdf_path = "/home/kevin/sample_bank_receipt.pdf";
    if !std::path::Path::new(pdf_path).exists() {
        return;
    }

    let input_dir = tempdir().unwrap();
    let plu_dir = tempdir().unwrap();
    let txt_dir = tempdir().unwrap();

    // Copy sample PDF to create multiple files for batch test
    std::fs::copy(pdf_path, input_dir.path().join("doc1.pdf")).unwrap();
    std::fs::copy(pdf_path, input_dir.path().join("doc2.pdf")).unwrap();

    // 1. Discover files
    let found = plu::batch::discover_files(input_dir.path(), "pdf").unwrap();
    assert_eq!(found.len(), 2);

    // 2. Batch load
    let load_stats = plu::batch::run_batch_load(
        input_dir.path(),
        plu_dir.path(),
        Some(2),
        DumpFormat::Plu,
    ).expect("Batch load failed");
    assert_eq!(load_stats.files_processed, 2);
    assert_eq!(load_stats.total_pages, 2);
    assert!(plu_dir.path().join("doc1.plu").exists());
    assert!(plu_dir.path().join("doc2.plu").exists());

    // 3. Batch unload
    let unload_stats = plu::batch::run_batch_unload(
        plu_dir.path(),
        txt_dir.path(),
    ).expect("Batch unload failed");
    assert_eq!(unload_stats.files_processed, 2);
    assert_eq!(unload_stats.total_pages, 2);
    assert!(txt_dir.path().join("doc1.txt").exists());
    assert!(txt_dir.path().join("doc2.txt").exists());

    let doc1_txt = std::fs::read_to_string(txt_dir.path().join("doc1.txt")).unwrap();
    assert!(doc1_txt.contains("SILICON VALLEY COMMERCE BANK"));
}

