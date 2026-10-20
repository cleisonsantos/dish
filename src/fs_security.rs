//! Small filesystem primitives for Dish-owned cache and state files.
//!
//! These paths live in a private directory, but a hostile local process (or an
//! old leftover file) can still place a symlink there. Writes use `create_new`
//! for the temporary file and replace the destination with `rename`, so the
//! final path is a file Dish created, never a symlink followed by `write`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a Dish-owned directory with private permissions and reject a
/// symlink at the final component. Parent directories keep their own policy.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Dish directory is a symlink or not a directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `bytes` atomically to `path`, creating the parent as private.
///
/// The temporary name is unpredictable and opened with `create_new`; if an
/// attacker pre-creates a symlink at that name the open fails instead of
/// following it. `rename` replaces a destination symlink rather than writing
/// through it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();
    ensure_private_dir(parent)?;

    let temporary = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let written = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dish-fs-security-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn write_replaces_a_file_and_creates_a_private_directory() {
        let root = root("write");
        let path = root.join("nested/state.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(root.join("nested"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_replaces_a_symlink_without_touching_its_target() {
        use std::os::unix::fs::symlink;
        let root = root("symlink");
        let target = root.join("target");
        let destination = root.join("state.json");
        fs::write(&target, b"safe").unwrap();
        symlink(&target, &destination).unwrap();

        write_atomic(&destination, b"new state").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"safe");
        assert_eq!(fs::read(&destination).unwrap(), b"new state");
        assert!(!fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_directory_rejects_a_symlink_at_the_final_component() {
        use std::os::unix::fs::symlink;
        let root = root("dir-symlink");
        let real = root.join("real");
        let link = root.join("link");
        fs::create_dir(&real).unwrap();
        symlink(&real, &link).unwrap();
        assert!(ensure_private_dir(&link).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
