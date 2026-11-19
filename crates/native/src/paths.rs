// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the launcher keeps its own files.

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

const DIR: &str = if cfg!(any(target_os = "macos", target_os = "windows")) { "Open3D Launcher" } else { "open3d-launcher" };

impl Paths {
    pub fn new() -> Result<Self, String> {
        if let Some(root) = portable_root() {
            return Ok(Self::under(&root));
        }
        let data = dirs::data_local_dir().ok_or("no per-user data folder on this system")?.join(DIR);
        let cache = dirs::cache_dir().ok_or("no per-user cache folder on this system")?.join(DIR);
        Ok(Self { data, cache })
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
/// in place of `%APPDATA%\Open3D`.
pub const SHARED_SETTINGS_VARIABLE: &str = "OPEN3D_SETTINGS_DIR";

/// Where a portable launcher keeps the language every Open3D app shares: in its own data folder,
/// so a suite carried on a stick keeps it and writes nothing into the profile of the computer it
/// is plugged into. The programs it opens are told so. `None` for a launcher that is installed, or
/// when `OPEN3D_SETTINGS_DIR` already names a folder: then it is the one every app uses.
pub fn portable_settings_dir() -> Option<PathBuf> {
    settings_dir_of(std::env::var_os(SHARED_SETTINGS_VARIABLE), portable_root())
}

fn settings_dir_of(named: Option<std::ffi::OsString>, portable: Option<PathBuf>) -> Option<PathBuf> {
    if named.is_some_and(|dir| !dir.is_empty()) { None } else { portable }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portable_launcher_keeps_the_shared_language_in_its_own_folder() {
        let portable = PathBuf::from("Suite").join("Open3DLauncherData");
        assert_eq!(settings_dir_of(None, Some(portable.clone())), Some(portable.clone()));
        assert_eq!(settings_dir_of(Some("".into()), Some(portable.clone())), Some(portable.clone()));
        // A folder the variable names wins; an installed launcher uses the computer's file.
        assert_eq!(settings_dir_of(Some("Elsewhere".into()), Some(portable)), None);
        assert_eq!(settings_dir_of(None, None), None);
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
