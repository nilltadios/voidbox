//! File download utilities

use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Download failed: {0}")]
    Failed(String),
}

/// Download a file with progress bar
pub fn download_file(url: &str, dest: &Path, show_progress: bool) -> Result<(), DownloadError> {
    let mut resp = ureq::get(url)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .map_err(|e| DownloadError::HttpError(e.to_string()))?;

    let total_size = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let pb = if show_progress && total_size > 0 {
        let pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"));
        Some(pb)
    } else {
        None
    };

    let mut out = File::create(dest)?;
    let mut reader = resp.body_mut().with_config().limit(1_000_000_000).reader();
    let mut buffer = vec![0u8; 8192];
    let mut downloaded = 0u64;

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n])?;
        downloaded += n as u64;

        if let Some(ref pb) = pb {
            pb.set_position(downloaded);
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Download complete");
    }

    Ok(())
}

/// Download content to string
pub fn download_string(url: &str) -> Result<String, DownloadError> {
    let mut resp = ureq::get(url)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .map_err(|e| DownloadError::HttpError(e.to_string()))?;

    let content = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| DownloadError::Failed(e.to_string()))?;

    Ok(content)
}
