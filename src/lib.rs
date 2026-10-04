pub mod format;
pub mod loader;
pub mod ops;
pub mod types;
pub mod ui;
pub mod unloader;

pub use format::{PluFormat, PLU_MAGIC, PLU_VERSION};
pub use loader::PdfLoader;
pub use ops::PdfOps;
pub use types::{DocumentMeta, DumpFormat, OperationStats, PageData, PluDocument};
pub use ui::run_interactive_ui;
pub use unloader::PdfUnloader;

use anyhow::Result;
use std::path::Path;

/// High-performance concurrent pipeline linking the Loader and Unloader components.
/// Uses a dedicated background thread for the Unloader and a Rayon worker pool for the Loader,
/// streaming extracted pages in real-time through a bounded lock-free channel.
pub fn load_and_unload<P: AsRef<Path>, Q: AsRef<Path>>(
    load_path: P,
    unload_path: Q,
    num_threads: Option<usize>,
) -> Result<OperationStats> {
    let loader = PdfLoader::load_file(load_path)?;
    let total_pages = loader.page_count();
    let meta = loader.metadata().clone();
    let format = PdfUnloader::detect_format(unload_path.as_ref());

    let channel_capacity = (num_threads.unwrap_or(8) * 4).max(32);
    let (tx, rx) = crossbeam_channel::bounded::<PageData>(channel_capacity);
    let unload_path_buf = unload_path.as_ref().to_path_buf();

    // Spawn Unloader on dedicated consumer thread
    let unloader_handle = std::thread::Builder::new()
        .name("plu-unloader".to_string())
        .spawn(move || {
            PdfUnloader::dump_stream(rx, &meta, total_pages, unload_path_buf, format)
        })?;

    // Loader concurrently extracts pages using parallel worker threads
    loader.stream_pages_parallel(tx, num_threads)?;

    // Join Unloader thread
    let stats = unloader_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Unloader thread panicked"))??;

    Ok(stats)
}
