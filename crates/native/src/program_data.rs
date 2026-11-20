// SPDX-License-Identifier: GPL-3.0-or-later
//! The folders of a program's own files, as every Open3D app finds them (`open3d_settings`, the
//! OPEN3D root's `common/settings.md`): its settings in `<App>` inside the folder
//! `OPEN3D_SETTINGS_DIR` names, else `%APPDATA%\Open3D` (D-042); the rest in `<App>` inside the
//! folder `OPEN3D_DATA_DIR` names, else `%LOCALAPPDATA%\Open3D` (D-044). A portable launcher gives
//! the programs it opens both folders inside its own (`paths::portable_variables`), so those are
//! its programs' folders.
//!
//! Open3D's own code, not part of ArtCraft Launcher.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::paths::{SHARED_DATA_VARIABLE, SHARED_SETTINGS_VARIABLE};

/// A program's two folders; `None` where Windows names no folder to put it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Folders {
    pub settings: Option<PathBuf>,
    pub data: Option<PathBuf>,
}

/// The folders of the program named `name` (`OpenFarm`), for this launcher.
pub(crate) fn folders_of(name: &str) -> Folders {
    let given = crate::paths::portable_variables();
    let variable = |variable: &str| {
        given.iter().find(|(v, _)| *v == variable).map(|(_, dir)| dir.clone().into_os_string()).or_else(|| std::env::var_os(variable))
    };
    folders_from(
        name,
        variable(SHARED_SETTINGS_VARIABLE),
        std::env::var_os("APPDATA"),
        variable(SHARED_DATA_VARIABLE),
        std::env::var_os("LOCALAPPDATA"),
    )
}

/// [`folders_of`] from the values of `OPEN3D_SETTINGS_DIR`, `APPDATA`, `OPEN3D_DATA_DIR` and
/// `LOCALAPPDATA`. An empty value counts as not set.
pub(crate) fn folders_from(
    name: &str,
    settings: Option<OsString>,
    appdata: Option<OsString>,
    data: Option<OsString>,
    localappdata: Option<OsString>,
) -> Folders {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty()).map(PathBuf::from);
    let settings = set(settings).or_else(|| set(appdata).map(|appdata| appdata.join("Open3D")));
    let data = craft_update::open3d_data_dir_from(data, localappdata);
    Folders { settings: settings.map(|dir| dir.join(name)), data: data.map(|dir| dir.join(name)) }
}

/// Deletes a program's settings and data folders, when the person asked for that on uninstalling
/// it (D-045), and nothing else: not the shared `settings.json` beside them, not the folders the
/// program used before the Open3D ones. A folder that is a link is unlinked, never followed.
/// `Err` names what is left; whatever could be deleted is gone.
pub(crate) fn delete(folders: &Folders) -> Result<(), String> {
    let mut left = Vec::new();
    for folder in [&folders.settings, &folders.data].into_iter().flatten() {
        match delete_folder(folder) {
            Ok(true) => log::info!("deleted {}", folder.display()),
            Ok(false) => {}
            Err(e) => left.push(format!("{} ({e})", folder.display())),
        }
    }
    if left.is_empty() { Ok(()) } else { Err(left.join("; ")) }
}

/// Deletes `folder` and what is in it; `Ok(false)` when there is no folder of that name.
fn delete_folder(folder: &Path) -> std::io::Result<bool> {
    use std::os::windows::fs::MetadataExt;
    /// A junction or a symbolic link.
    const REPARSE_POINT: u32 = 0x400;
    let meta = match std::fs::symlink_metadata(folder) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if meta.file_attributes() & REPARSE_POINT != 0 {
        std::fs::remove_dir(folder).or_else(|_| std::fs::remove_file(folder))?;
        return Ok(true);
    }
    // Something else of that name isn't the program's folder.
    if !meta.is_dir() {
        return Ok(false);
    }
    std::fs::remove_dir_all(folder)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program's two folders, the shared `settings.json` and another program's folders beside
    /// them, under `root`.
    fn suite(root: &Path) -> Folders {
        let (settings, data) = (root.join("Roaming").join("Open3D"), root.join("Local").join("Open3D"));
        for (top, file) in [(&settings, "config.json"), (&data, "farm.sqlite")] {
            std::fs::create_dir_all(top.join("OpenFarm").join("deep")).unwrap();
            std::fs::write(top.join("OpenFarm").join(file), b"mine").unwrap();
            std::fs::write(top.join("OpenFarm").join("deep").join("more.txt"), b"mine").unwrap();
            std::fs::create_dir_all(top.join("OpenRef")).unwrap();
            std::fs::write(top.join("OpenRef").join("settings.json"), b"another program's").unwrap();
        }
        std::fs::write(settings.join("settings.json"), br#"{"language":"es"}"#).unwrap();
        folders_from("OpenFarm", None, Some(root.join("Roaming").into()), None, Some(root.join("Local").into()))
    }

    #[test]
    fn deleting_a_programs_files_takes_its_two_folders_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = suite(tmp.path());
        // The folder it used before the Open3D folders stays too.
        let old = tmp.path().join("Local").join("OpenFarm");
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(delete(&folders), Ok(()));
        let (settings, data) = (folders.settings.clone().unwrap(), folders.data.clone().unwrap());
        assert!(!settings.exists() && !data.exists());
        assert!(settings.parent().unwrap().join("settings.json").is_file(), "the language every app shares");
        assert!(settings.parent().unwrap().join("OpenRef").join("settings.json").is_file());
        assert!(data.parent().unwrap().join("OpenRef").join("settings.json").is_file());
        assert!(old.is_dir());
        // Nothing left to delete is no error.
        assert_eq!(delete(&folders), Ok(()));
    }

    #[test]
    fn a_folder_that_is_a_link_is_unlinked_and_what_it_points_at_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = suite(tmp.path());
        let data = folders.data.clone().unwrap();
        let elsewhere = tmp.path().join("Elsewhere");
        std::fs::rename(&data, &elsewhere).unwrap();
        // A junction needs no administrator rights.
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&data).arg(&elsewhere).output().unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stdout));
        assert_eq!(delete(&folders), Ok(()));
        assert!(std::fs::symlink_metadata(&data).is_err(), "the link is gone");
        assert!(elsewhere.join("farm.sqlite").is_file(), "what it pointed at stays");
    }

    #[test]
    fn what_cannot_be_deleted_is_named() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let folders = suite(tmp.path());
        let settings = folders.settings.clone().unwrap();
        // Held open without sharing deletion, as a program that still writes it would.
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(settings.join("config.json")).unwrap();
        let left = delete(&folders).unwrap_err();
        assert!(left.contains(&settings.display().to_string()), "{left}");
        assert!(!folders.data.as_ref().unwrap().exists(), "the other folder still goes");
        drop(held);
        assert_eq!(delete(&folders), Ok(()));
    }

    fn os(text: &str) -> Option<OsString> {
        Some(OsString::from(text))
    }

    /// The same folders `open3d_settings::app_settings_dir` and `app_data_dir` give.
    #[test]
    fn a_programs_folders_are_its_name_in_the_open3d_folders() {
        let folders = folders_from("OpenFarm", None, os(r"C:\Roaming"), None, os(r"C:\Local"));
        assert_eq!(folders.settings, Some(PathBuf::from(r"C:\Roaming\Open3D\OpenFarm")));
        assert_eq!(folders.data, Some(PathBuf::from(r"C:\Local\Open3D\OpenFarm")));
        // The variables move them; an empty one counts as not set.
        let moved = folders_from("OpenFarm", os(r"D:\Suite\Data"), os(r"C:\Roaming"), os(r"D:\Suite\Data\Local"), os(r"C:\Local"));
        assert_eq!(moved.settings, Some(PathBuf::from(r"D:\Suite\Data\OpenFarm")));
        assert_eq!(moved.data, Some(PathBuf::from(r"D:\Suite\Data\Local\OpenFarm")));
        assert_eq!(folders_from("OpenFarm", os(""), os(r"C:\Roaming"), os(""), os(r"C:\Local")), folders);
        assert_eq!(folders_from("OpenFarm", None, None, None, None), Folders { settings: None, data: None });
    }
}
