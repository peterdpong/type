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

pub fn filename(name: &str, default_extension: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || matches!(name, "." | "..") {
        bail!("Enter a filename.");
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        bail!("Use a filename without slashes, colons, or control characters.");
    }
    if Path::new(name).extension().is_none() {
        Ok(format!("{name}.{default_extension}"))
    } else {
        Ok(name.to_owned())
    }
}

/// Move the existing file without writing the editor buffer or replacing another file.
pub fn rename(path: &Path, name: &str) -> Result<PathBuf> {
    let source = path
        .canonicalize()
        .context("The original file is no longer available.")?;
    let name = filename(
        name,
        source.extension().and_then(|s| s.to_str()).unwrap_or("md"),
    )?;
    let destination = source.with_file_name(name);
    if destination == source {
        return Ok(source);
    }
    if !source.is_file() {
        bail!("Only files can be renamed.");
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, &source, CWD, &destination, RenameFlags::NOREPLACE)
            .map_err(std::io::Error::from)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    anyhow::anyhow!("A file with this name already exists. Choose another name.")
                } else {
                    anyhow::anyhow!(error).context("Could not rename the file.")
                }
            })?;
        Ok(destination)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    bail!("File renaming is not supported on this platform.")
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
    fn rename_preserves_bytes_permissions_and_supports_case_changes() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("note.md");
        let text = "# Café 🦀\r\n";
        fs::write(&old, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&old, fs::Permissions::from_mode(0o640)).unwrap();
        }
        let renamed = rename(&old, "Note.md").unwrap();
        assert_eq!(fs::read_to_string(&renamed).unwrap(), text);
        assert_eq!(
            fs::read_dir(dir.path())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .file_name(),
            "Note.md"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&renamed).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
        let renamed = rename(&renamed, "名字 🦀").unwrap();
        assert_eq!(renamed.file_name().unwrap(), "名字 🦀.md");
        assert_eq!(fs::read_to_string(renamed).unwrap(), text);
    }
    #[test]
    fn rename_never_replaces_an_existing_destination_even_in_a_race() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let (a, b) = std::thread::scope(|scope| {
            let a = scope.spawn(|| rename(&first, "winner.md"));
            let b = scope.spawn(|| rename(&second, "winner.md"));
            (a.join().unwrap(), b.join().unwrap())
        });
        assert_ne!(a.is_ok(), b.is_ok());
        let winner = fs::read_to_string(dir.path().join("winner.md")).unwrap();
        if a.is_ok() {
            assert_eq!(winner, "first");
            assert_eq!(fs::read_to_string(second).unwrap(), "second");
        } else {
            assert_eq!(winner, "second");
            assert_eq!(fs::read_to_string(first).unwrap(), "first");
        }
    }
    #[test]
    fn filename_validation_keeps_renames_in_the_same_directory() {
        for invalid in [
            "",
            "  ",
            ".",
            "..",
            "../other",
            "a/b",
            "a\\b",
            "a:b",
            "bad\0name",
        ] {
            assert!(filename(invalid, "md").is_err(), "{invalid:?}");
        }
        assert_eq!(
            filename("  A name  ", "markdown").unwrap(),
            "A name.markdown"
        );
        assert_eq!(filename("different.txt", "md").unwrap(), "different.txt");
    }
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
