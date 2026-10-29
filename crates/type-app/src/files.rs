use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn read(path: &Path) -> Result<(PathBuf, String)> {
    let path = path
        .canonicalize()
        .with_context(|| format!("Could not open {}", path.display()))?;
    let text =
        fs::read_to_string(&path).context("This file could not be read as UTF-8 Markdown.")?;
    Ok((path, text))
}

/// Resolve aliases for open tabs and in-flight Save As destinations, including new files.
pub fn identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        match (parent.canonicalize(), path.file_name()) {
            (Ok(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    })
}

/// Atomically save UTF-8 text, preserving permissions and symlink targets.
pub fn save(path: &Path, text: &str, expected: Option<&str>) -> Result<PathBuf> {
    // Save to the symlink target.
    let path = if path.exists() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };
    if let Some(expected) = expected {
        let current = fs::read_to_string(&path).context(
            "The original file is no longer readable. Use Save As to keep your changes.",
        )?;
        if current != expected {
            bail!(
                "This file changed on disk. Use Save As to keep your edits in a separate file, or reopen the file to read its latest version."
            );
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .context("Could not create a temporary file beside the document.")?;
    if let Ok(metadata) = fs::metadata(&path) {
        temp.as_file().set_permissions(metadata.permissions())?;
    }
    temp.write_all(text.as_bytes())
        .context("Could not write the document.")?;
    temp.as_file()
        .sync_all()
        .context("Could not flush the document to disk.")?;
    temp.persist(&path)
        .context("Could not replace the destination file.")?;
    Ok(path.canonicalize().unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_exact_source_and_detects_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let text = "# Café 🦀\r\n\r\n**bold**\r\n";
        save(&path, text, None).unwrap();
        assert_eq!(read(&path).unwrap().1, text);
        fs::write(&path, "external change").unwrap();
        assert!(save(&path, "my changes", Some(text)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
    }
    #[test]
    fn failed_save_keeps_existing_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "original").unwrap();
        assert!(save(&path, "new", Some("different baseline")).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }
    #[cfg(unix)]
    #[test]
    fn preserves_symlinks_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("actual.md");
        let link = dir.path().join("link.md");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, &link).unwrap();
        save(&link, "new", Some("old")).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
