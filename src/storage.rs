//! Portable storage and non-destructive import of the former user-profile index.
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, backup::Backup};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn portable_data_dir(executable: &Path) -> Result<PathBuf> {
    let parent = executable
        .parent()
        .context("Cannot locate the executable directory")?;
    // Keep data outside a signed macOS app bundle so moving/upgrading the bundle
    // does not invalidate its signature or discard the user's index.
    if parent.file_name().is_some_and(|n| n == "MacOS")
        && let Some(contents) = parent.parent()
        && contents.file_name().is_some_and(|n| n == "Contents")
        && let Some(bundle) = contents.parent()
        && bundle.extension().is_some_and(|n| n == "app")
    {
        return Ok(bundle
            .parent()
            .context("Cannot locate app bundle directory")?
            .join("data"));
    }
    Ok(parent.join("data"))
}

pub fn legacy_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("zone", "pom", "CacheCat")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
}

/// Only a new default portable index imports the old location. Explicit
/// --data-dir paths and existing portable databases are never merged or replaced.
pub fn import_source(destination: &Path) -> Option<PathBuf> {
    if destination.join("index.sqlite3").exists() {
        return None;
    }
    legacy_data_dir()
        .filter(|source| source != destination && source.join("index.sqlite3").is_file())
}

fn persist_new(file: tempfile::NamedTempFile, destination: &Path) -> Result<()> {
    match file.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.error.into()),
    }
}

fn copy_new(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        return Ok(());
    }
    let parent = destination
        .parent()
        .context("Missing destination directory")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut fs::File::open(source)?, temporary.as_file_mut())?;
    persist_new(temporary, destination)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        } else if kind.is_file() && !entry.path().extension().is_some_and(|ext| ext == "partial") {
            copy_new(&entry.path(), &destination.join(entry.file_name()))?;
        }
    }
    Ok(())
}

pub fn import_legacy(
    source: &Path,
    destination: &Path,
    mut progress: impl FnMut(String),
) -> Result<()> {
    let target_db = destination.join("index.sqlite3");
    if source == destination || target_db.exists() {
        return Ok(());
    }
    fs::create_dir_all(destination).context(
        "Cannot write portable data; move CacheCat to a writable folder or use --data-dir",
    )?;
    progress("Importing existing models into portable storage…".into());
    copy_tree(&source.join("models"), &destination.join("models"))?;
    if source.join("settings.json").is_file() {
        copy_new(
            &source.join("settings.json"),
            &destination.join("settings.json"),
        )?;
    }
    progress("Importing existing embeddings into portable storage…".into());
    // SQLite backup includes committed WAL contents even if the old app is open.
    // Copying index.sqlite3 alone can silently lose the most recent embeddings.
    let source_db = Connection::open_with_flags(
        source.join("index.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    source_db.busy_timeout(Duration::from_secs(5))?;
    let temporary = tempfile::NamedTempFile::new_in(destination)?;
    {
        let mut target = Connection::open(temporary.path())?;
        Backup::new(&source_db, &mut target)?.run_to_completion(
            128,
            Duration::from_millis(5),
            None,
        )?;
    }
    // Publish the database last: an interrupted import can resume safely, skipping
    // completed files. Existing portable data and the original profile stay intact.
    persist_new(temporary, &target_db)?;
    progress("Existing index imported into portable storage".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_paths_follow_executable_and_keep_macos_bundle_intact() {
        assert_eq!(
            portable_data_dir(Path::new("/portable/cachecat.exe")).unwrap(),
            Path::new("/portable/data")
        );
        assert_eq!(
            portable_data_dir(Path::new("/portable/CacheCat.app/Contents/MacOS/cachecat")).unwrap(),
            Path::new("/portable/data")
        );
    }
    #[test]
    fn import_includes_live_wal_and_does_not_replace_existing_data() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("old");
        let destination = root.path().join("portable");
        fs::create_dir_all(source.join("models")).unwrap();
        fs::create_dir_all(source.join("thumbnails")).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("models/model.onnx"), b"model").unwrap();
        fs::write(source.join("thumbnails/thumb.png"), b"thumb").unwrap();
        fs::write(source.join("settings.json"), b"old settings").unwrap();
        fs::write(destination.join("settings.json"), b"portable settings").unwrap();
        let db = Connection::open(source.join("index.sqlite3")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sample(value TEXT); INSERT INTO sample VALUES('saved embedding');").unwrap();
        assert!(source.join("index.sqlite3-wal").metadata().unwrap().len() > 0);
        import_legacy(&source, &destination, |_| {}).unwrap();
        let imported = Connection::open(destination.join("index.sqlite3")).unwrap();
        assert_eq!(
            imported
                .query_row("SELECT value FROM sample", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "saved embedding"
        );
        assert_eq!(
            fs::read(destination.join("settings.json")).unwrap(),
            b"portable settings"
        );
        assert_eq!(
            fs::read(destination.join("models/model.onnx")).unwrap(),
            b"model"
        );
        assert!(!destination.join("thumbnails").exists());
        assert_eq!(
            fs::read(source.join("thumbnails/thumb.png")).unwrap(),
            b"thumb"
        );
        db.execute_batch("INSERT INTO sample VALUES('later');")
            .unwrap();
        import_legacy(&source, &destination, |_| {}).unwrap();
        assert_eq!(
            imported
                .query_row("SELECT COUNT(*) FROM sample", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sample", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
