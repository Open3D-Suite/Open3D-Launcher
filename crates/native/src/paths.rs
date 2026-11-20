// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the launcher keeps its own files.
//!
//! Everything but its settings (`app.ron`, see the launcher's `folders.rs`) is in
//! `%LOCALAPPDATA%\Open3D\Launcher` (D-044), or in `Launcher` inside the folder `OPEN3D_DATA_DIR`
//! names; before, it was in `%LOCALAPPDATA%\Open3D Launcher`, which stays in use until the
//! launcher has copied what it keeps of it ([`craft_update::in_use`]). A portable launcher keeps
//! everything in `Open3DLauncherData` beside it.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

/// The launcher's folders.
#[derive(Clone, Debug)]
pub struct Paths {
    /// Records of what the launcher installed, and tarball installs.
    pub data: PathBuf,
    /// Saved release answers and in-progress downloads.
    pub cache: PathBuf,
}

/// The launcher's folder among the Open3D folders: `%APPDATA%\Open3D\Launcher` for its settings,
/// `%LOCALAPPDATA%\Open3D\Launcher` for the rest.
pub const FOLDER: &str = craft_update::suite::LAUNCHER.folder;

pub use craft_update::{TEST_OLD_FOLDERS, exists, in_use, open3d_data_dir_from};

impl Paths {
    /// In portable mode `data` and `cache` in `Open3DLauncherData`; otherwise both are the
    /// launcher's per-user folder ([`per_user_dir`]).
    pub fn new() -> Result<Self, String> {
        if let Some(root) = portable_root() {
            return Ok(Self::under(&root));
        }
        let own = craft_update::Paths::per_user(&craft_update::suite::LAUNCHER)?;
        Ok(Self { data: own.data, cache: own.cache })
    }

    /// For tests: everything under one folder.
    pub(crate) fn under(root: &std::path::Path) -> Self {
        Self { data: root.join("data"), cache: root.join("cache") }
    }

    pub(crate) fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }

    pub(crate) fn release_cache(&self) -> PathBuf {
        self.cache.join("releases.json")
    }

    /// The last answer from Open3D's release feed, for an instant first paint and as a fallback.
    pub(crate) fn feed_cache(&self) -> PathBuf {
        self.cache.join("feed.json")
    }

    /// How long to hold off calling Open3D's API (after failures or HTTP 429), across restarts.
    pub(crate) fn feed_throttle(&self) -> PathBuf {
        self.cache.join("feed-throttle.json")
    }

    /// A picture of an app's details page, saved after its first download: under the revision of the
    /// pictures repository it was fetched from, so a new revision never reads an old picture.
    pub(crate) fn screenshot(&self, app: &str, key: &str) -> PathBuf {
        self.cache.join("screenshots").join(open3d_launcher_core::details::SCREENSHOTS_REV).join(app).join(format!("{key}.jpg"))
    }

    pub(crate) fn manifest(&self) -> PathBuf {
        self.data.join("installed.json")
    }

    /// The launcher's own log files (`open3d-launcher.log` and the previous runs').
    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }

    /// Where tarball installs (FreeBSD, Linux without AppImage) are unpacked: `<data>/apps/<app>`.
    pub(crate) fn apps(&self) -> PathBuf {
        self.data.join("apps")
    }
}

/// The launcher's per-user folder, the one of [`data_folders`] that is in use: also for a
/// portable launcher, which keeps there only the secret launchers of this account share
/// (`single_instance`). `None` when the system has no per-user data folder.
pub fn per_user_dir() -> Option<PathBuf> {
    craft_update::Paths::per_user(&craft_update::suite::LAUNCHER).ok().map(|paths| paths.data)
}

/// The launcher's per-user folder among the Open3D folders and the one it used before them:
/// `%LOCALAPPDATA%\Open3D\Launcher` (or `Launcher` in the folder `OPEN3D_DATA_DIR` names) and
/// `%LOCALAPPDATA%\Open3D Launcher`, for copying the old one over at start.
pub fn data_folders() -> (Option<PathBuf>, Option<PathBuf>) {
    craft_update::per_user_dirs(&craft_update::suite::LAUNCHER)
}

/// Writes `bytes` to `path` via a temporary file and a rename, so a crash never leaves half a file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {} ({e})", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("couldn't create a temporary file in {} ({e})", dir.display()))?;
    tmp.write_all(bytes).map_err(|e| format!("couldn't write {} ({e})", tmp.path().display()))?;
    tmp.persist(path).map_err(|e| format!("couldn't replace {} ({e})", path.display()))?;
    Ok(())
}

/// Portable mode: a `portable.txt` beside the executable keeps everything in
/// `Open3DLauncherData` next to it (the Windows portable zip ships one).
pub fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable.txt").is_file().then(|| dir.join("Open3DLauncherData"))
}

/// The variable that names the folder of `settings.json`, the language every Open3D app shares,
/// and of each app's settings, in place of `%APPDATA%\Open3D`.
pub const SHARED_SETTINGS_VARIABLE: &str = "OPEN3D_SETTINGS_DIR";

/// The variable that names the folder of each Open3D app's other files in place of
/// `%LOCALAPPDATA%\Open3D`.
pub const SHARED_DATA_VARIABLE: &str = craft_update::DATA_DIR_VARIABLE;

/// The folder in `Open3DLauncherData` that a portable launcher gives the programs it opens in place
/// of `%LOCALAPPDATA%\Open3D`, as `Open3DLauncherData` itself stands for `%APPDATA%\Open3D`.
pub const PORTABLE_DATA: &str = "Local";

/// Where a portable launcher keeps the language every Open3D app shares: in its own data folder,
/// so a suite carried on a stick keeps it and writes nothing into the profile of the computer it
/// is plugged into. The programs it opens are told so. `None` for a launcher that is installed, or
/// when `OPEN3D_SETTINGS_DIR` already names a folder: then it is the one every app uses.
pub fn portable_settings_dir() -> Option<PathBuf> {
    unless_named(std::env::var_os(SHARED_SETTINGS_VARIABLE), portable_root())
}

/// The variables a program opened from this launcher is given (D-049): for a portable launcher,
/// `OPEN3D_SETTINGS_DIR` and `OPEN3D_DATA_DIR` naming folders inside `Open3DLauncherData`, so the
/// program keeps its files in the portable folder too. A variable already set is left as it is,
/// and the program inherits it. None for a launcher that is installed.
pub fn portable_variables() -> Vec<(&'static str, PathBuf)> {
    variables_for(std::env::var_os(SHARED_SETTINGS_VARIABLE), std::env::var_os(SHARED_DATA_VARIABLE), portable_root())
}

fn variables_for(settings: Option<OsString>, data: Option<OsString>, portable: Option<PathBuf>) -> Vec<(&'static str, PathBuf)> {
    let settings = unless_named(settings, portable.clone()).map(|dir| (SHARED_SETTINGS_VARIABLE, dir));
    let data = unless_named(data, portable.map(|root| root.join(PORTABLE_DATA))).map(|dir| (SHARED_DATA_VARIABLE, dir));
    settings.into_iter().chain(data).collect()
}

/// `portable`, unless a variable already names a folder.
fn unless_named(named: Option<OsString>, portable: Option<PathBuf>) -> Option<PathBuf> {
    if named.is_some_and(|dir| !dir.is_empty()) { None } else { portable }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portable_launcher_keeps_the_shared_language_in_its_own_folder() {
        let portable = PathBuf::from("Suite").join("Open3DLauncherData");
        assert_eq!(unless_named(None, Some(portable.clone())), Some(portable.clone()));
        assert_eq!(unless_named(Some("".into()), Some(portable.clone())), Some(portable.clone()));
        // A folder the variable names wins; an installed launcher uses the computer's file.
        assert_eq!(unless_named(Some("Elsewhere".into()), Some(portable)), None);
        assert_eq!(unless_named(None, None), None);
    }

    /// A portable launcher gives the programs it opens both folders inside itself (D-049).
    #[test]
    fn a_portable_launcher_gives_the_programs_it_opens_both_folders_inside_itself() {
        let portable = PathBuf::from("Suite").join("Open3DLauncherData");
        let both = vec![(SHARED_SETTINGS_VARIABLE, portable.clone()), (SHARED_DATA_VARIABLE, portable.join("Local"))];
        assert_eq!(variables_for(None, None, Some(portable.clone())), both);
        assert_eq!(variables_for(Some("".into()), Some("".into()), Some(portable.clone())), both);
        // A variable already set is inherited as it is.
        assert_eq!(variables_for(Some("Elsewhere".into()), None, Some(portable.clone())), vec![(SHARED_DATA_VARIABLE, portable.join("Local"))]);
        assert_eq!(variables_for(None, Some("Elsewhere".into()), Some(portable.clone())), vec![(SHARED_SETTINGS_VARIABLE, portable)]);
        // An installed launcher gives none.
        assert_eq!(variables_for(None, None, None), vec![]);
    }

    #[test]
    fn the_launchers_folder_is_named_launcher() {
        assert_eq!(FOLDER, "Launcher");
        assert_eq!(SHARED_DATA_VARIABLE, "OPEN3D_DATA_DIR");
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
