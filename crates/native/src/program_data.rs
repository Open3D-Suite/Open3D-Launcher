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
use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;

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
