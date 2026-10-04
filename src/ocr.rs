use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

/// Checks whether the required OCR tools (`tesseract` and `pdftoppm`) are installed and available in PATH.
pub fn is_ocr_available() -> bool {
    let tesseract_ok = Command::new("tesseract")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let pdftoppm_ok = Command::new("pdftoppm")
        .arg("-v")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    tesseract_ok && pdftoppm_ok
}

/// Renders a specific PDF page into an in-memory PNG stream using `pdftoppm`
/// and pipes directly into `tesseract` for Optical Character Recognition.
/// Operates entirely via kernel pipes with zero intermediate disk writes.
pub fn ocr_page(pdf_path: &Path, page_num: u32, lang: &str) -> Result<String> {
    let mut ppm_child = Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg("150")
        .arg("-f")
        .arg(page_num.to_string())
        .arg("-l")
        .arg(page_num.to_string())
        .arg(pdf_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| "Failed to spawn pdftoppm for page rasterization")?;

    let ppm_stdout = ppm_child
        .stdout
        .take()
        .context("Failed to capture pdftoppm stdout pipe")?;

    let tess_child = Command::new("tesseract")
        .arg("stdin")
        .arg("stdout")
        .arg("-l")
        .arg(lang)
        .stdin(ppm_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| "Failed to spawn tesseract OCR engine")?;

    let output = tess_child
        .wait_with_output()
        .with_context(|| "Failed to wait on tesseract execution")?;

    let _ = ppm_child.wait();

    if !output.status.success() {
        return Ok(String::new());
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(text)
}
