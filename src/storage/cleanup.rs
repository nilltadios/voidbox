//! Filesystem cleanup helpers

use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Remove a directory tree, relaxing permissions if needed.
pub fn remove_dir_all_force(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            make_tree_writable(path)?;
            fs::remove_dir_all(path)
        }
        Err(err) => Err(err),
    }
}

fn make_tree_writable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use walkdir::WalkDir;

        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));

        let work_dir = path.join("work");
        let work_inner = work_dir.join("work");
        if work_dir.exists() {
            let _ = fs::set_permissions(&work_dir, fs::Permissions::from_mode(0o700));
        }
        if work_inner.exists() {
            let _ = fs::set_permissions(&work_inner, fs::Permissions::from_mode(0o700));
        }

        for entry in WalkDir::new(path).follow_links(false).contents_first(true) {
            match entry {
                Ok(entry) => {
                    let meta = entry.path().symlink_metadata();
                    if let Ok(meta) = meta {
                        let mode = if meta.is_dir() { 0o700 } else { 0o600 };
                        let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(mode));
                    }
                }
                Err(err) => {
                    if let Some(path) = err.path() {
                        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                    }
                }
            }
        }
    }

    Ok(())
}
