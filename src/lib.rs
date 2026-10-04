pub mod format;
pub mod loader;
pub mod types;
pub mod unloader;

pub use format::{PluFormat, PLU_MAGIC, PLU_VERSION};
pub use loader::PdfLoader;
pub use types::{DocumentMeta, DumpFormat, OperationStats, PageData, PluDocument};
pub use unloader::PdfUnloader;

use anyhow::Result;
use std::path::Path;

/// High-performance pipeline linking the Loader and Unloader components.
/// Loads PDF, extracts content page-by-page concurrently, and dumps it to the destination file.
pub fn load_and_unload<P: AsRef<Path>, Q: AsRef<Path>>(
    load_path: P,
    unload_path: Q,
    num_threads: Option<usize>,
) -> Result<OperationStats> {
    let loader = PdfLoader::load_file(load_path)?;
    let pages = loader.extract_all_parallel(num_threads)?;

    let doc = PluDocument::new(
        loader.source_path().to_string_lossy().to_string(),
        loader.metadata().title.clone(),
        loader.metadata().author.clone(),
        pages,
    );

    PdfUnloader::dump_to_file(&doc, unload_path)
}
