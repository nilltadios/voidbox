//! Self-extracting .voidbox bundle support

use crate::manifest::parse_manifest_str;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const BUNDLE_MAGIC: &[u8; 8] = b"VBOXBNDL";
const BUNDLE_VERSION: u8 = 1;
const FOOTER_LEN: u64 = 8 + 1 + 8;

#[derive(Debug, Clone)]
pub struct BundleManifestInfo {
    pub app_name: String,
    pub display_name: String,
    pub manifest_content: String,
}

#[derive(Debug, Clone)]
pub struct BundleExtracted {
    pub manifest_content: String,
    pub archive_path: PathBuf,
    pub archive_ext: String,
    temp_dir: PathBuf,
}

impl BundleExtracted {
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

#[derive(Error, Debug)]
pub enum BundleError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Invalid bundle: {0}")]
    InvalidBundle(String),

    #[error("Unsupported bundle version: {0}")]
    UnsupportedVersion(u8),

    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

struct BundleFooter {
    payload_len: u64,
    version: u8,
}

pub fn embedded_manifest_info() -> Result<Option<BundleManifestInfo>, BundleError> {
    let exe_path = std::env::current_exe()?;
    manifest_info_from_file(&exe_path)
}

pub fn extract_embedded_bundle() -> Result<Option<BundleExtracted>, BundleError> {
    let exe_path = std::env::current_exe()?;
    let mut file = File::open(&exe_path)?;
    if read_footer(&mut file)?.is_none() {
        return Ok(None);
    }
    extract_bundle_from_file(&exe_path).map(Some)
}

pub fn extract_bundle_from_file(path: &Path) -> Result<BundleExtracted, BundleError> {
    let mut file = File::open(path)?;
    let footer = read_footer(&mut file)?.ok_or_else(|| {
        BundleError::InvalidBundle("bundle footer not found".to_string())
    })?;
    if footer.version != BUNDLE_VERSION {
        return Err(BundleError::UnsupportedVersion(footer.version));
    }

    let file_len = file.metadata()?.len();
    let payload_start = file_len - FOOTER_LEN - footer.payload_len;
    let payload = read_payload_header(&mut file, payload_start, footer.payload_len)?;

    let temp_dir = create_temp_dir()?;
    let archive_path = temp_dir.join(format!("app{}", payload.archive_ext));

    file.seek(SeekFrom::Start(payload.archive_offset))?;
    let mut take = file.take(payload.archive_len);
    let mut out = File::create(&archive_path)?;
    let written = std::io::copy(&mut take, &mut out)?;
    if written != payload.archive_len {
        return Err(BundleError::InvalidBundle(
            "archive payload truncated".to_string(),
        ));
    }

    Ok(BundleExtracted {
        manifest_content: payload.manifest_content,
        archive_path,
        archive_ext: payload.archive_ext,
        temp_dir,
    })
}

pub fn create_bundle(
    manifest_path: &Path,
    archive_path: &Path,
    output_path: &Path,
) -> Result<(), BundleError> {
    let current_exe = std::env::args()
        .next()
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing argv[0]"))?;
    if has_bundle(&current_exe)? {
        return Err(BundleError::InvalidBundle(
            "cannot create bundle from an existing bundle".to_string(),
        ));
    }

    let manifest_content = fs::read_to_string(manifest_path).map_err(|e| {
        BundleError::InvalidBundle(format!(
            "read manifest {}: {}",
            manifest_path.display(),
            e
        ))
    })?;
    let archive_ext = detect_archive_extension(archive_path);

    let manifest_bytes = manifest_content.as_bytes();
    let archive_len = fs::metadata(archive_path)
        .map_err(|e| {
            BundleError::InvalidBundle(format!(
                "stat archive {}: {}",
                archive_path.display(),
                e
            ))
        })?
        .len();
    let ext_bytes = archive_ext.as_bytes();

    if manifest_bytes.len() > u32::MAX as usize {
        return Err(BundleError::InvalidBundle(
            "manifest too large".to_string(),
        ));
    }
    if ext_bytes.len() > u16::MAX as usize {
        return Err(BundleError::InvalidBundle(
            "archive extension too long".to_string(),
        ));
    }

    let mut out = File::create(output_path).map_err(|e| {
        BundleError::InvalidBundle(format!(
            "create output {}: {}",
            output_path.display(),
            e
        ))
    })?;
    let mut self_file = File::open(&current_exe).map_err(|e| {
        BundleError::InvalidBundle(format!(
            "open self {}: {}",
            current_exe.display(),
            e
        ))
    })?;
    std::io::copy(&mut self_file, &mut out).map_err(|e| {
        BundleError::InvalidBundle(format!(
            "copy self to {}: {}",
            output_path.display(),
            e
        ))
    })?;

    out.write_all(&(manifest_bytes.len() as u32).to_le_bytes())
        .map_err(|e| BundleError::InvalidBundle(format!("write manifest len: {}", e)))?;
    out.write_all(manifest_bytes)
        .map_err(|e| BundleError::InvalidBundle(format!("write manifest: {}", e)))?;
    out.write_all(&(ext_bytes.len() as u16).to_le_bytes())
        .map_err(|e| BundleError::InvalidBundle(format!("write ext len: {}", e)))?;
    out.write_all(ext_bytes)
        .map_err(|e| BundleError::InvalidBundle(format!("write ext: {}", e)))?;

    let mut archive_file = File::open(archive_path).map_err(|e| {
        BundleError::InvalidBundle(format!(
            "open archive {}: {}",
            archive_path.display(),
            e
        ))
    })?;
    std::io::copy(&mut archive_file, &mut out).map_err(|e| {
        BundleError::InvalidBundle(format!(
            "append archive {}: {}",
            archive_path.display(),
            e
        ))
    })?;

    let payload_len =
        4u64 + manifest_bytes.len() as u64 + 2u64 + ext_bytes.len() as u64 + archive_len;
    out.write_all(BUNDLE_MAGIC)
        .map_err(|e| BundleError::InvalidBundle(format!("write magic: {}", e)))?;
    out.write_all(&[BUNDLE_VERSION])
        .map_err(|e| BundleError::InvalidBundle(format!("write version: {}", e)))?;
    out.write_all(&payload_len.to_le_bytes())
        .map_err(|e| BundleError::InvalidBundle(format!("write payload len: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(output_path, perms).map_err(|e| {
            BundleError::InvalidBundle(format!(
                "set permissions {}: {}",
                output_path.display(),
                e
            ))
        })?;
    }

    Ok(())
}

pub fn manifest_info_from_file(path: &Path) -> Result<Option<BundleManifestInfo>, BundleError> {
    let mut file = File::open(path)?;
    let footer = match read_footer(&mut file)? {
        Some(footer) => footer,
        None => return Ok(None),
    };
    if footer.version != BUNDLE_VERSION {
        return Err(BundleError::UnsupportedVersion(footer.version));
    }

    let file_len = file.metadata()?.len();
    let payload_start = file_len - FOOTER_LEN - footer.payload_len;
    let payload = read_payload_header(&mut file, payload_start, footer.payload_len)?;

    let manifest = parse_manifest_str(&payload.manifest_content)?;
    Ok(Some(BundleManifestInfo {
        app_name: manifest.app.name,
        display_name: manifest.app.display_name,
        manifest_content: payload.manifest_content,
    }))
}

fn has_bundle(path: &Path) -> Result<bool, BundleError> {
    let mut file = File::open(path)?;
    Ok(read_footer(&mut file)?.is_some())
}

struct PayloadHeader {
    manifest_content: String,
    archive_ext: String,
    archive_offset: u64,
    archive_len: u64,
}

fn read_payload_header(
    file: &mut File,
    payload_start: u64,
    payload_len: u64,
) -> Result<PayloadHeader, BundleError> {
    file.seek(SeekFrom::Start(payload_start))?;

    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)?;
    let manifest_len = u32::from_le_bytes(len_buf) as u64;
    if 4 + manifest_len + 2 > payload_len {
        return Err(BundleError::InvalidBundle(
            "manifest length out of bounds".to_string(),
        ));
    }

    let mut manifest_bytes = vec![0u8; manifest_len as usize];
    file.read_exact(&mut manifest_bytes)?;
    let manifest_content = String::from_utf8(manifest_bytes)?;

    let mut ext_len_buf = [0u8; 2];
    file.read_exact(&mut ext_len_buf)?;
    let ext_len = u16::from_le_bytes(ext_len_buf) as u64;
    if 4 + manifest_len + 2 + ext_len > payload_len {
        return Err(BundleError::InvalidBundle(
            "extension length out of bounds".to_string(),
        ));
    }

    let mut ext_bytes = vec![0u8; ext_len as usize];
    file.read_exact(&mut ext_bytes)?;
    let archive_ext = String::from_utf8(ext_bytes)?;

    let current_pos = file.seek(SeekFrom::Current(0))?;
    let header_len = current_pos
        .checked_sub(payload_start)
        .ok_or_else(|| BundleError::InvalidBundle("invalid payload offsets".to_string()))?;
    let archive_len = payload_len
        .checked_sub(header_len)
        .ok_or_else(|| BundleError::InvalidBundle("invalid payload size".to_string()))?;

    Ok(PayloadHeader {
        manifest_content,
        archive_ext,
        archive_offset: current_pos,
        archive_len,
    })
}

fn read_footer(file: &mut File) -> Result<Option<BundleFooter>, BundleError> {
    let len = file.metadata()?.len();
    if len < FOOTER_LEN {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != BUNDLE_MAGIC {
        return Ok(None);
    }

    let mut version_buf = [0u8; 1];
    file.read_exact(&mut version_buf)?;
    let version = version_buf[0];

    let mut payload_buf = [0u8; 8];
    file.read_exact(&mut payload_buf)?;
    let payload_len = u64::from_le_bytes(payload_buf);

    if payload_len + FOOTER_LEN > len {
        return Err(BundleError::InvalidBundle(
            "payload length out of bounds".to_string(),
        ));
    }

    Ok(Some(BundleFooter { payload_len, version }))
}

fn create_temp_dir() -> Result<PathBuf, BundleError> {
    let mut dir = std::env::temp_dir();
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    dir.push(format!(
        "voidbox-bundle-{}-{}",
        std::process::id(),
        since_epoch
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn detect_archive_extension(path: &Path) -> String {
    let name = path.to_string_lossy();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        ".tar.gz".to_string()
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        ".tar.xz".to_string()
    } else if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        ".tar.zst".to_string()
    } else if name.ends_with(".zip") {
        ".zip".to_string()
    } else {
        ".zip".to_string()
    }
}
