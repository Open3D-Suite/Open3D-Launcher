// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the launcher keeps its files, and carrying its old folders over at start.
//!
//! Its settings, eframe's `app.ron`, are in `%APPDATA%\Open3D\Launcher` (D-042), and everything
//! else in `%LOCALAPPDATA%\Open3D\Launcher` (D-044, `open3d_launcher_native::paths`), or in
//! `Launcher` inside the folders that `OPEN3D_SETTINGS_DIR` and `OPEN3D_DATA_DIR` name. A portable
//! launcher keeps both in `Open3DLauncherData` beside it, as before, and copies nothing.
//!
//! At start, once this is the only launcher running and before anything is written into the new
//! folders, [`carry_over`] copies what a launcher from before kept in its old ones, once, and leaves
//! those as they were (D-048, D-049): the whole of eframe's own folder,
//! `%APPDATA%\io.github.carlesllavata.open3d-launcher\data`, and of `%LOCALAPPDATA%\Open3D Launcher`
//! only what holds the person's installs ([`KEPT`]). Its downloads, saved release answers, pictures
//! and logs are made again. When a copy fails, that run goes on with the old folder, as before, and
//! the next start tries again ([`open3d_launcher_native::paths::in_use`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use open3d_launcher_native::paths as native;
use open3d_launcher_ui_egui::open3d_settings::{self, CarryOver};

/// eframe's file in the settings folder.
pub const SETTINGS_FILE: &str = "app.ron";

/// What is copied of the old data folder, by its path inside it: what holds the state of the
/// person's installs and of the launcher's own. The rest (downloads, saved release answers and
/// catalog, pictures, logs, the update helper's copy) is made again when it is needed.
pub const KEPT: [&str; 5] = [
    // The apps the launcher installed.
    "installed.json",
    // This account's secret, which its launchers prove to each other (`single_instance`).
    ".instance-token",
    // A self-update just installed, so that it is not offered again.
    "self-update.txt",
    // The version a finished self-update installed, for that version's first start.
    "self-update-done.txt",
    // Why the last self-update failed, for the next start to say.
    "logs/self-update-error.txt",
];

/// The folder of the launcher's settings in use: `%APPDATA%\Open3D\Launcher`, or eframe's own
/// folder from before while the new one is not there (see [`native::in_use`]).
pub fn settings_dir(app_id: &str) -> Option<PathBuf> {
    native::in_use(open3d_settings::app_settings_dir(native::FOLDER), old_settings_dir(app_id))
}

/// eframe's own folder for `app.ron`, where launchers kept their settings before, or
/// `Roaming\<app_id>\data` in the folder `OPEN3D_LAUNCHER_TEST_OLD_FOLDERS` names in a debug build.
fn old_settings_dir(app_id: &str) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(test) = std::env::var_os(native::TEST_OLD_FOLDERS).filter(|test| !test.is_empty()) {
        return Some(PathBuf::from(test).join("Roaming").join(app_id).join("data"));
    }
    eframe::storage_dir(app_id)
}

/// Copies the launcher's old folders into its new ones, the first time it starts with them, and
/// gives what could not be copied, to tell the person. Call it once, at start, before anything is
/// written into the new folders. A portable launcher copies nothing.
pub fn carry_over(app_id: &str) -> Vec<String> {
    if native::portable_root().is_some() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    if let (Some(old), Some(new)) = (old_settings_dir(app_id), open3d_settings::app_settings_dir(native::FOLDER)) {
        problems.extend(said(&old, &new, open3d_settings::carry_over(&old, &new)));
    }
    if let (Some(new), Some(old)) = native::data_folders() {
        problems.extend(said(&old, &new, carry_kept(&old, &new, &KEPT)));
    }
    problems
}

/// Logs what carrying `old` over into `new` came to, and gives what the person is told: the files
/// left out, or why nothing could be copied (then the old folder is used, and the next start tries
/// again).
fn said(old: &Path, new: &Path, outcome: CarryOver) -> Option<String> {
    match outcome {
        CarryOver::Nothing => None,
        CarryOver::Copied { files, left_out } => {
            log::info!("copied {files} files of {} to {}", old.display(), new.display());
            if left_out.is_empty() {
                return None;
            }
            let left_out = left_out.iter().map(|(path, why)| format!("{} ({why})", old.join(path).display())).collect::<Vec<_>>().join(", ");
            log::warn!("left out of the copy: {left_out}");
            Some(left_out)
        }
        CarryOver::Failed(reason) => {
            log::warn!("couldn't copy {} to {}, so this time the launcher keeps using it: {reason}", old.display(), new.display());
            Some(reason)
        }
    }
}

/// Copies of the folder `old` only the files `kept` names (with `/` between folders) into the new
/// folder `new`, as `open3d_settings::carry_over` copies a whole folder, with the same promises:
/// only when nothing has the name `new` and `old` is a folder; `old` is left as it was; a copy cut
/// half way is never taken for a finished one. The kept files are first copied into a folder of
/// their own beside `new`, which `carry_over` then copies into `new`, and which is then removed.
/// A kept file that is missing is no error; one that cannot be read is left out and said.
fn carry_kept(old: &Path, new: &Path, kept: &[&str]) -> CarryOver {
    if native::exists(new) || !old.is_dir() {
        return CarryOver::Nothing;
    }
    let absolute = |path: &Path| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if absolute(new).starts_with(absolute(old)) {
        return CarryOver::Failed(format!("{} is inside {}", new.display(), old.display()));
    }
    let (picked, picked_out) = match pick(old, new, kept) {
        Ok(picked) => picked,
        Err(reason) => return CarryOver::Failed(reason),
    };
    let outcome = open3d_settings::carry_over(&picked, new);
    let _ = std::fs::remove_dir_all(&picked);
    match outcome {
        CarryOver::Copied { files, left_out } => CarryOver::Copied { files, left_out: picked_out.into_iter().chain(left_out).collect() },
        other => other,
    }
}

/// Copies the files `kept` names from `old` into a new folder beside `new`, and gives that folder
/// and the files left out, each with why. The error stops the copy: the folder cannot be made, or
/// a file cannot be written; then nothing is left behind.
fn pick(old: &Path, new: &Path, kept: &[&str]) -> Result<(PathBuf, Vec<(PathBuf, String)>), String> {
    let (Some(parent), Some(name)) = (new.parent(), new.file_name()) else {
        return Err(format!("{} cannot be a folder of its own", new.display()));
    };
    let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
    let picked = std::fs::create_dir_all(parent)
        .and_then(|()| new_folder(parent, name))
        .map_err(|e| format!("couldn't create a folder in {} ({e})", parent.display()))?;
    let mut left_out = Vec::new();
    for file in kept {
        let inside: PathBuf = file.split('/').collect();
        let from = old.join(&inside);
        // Only a plain file: a link is not followed, as `carry_over` does not follow one.
        match std::fs::symlink_metadata(&from) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                left_out.push((inside, "not a plain file, so not copied".to_string()));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                left_out.push((inside, e.to_string()));
                continue;
            }
        }
        let to = picked.join(&inside);
        let copied = to.parent().map_or(Ok(()), std::fs::create_dir_all).map_err(|e| format!("couldn't write {} ({e})", to.display()));
        match copied.and_then(|()| copy_file(&from, &to)) {
            Ok(None) => {}
            Ok(Some(why)) => left_out.push((inside, why)),
            Err(reason) => {
                let _ = std::fs::remove_dir_all(&picked);
                return Err(reason);
            }
        }
    }
    Ok((picked, left_out))
}

/// A new, empty folder of its own in `dir`, named after `name`.
fn new_folder(dir: &Path, name: &OsStr) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_nanos());
    for attempt in 0..100 {
        let mut folder = name.to_os_string();
        folder.push(format!(".pick-{}-{stamp}-{attempt}.tmp", std::process::id()));
        let path = dir.join(folder);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "every name tried is taken"))
}

/// Copies the file `from` into the new file `to`, with the time it was last changed. `Ok(Some(why))`
/// when `from` cannot be read; the error is one of writing. `from` is opened as other programs may
/// still use it.
fn copy_file(from: &Path, to: &Path) -> Result<Option<String>, String> {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    let read = std::fs::File::open(from).and_then(|mut file| {
        let modified = file.metadata().and_then(|metadata| metadata.modified()).ok();
        file.read_to_end(&mut bytes).map(|_| modified)
    });
    let modified = match read {
        Ok(modified) => modified,
        Err(e) => return Ok(Some(e.to_string())),
    };
    let couldnt_write = |e: std::io::Error| format!("couldn't write {} ({e})", to.display());
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(to).map_err(couldnt_write)?;
    file.write_all(&bytes).map_err(couldnt_write)?;
    if let Some(modified) = modified {
        let _ = file.set_modified(modified);
    }
    file.sync_all().map_err(couldnt_write)?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A folder of its own, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("open3d-launcher-folders-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Everything in `dir` by its path inside it, with "/": a file with its bytes, a folder with none.
    fn snapshot(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
        fn walk(dir: &Path, prefix: &str, found: &mut BTreeMap<String, Option<Vec<u8>>>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries {
                let entry = entry.unwrap();
                let key = format!("{prefix}{}", entry.file_name().to_string_lossy());
                if entry.file_type().unwrap().is_dir() {
                    found.insert(key.clone(), None);
                    walk(&entry.path(), &format!("{key}/"), found);
                } else {
                    found.insert(key, Some(std::fs::read(entry.path()).unwrap()));
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(dir, "", &mut found);
        found
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// An old data folder as a launcher from before leaves it: what is kept and what is made again.
    fn old_data_folder(dir: &Path) {
        for (path, text) in [
            ("installed.json", "{\"apps\":{}}"),
            (".instance-token", "deadbeef"),
            ("self-update.txt", "v0.3.4\n1791500000\n0.3.4\n"),
            ("self-update-done.txt", "0.3.4"),
            ("logs/self-update-error.txt", "the MSI failed"),
            ("logs/open3d-launcher.log", "a log"),
            ("logs/open3d-launcher.1.log", "an older log"),
            ("releases.json", "{}"),
            ("feed.json", "{}"),
            ("feed-throttle.json", "{}"),
            ("launcher-catalog.json", "{}"),
            ("downloads/opentools-0.5.0-windows-x64-3dsmax.zip", "a download"),
            ("screenshots/rev/opentools/main.jpg", "a picture"),
            ("launcher-update-helper/open3d-launcher.exe", "a helper"),
        ] {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
    }

    #[test]
    fn only_what_holds_the_installs_is_copied_once_and_the_old_folder_is_left_as_it_was() {
        let scratch = Scratch::new("kept");
        let (old, new) = (scratch.0.join("Open3D Launcher"), scratch.0.join("Open3D").join("Launcher"));
        old_data_folder(&old);
        let before = snapshot(&old);
        assert_eq!(carry_kept(&old, &new, &KEPT), CarryOver::Copied { files: 5, left_out: vec![] });
        let copied = snapshot(&new);
        assert_eq!(
            copied.keys().map(String::as_str).collect::<Vec<_>>(),
            [".instance-token", "installed.json", "logs", "logs/self-update-error.txt", "self-update-done.txt", "self-update.txt"]
        );
        for (path, bytes) in &copied {
            assert_eq!(Some(bytes), before.get(path), "{path} is copied as it was");
        }
        assert_eq!(snapshot(&old), before, "the old folder is left as it was");
        assert_eq!(names(&scratch.0.join("Open3D")), ["Launcher"], "nothing is left beside the new folder");
        // From then on the new folder is used as it is, and the old one is not read.
        std::fs::write(new.join("installed.json"), "{\"apps\":{\"opentools\":{}}}").unwrap();
        assert_eq!(carry_kept(&old, &new, &KEPT), CarryOver::Nothing);
        assert_eq!(std::fs::read(new.join("installed.json")).unwrap(), b"{\"apps\":{\"opentools\":{}}}");
        assert_eq!(snapshot(&old), before);
        assert_eq!(native::in_use(Some(new.clone()), Some(old)), Some(new));
    }

    #[test]
    fn the_time_a_kept_file_was_changed_is_kept() {
        let scratch = Scratch::new("time");
        let (old, new) = (scratch.0.join("old"), scratch.0.join("new"));
        old_data_folder(&old);
        let then = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(old.join("installed.json")).unwrap().set_modified(then).unwrap();
        assert!(matches!(carry_kept(&old, &new, &KEPT), CarryOver::Copied { files: 5, .. }));
        assert_eq!(std::fs::metadata(new.join("installed.json")).unwrap().modified().unwrap(), then);
    }

    #[test]
    fn an_old_folder_with_nothing_to_keep_gives_an_empty_new_one_and_none_gives_nothing() {
        let scratch = Scratch::new("empty");
        let (old, new) = (scratch.0.join("old"), scratch.0.join("new"));
        std::fs::create_dir_all(old.join("downloads")).unwrap();
        std::fs::write(old.join("feed.json"), "{}").unwrap();
        assert_eq!(carry_kept(&old, &new, &KEPT), CarryOver::Copied { files: 0, left_out: vec![] });
        assert!(snapshot(&new).is_empty());
        let (missing, other) = (scratch.0.join("missing"), scratch.0.join("other"));
        assert_eq!(carry_kept(&missing, &other, &KEPT), CarryOver::Nothing);
        assert!(!other.exists());
    }

    #[test]
    fn a_copy_that_fails_leaves_nothing_and_the_old_folder_in_use() {
        let scratch = Scratch::new("fail");
        let old = scratch.0.join("Open3D Launcher");
        old_data_folder(&old);
        let before = snapshot(&old);
        // The folder the new one goes in is a file.
        let blocked = scratch.0.join("Open3D");
        std::fs::write(&blocked, b"a file").unwrap();
        let new = blocked.join("Launcher");
        let outcome = carry_kept(&old, &new, &KEPT);
        assert!(matches!(&outcome, CarryOver::Failed(reason) if !reason.is_empty()), "{outcome:?}");
        assert_eq!(snapshot(&old), before);
        assert_eq!(names(&scratch.0), ["Open3D", "Open3D Launcher"]);
        assert_eq!(native::in_use(Some(new.clone()), Some(old.clone())), Some(old.clone()));
        let problem = said(&old, &new, outcome);
        assert!(problem.is_some_and(|problem| !problem.is_empty()));
        // A new folder inside the old one would be copied into itself.
        assert!(matches!(carry_kept(&old, &old.join("deeper").join("Launcher"), &KEPT), CarryOver::Failed(_)));
        assert_eq!(snapshot(&old), before);
    }

    #[test]
    fn a_kept_name_that_is_not_a_plain_file_is_left_out_and_said() {
        let scratch = Scratch::new("folder");
        let (old, new) = (scratch.0.join("old"), scratch.0.join("new"));
        std::fs::create_dir_all(old.join("installed.json")).unwrap();
        std::fs::write(old.join(".instance-token"), "deadbeef").unwrap();
        let CarryOver::Copied { files: 1, left_out } = carry_kept(&old, &new, &KEPT) else { panic!("not copied") };
        assert_eq!(left_out.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(), [PathBuf::from("installed.json")]);
        let problem = said(&old, &new, CarryOver::Copied { files: 1, left_out }).unwrap();
        assert!(problem.contains("installed.json"), "{problem}");
        assert_eq!(names(&new), [".instance-token"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_kept_file_another_program_holds_is_left_out_and_the_rest_copied() {
        use std::os::windows::fs::OpenOptionsExt;
        let scratch = Scratch::new("held");
        let (old, new) = (scratch.0.join("old"), scratch.0.join("new"));
        old_data_folder(&old);
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(old.join("installed.json")).unwrap();
        let outcome = carry_kept(&old, &new, &KEPT);
        drop(held);
        let CarryOver::Copied { files: 4, left_out } = outcome else { panic!("{outcome:?}") };
        assert_eq!(left_out.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(), [PathBuf::from("installed.json")]);
        assert!(!new.join("installed.json").exists());
        assert_eq!(std::fs::read(old.join("installed.json")).unwrap(), b"{\"apps\":{}}");
    }

    /// The data folder the launcher carries into and uses is the one every Open3D app's
    /// `app_data_dir` gives.
    #[test]
    fn the_new_data_folder_is_the_shared_one() {
        let os = |text: &str| Some(std::ffi::OsString::from(text));
        for (variable, local) in
            [(os("D:\\Suite\\Data"), os("C:\\Local")), (None, os("C:\\Local")), (os(""), os("C:\\Local")), (None, os("")), (None, None)]
        {
            assert_eq!(
                native::open3d_data_dir_from(variable.clone(), local.clone()),
                open3d_settings::data_dir_from(variable.clone(), local.clone()),
                "{variable:?} {local:?}"
            );
        }
        assert_eq!(native::FOLDER, "Launcher");
    }
}
