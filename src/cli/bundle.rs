//! Bundle command implementation

use crate::bundle;
use crate::manifest::parse_manifest_str;
use crate::storage::paths;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BundleCliError {
    #[error("Bundle error: {0}")]
    BundleError(#[from] crate::bundle::BundleError),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Install error: {0}")]
    InstallError(#[from] crate::cli::InstallError),

    #[error("Run error: {0}")]
    RunError(#[from] crate::cli::RunError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub fn bundle_create(
    manifest_path: &Path,
    archive_path: &Path,
    output_path: Option<&Path>,
) -> Result<(), BundleCliError> {
    let output = output_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| {
            let name = manifest_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("app");
            Path::new(&format!("{}.voidbox", name)).to_path_buf()
        });

    println!(
        "[voidbox] Creating bundle: {}",
        output.to_string_lossy()
    );
    bundle::create_bundle(manifest_path, archive_path, &output)?;
    println!("[voidbox] Bundle created successfully.");
    Ok(())
}

pub fn bundle_install(bundle_path: &Path, run: bool) -> Result<(), BundleCliError> {
    let extracted = bundle::extract_bundle_from_file(bundle_path)?;
    let manifest_content = extracted.manifest_content.clone();
    let manifest = parse_manifest_str(&manifest_content)?;

    paths::ensure_dirs()?;
    let install_result = crate::cli::install_app_from_bundle(
        &manifest_content,
        &extracted.archive_path,
        &extracted.archive_ext,
        false,
    );
    extracted.cleanup();
    install_result?;

    if run {
        crate::cli::run_app(&manifest.app.name, &[], None, false)?;
    }

    Ok(())
}
