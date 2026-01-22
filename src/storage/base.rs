//! Base image metadata storage

use crate::storage::paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseInfo {
    pub base: String,
    pub arch: String,
    pub version: String,
}

#[derive(Error, Debug)]
pub enum BaseInfoError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Invalid rootfs path: {0}")]
    InvalidPath(PathBuf),
}

pub fn write_base_info(app_name: &str, info: &BaseInfo) -> Result<(), BaseInfoError> {
    let path = paths::app_base_info_path(app_name);
    let content = serde_json::to_string_pretty(info)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn read_base_info_for_rootfs(rootfs: &Path) -> Result<Option<BaseInfo>, BaseInfoError> {
    let app_dir = rootfs
        .parent()
        .ok_or_else(|| BaseInfoError::InvalidPath(rootfs.to_path_buf()))?;
    let info_path = app_dir.join("base.json");
    if !info_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(info_path)?;
    let info = serde_json::from_str(&content)?;
    Ok(Some(info))
}
