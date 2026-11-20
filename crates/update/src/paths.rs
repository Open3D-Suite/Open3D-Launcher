// SPDX-License-Identifier: GPL-3.0-or-later
//! Where an installer keeps its own files: downloads, records, logs.
//!
//! An app keeps them in its folder among the Open3D folders, `%LOCALAPPDATA%\Open3D\<folder>`, or
//! `<folder>` inside the folder `OPEN3D_DATA_DIR` names (D-044; `app_data_dir` in the OPEN3D root's
//! `common/settings.md`): `%LOCALAPPDATA%\Open3D\Launcher` for the launcher. Before, they were in
//! `%LOCALAPPDATA%\<name>`. The launcher copies what it keeps of that folder into the new one at
//! its first start; until then, or while that copy fails, the old folder stays in use ([`in_use`]),
//! so the update helper always looks where the launcher does.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::app::App;

/// The variable that names the folder in place of `%LOCALAPPDATA%\Open3D`, for every Open3D app.
pub const DATA_DIR_VARIABLE: &str = "OPEN3D_DATA_DIR";

/// Only debug builds read it: a folder that stands for the user's `AppData` in a test, whose
/// `Local` and `Roaming` hold the old folders, so that a test never reads the person's own.
pub const TEST_OLD_FOLDERS: &str = "OPEN3D_LAUNCHER_TEST_OLD_FOLDERS";

/// An installer's folders.
#[derive(Clone, Debug)]
pub struct Paths {
    /// Records of what was installed, and tarball installs.
    pub data: PathBuf,
    /// Saved release answers and in-progress downloads.
    pub cache: PathBuf,
}

impl Paths {
    /// Discovers paths for an app from a specific executable path.
    pub fn for_exe(app: &App, exe: &Path) -> Result<Self, String> {
        if let Some(parent) = exe.parent()
            && parent.join("portable.txt").is_file()
        {
            let data_dir_name = format!("{}Data", app.name.replace(' ', ""));
            return Ok(Self::under(&parent.join(data_dir_name)));
        }
        Self::per_user(app)
    }

    /// The per-user folders of `app`: `data` and `cache` are both the one of its two folders
    /// ([`per_user_dirs`]) that is in use ([`in_use`]).
    pub fn per_user(app: &App) -> Result<Self, String> {
        let (new, old) = per_user_dirs(app);
        let dir = in_use(new, old).ok_or("no per-user data folder on this system")?;
        Ok(Self { data: dir.clone(), cache: dir })
    }

    /// Everything under one folder (portable mode, tests).
    pub fn under(root: &std::path::Path) -> Self {
        Self { data: root.join("data"), cache: root.join("cache") }
    }

    pub fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }

    /// Log files, and the error of a failed self-update waiting for the next start.
    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }

    /// Where tarball installs (FreeBSD, Linux without AppImage) are unpacked: `<data>/apps/<app>`.
    pub fn apps(&self) -> PathBuf {
        self.data.join("apps")
    }
}

/// `app`'s own folder among the Open3D folders, `<folder>` in [`open3d_data_dir`], and the
/// per-user folder it used before them: `%LOCALAPPDATA%\<name>` on Windows (`<id>` in the per-user
/// data folder off macOS and Windows), or `Local\<name>` in the folder [`TEST_OLD_FOLDERS`] names
/// in a debug build.
pub fn per_user_dirs(app: &App) -> (Option<PathBuf>, Option<PathBuf>) {
    let new = open3d_data_dir().map(|dir| dir.join(app.folder));
    let old = if cfg!(any(target_os = "macos", target_os = "windows")) { app.name } else { app.id };
    #[cfg(debug_assertions)]
    if let Some(test) = std::env::var_os(TEST_OLD_FOLDERS).filter(|test| !test.is_empty()) {
        return (new, Some(PathBuf::from(test).join("Local").join(old)));
    }
    (new, crate::folders::data_local_dir().map(|local| local.join(old)))
}

/// The folder of every Open3D app's other files: the one `OPEN3D_DATA_DIR` names, otherwise
/// `%LOCALAPPDATA%\Open3D` (`data_dir` in `common/settings.md`). `None` when neither is set.
pub fn open3d_data_dir() -> Option<PathBuf> {
    open3d_data_dir_from(std::env::var_os(DATA_DIR_VARIABLE), std::env::var_os("LOCALAPPDATA"))
}

/// [`open3d_data_dir`] from the values of `OPEN3D_DATA_DIR` and `LOCALAPPDATA`. An empty value
/// counts as not set.
pub fn open3d_data_dir_from(variable: Option<OsString>, localappdata: Option<OsString>) -> Option<PathBuf> {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty()).map(PathBuf::from);
    set(variable).or_else(|| set(localappdata).map(|local| local.join("Open3D")))
}

/// Which of an app's two folders is in use: `new`, its folder among the Open3D folders; but `old`,
/// the one it used before, while `old` is a folder and nothing has the name `new`. That is so
/// before the app has first copied `old` into `new`, and when that copy failed: then this run goes
/// on with `old`, and the next start tries again (D-049). `old` when there is no `new`.
pub fn in_use(new: Option<PathBuf>, old: Option<PathBuf>) -> Option<PathBuf> {
    match (new, old) {
        (Some(new), Some(old)) if !exists(&new) && old.is_dir() => Some(old),
        (Some(new), _) => Some(new),
        (None, old) => old,
    }
}

/// Whether something has the name `path`: a folder, a file or a link. When that cannot be told, it
/// counts as there, as it does for copying the old folder over.
pub fn exists(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) => !matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory),
    }
}

/// Writes `bytes` to `path` via a temporary file and a rename, so a crash never leaves half a file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {} ({e})", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("couldn't create a temporary file in {} ({e})", dir.display()))?;
    tmp.write_all(bytes).map_err(|e| format!("couldn't write {} ({e})", tmp.path().display()))?;
    tmp.as_file().sync_all().map_err(|e| format!("couldn't sync {} ({e})", tmp.path().display()))?;
    tmp.persist(path).map_err(|e| format!("couldn't replace {} ({e})", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open3d_data_folder_is_the_variable_then_localappdata() {
        let os = |text: &str| Some(OsString::from(text));
        assert_eq!(open3d_data_dir_from(os("D:\\Suite\\Data"), os("C:\\Local")), Some(PathBuf::from("D:\\Suite\\Data")));
        assert_eq!(open3d_data_dir_from(None, os("C:\\Local")), Some(Path::new("C:\\Local").join("Open3D")));
        assert_eq!(open3d_data_dir_from(os(""), os("C:\\Local")), Some(Path::new("C:\\Local").join("Open3D")));
        assert_eq!(open3d_data_dir_from(None, os("")), None);
        assert_eq!(open3d_data_dir_from(None, None), None);
    }

    #[test]
    fn the_launcher_has_a_folder_of_its_own_among_the_open3d_folders() {
        assert_eq!(crate::suite::LAUNCHER.folder, "Launcher");
        assert_eq!(crate::suite::OPENTOOLS.folder, "OpenTools");
    }

    /// The new folder once it is there; the old one only while it is there and the new one is not.
    #[test]
    fn the_old_folder_is_in_use_only_until_the_new_one_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let (new, old) = (dir.path().join("Open3D").join("Launcher"), dir.path().join("Open3D Launcher"));
        // Neither is there: the new one, which the app creates as it writes.
        assert_eq!(in_use(Some(new.clone()), Some(old.clone())), Some(new.clone()));
        // Only the old one: copying it over has not been done, or failed.
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(in_use(Some(new.clone()), Some(old.clone())), Some(old.clone()));
        // Both, or the new one alone, even when it is not a folder: the new one.
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(in_use(Some(new.clone()), Some(old.clone())), Some(new.clone()));
        let file = dir.path().join("a file");
        std::fs::write(&file, b"a file").unwrap();
        assert_eq!(in_use(Some(file.clone()), Some(old.clone())), Some(file));
        // An old name that is a file is no folder to go on with.
        assert_eq!(in_use(Some(dir.path().join("missing")), Some(dir.path().join("a file"))), Some(dir.path().join("missing")));
        // Without a new folder, the old one.
        assert_eq!(in_use(None, Some(old.clone())), Some(old));
        assert_eq!(in_use(None, None), None);
    }

    #[test]
    fn atomic_write_does_not_overwrite_an_unrelated_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installed.json");
        let unrelated = path.with_extension("tmp");
        std::fs::write(&unrelated, b"another writer's data").unwrap();
        write_atomic(&path, b"new manifest").unwrap();
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"another writer's data");
        assert_eq!(std::fs::read(&path).unwrap(), b"new manifest");
    }

    #[test]
    fn atomic_write_replaces_a_complete_file_and_cleans_up_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installed.json");
        std::fs::write(&path, b"old manifest").unwrap();
        write_atomic(&path, b"new manifest").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new manifest");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        let blocked = dir.path().join("directory.json");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep.txt"), b"keep").unwrap();
        assert!(write_atomic(&blocked, b"replacement").is_err());
        assert_eq!(std::fs::read(blocked.join("keep.txt")).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
