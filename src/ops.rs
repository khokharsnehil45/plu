use anyhow::{bail, Context, Result};
use lopdf::{Bookmark, Document, Object, ObjectId};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// PDF manipulation operations: Compress, Split, Merge
pub struct PdfOps;

impl PdfOps {
    /// Compresses stream objects in a PDF using FlateDecode and saves to output.
    /// Returns (original_size_bytes, compressed_size_bytes).
    pub fn compress_pdf<P: AsRef<Path>, Q: AsRef<Path>>(input: P, output: Q) -> Result<(u64, u64)> {
        let in_path = input.as_ref();
        let out_path = output.as_ref();

        if !in_path.exists() {
            bail!("Input PDF not found: {}", in_path.display());
        }

        let mut doc = Document::load(in_path)
            .with_context(|| format!("Failed to load PDF for compression: {}", in_path.display()))?;

        let orig_size = fs::metadata(in_path)?.len();

        doc.compress();
        Self::sanitize_doc(&mut doc);

        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        doc.save(out_path)
            .with_context(|| format!("Failed to save compressed PDF: {}", out_path.display()))?;

        let new_size = fs::metadata(out_path)?.len();
        Ok((orig_size, new_size))
    }

    /// Splits selected pages from input PDF and saves as a new PDF.
    pub fn split_pages<P: AsRef<Path>, Q: AsRef<Path>>(
        input: P,
        pages: &[u32],
        output: Q,
    ) -> Result<usize> {
        let in_path = input.as_ref();
        let out_path = output.as_ref();

        if !in_path.exists() {
            bail!("Input PDF not found: {}", in_path.display());
        }
        if pages.is_empty() {
            bail!("No pages specified for split");
        }

        let mut doc = Document::load(in_path)
            .with_context(|| format!("Failed to load PDF for split: {}", in_path.display()))?;

        let total_pages = doc.get_pages().len() as u32;
        let to_delete: Vec<u32> = (1..=total_pages)
            .filter(|p| !pages.contains(p))
            .collect();

        if to_delete.len() == total_pages as usize {
            bail!("All pages would be deleted. Please specify valid pages in range 1..={total_pages}");
        }

        doc.delete_pages(&to_delete);

        Self::sanitize_doc(&mut doc);

        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        doc.save(out_path)?;
        Ok(pages.len())
    }

    /// Splits all pages into individual single-page PDF files in output_dir.
    pub fn split_all<P: AsRef<Path>, Q: AsRef<Path>>(input: P, output_dir: Q) -> Result<usize> {
        let in_path = input.as_ref();
        let out_dir = output_dir.as_ref();

        if !in_path.exists() {
            bail!("Input PDF not found: {}", in_path.display());
        }

        let doc = Document::load(in_path)
            .with_context(|| format!("Failed to load PDF: {}", in_path.display()))?;

        let total_pages = doc.get_pages().len() as u32;
        fs::create_dir_all(out_dir)?;

        for p in 1..=total_pages {
            let mut single_page_doc = doc.clone();
            let to_delete: Vec<u32> = (1..=total_pages).filter(|&x| x != p).collect();
            single_page_doc.delete_pages(&to_delete);
            Self::sanitize_doc(&mut single_page_doc);

            let out_file = out_dir.join(format!("page_{:04}.pdf", p));
            single_page_doc.save(&out_file)?;
        }

        Ok(total_pages as usize)
    }

    /// Merges multiple PDF documents into a single cohesive document.
    pub fn merge_pdfs<P: AsRef<Path>>(inputs: &[PathBuf], output: P) -> Result<u32> {
        if inputs.is_empty() {
            bail!("No input files provided for merge");
        }
        if inputs.len() == 1 {
            fs::copy(&inputs[0], output.as_ref())?;
            let doc = Document::load(&inputs[0])?;
            return Ok(doc.get_pages().len() as u32);
        }

        let mut max_id = 1;
        let mut pagenum = 1;
        let mut documents_pages = BTreeMap::new();
        let mut documents_objects = BTreeMap::new();
        let mut document = Document::with_version("1.5");

        for in_path in inputs {
            if !in_path.exists() {
                bail!("Merge input not found: {}", in_path.display());
            }

            let mut doc = Document::load(in_path)
                .with_context(|| format!("Failed to load PDF: {}", in_path.display()))?;

            let mut first = false;
            doc.renumber_objects_with(max_id);
            max_id = doc.max_id + 1;

            let file_stem = in_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("Doc_{}", pagenum));

            for (_, object_id) in doc.get_pages() {
                if !first {
                    let bookmark = Bookmark::new(
                        format!("{}_{}", file_stem, pagenum),
                        [0.0, 0.0, 1.0],
                        0,
                        object_id,
                    );
                    document.add_bookmark(bookmark, None);
                    first = true;
                }
                if let Ok(obj) = doc.get_object(object_id) {
                    documents_pages.insert(object_id, obj.clone());
                }
                pagenum += 1;
            }

            documents_objects.extend(doc.objects);
        }

        let mut catalog_object: Option<(ObjectId, Object)> = None;
        let mut pages_object: Option<(ObjectId, Object)> = None;

        for (object_id, object) in documents_objects.iter() {
            match object.type_name().unwrap_or(b"") {
                b"Catalog" => {
                    catalog_object = Some((
                        if let Some((id, _)) = catalog_object {
                            id
                        } else {
                            *object_id
                        },
                        object.clone(),
                    ));
                }
                b"Pages" => {
                    if let Ok(dictionary) = object.as_dict() {
                        let mut dictionary = dictionary.clone();
                        if let Some((_, ref old_obj)) = pages_object {
                            if let Ok(old_dictionary) = old_obj.as_dict() {
                                dictionary.extend(old_dictionary);
                            }
                        }
                        pages_object = Some((
                            if let Some((id, _)) = pages_object {
                                id
                            } else {
                                *object_id
                            },
                            Object::Dictionary(dictionary),
                        ));
                    }
                }
                b"Page" | b"Outlines" | b"Outline" => {}
                _ => {
                    document.objects.insert(*object_id, object.clone());
                }
            }
        }

        let pages_object = pages_object
            .context("Pages root dictionary not found in document objects")?;
        let catalog_object = catalog_object
            .context("Catalog root dictionary not found in document objects")?;

        for (object_id, object) in documents_pages.iter() {
            if let Ok(dictionary) = object.as_dict() {
                let mut dictionary = dictionary.clone();
                dictionary.set("Parent", pages_object.0);
                document
                    .objects
                    .insert(*object_id, Object::Dictionary(dictionary));
            }
        }

        if let Ok(dictionary) = pages_object.1.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Count", documents_pages.len() as u32);
            dictionary.set(
                "Kids",
                documents_pages
                    .keys()
                    .map(|&id| Object::Reference(id))
                    .collect::<Vec<_>>(),
            );
            document
                .objects
                .insert(pages_object.0, Object::Dictionary(dictionary));
        }

        if let Ok(dictionary) = catalog_object.1.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Pages", pages_object.0);
            dictionary.remove(b"Outlines");
            document
                .objects
                .insert(catalog_object.0, Object::Dictionary(dictionary));
        }

        document.trailer.set("Root", catalog_object.0);
        document.max_id = document.objects.len() as u32;
        document.renumber_objects();
        document.adjust_zero_pages();

        if let Some(n) = document.build_outline() {
            if let Ok(Object::Dictionary(dict)) = document.get_object_mut(catalog_object.0) {
                dict.set("Outlines", Object::Reference(n));
            }
        }

        document.compress();
        Self::sanitize_doc(&mut document);

        let out_path = output.as_ref();
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        document.save(out_path)?;
        Ok(documents_pages.len() as u32)
    }

    /// Sanitizes the binary mark in a lopdf Document to ensure standard-compliant >=128 bytes.
    pub fn sanitize_doc(doc: &mut Document) {
        if !doc.binary_mark.iter().all(|&b| b >= 128) || doc.binary_mark.len() < 4 {
            doc.binary_mark = vec![0xBB, 0xAD, 0xC0, 0xDE];
        }
    }
}
