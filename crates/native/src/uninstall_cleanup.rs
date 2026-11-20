// SPDX-License-Identifier: GPL-3.0-or-later
//! What uninstalling the launcher takes away besides its own files (D-045, D-158): its
//! start-at-login entry, when it is this launcher's, and its caches.
//!
//! The launcher's MSI (`packaging/windows/open3d-launcher.wxs`) runs the installed exe itself with
//! [`FLAG`] on a real uninstall only, not when an update removes the version before it, and before
//! its files go. The exe then does this and exits, with no window. Whatever fails here never stops
//! the uninstall. The settings (`app.ron`), the record of the apps it installed, its logs and the
//! token of its single instance stay; so do the apps.
//!
//! Open3D's own code, not part of ArtCraft Launcher.

use std::path::{Path, PathBuf};

/// The first argument the launcher's MSI starts the installed exe with on a real uninstall.
pub const FLAG: &str = "--uninstall-cleanup";

/// The caches in the launcher's cache folder (`paths::Paths::cache`): downloads, saved release
/// answers, the release feed and its throttle, the pictures of the details pages, the signed
/// catalog, and what its self-update leaves (the update helper, a portable update's staging).
const CACHES: [&str; 8] = [
    "downloads",
    "releases.json",
    "feed.json",
    "feed-throttle.json",
    "screenshots",
    "launcher-catalog.json",
    "launcher-update-helper",
    "launcher-update",
];

/// The catalog's app icons saved beside them: `catalog-icon-<sha256>.png`.
const ICON_PREFIX: &str = "catalog-icon-";

/// The start-at-login entry. Tests give their own.
pub(crate) trait StartAtLogin {
    /// The value as written (`"<exe>" --hidden`), when there is one.
    fn read(&self) -> Option<String>;
    fn remove(&self) -> Result<(), String>;
}

/// This account's: `Open3D Launcher` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
struct WindowsStartAtLogin;

impl StartAtLogin for WindowsStartAtLogin {
    fn read(&self) -> Option<String> {
        crate::login::entry()
    }

    fn remove(&self) -> Result<(), String> {
        crate::login::set_enabled(false)
    }
}

/// Does the cleanup for the launcher that runs: its own exe, this account's registry, and its cache
/// folder as the rest of the launcher finds it (`OPEN3D_DATA_DIR`, a portable launcher's folder,
/// or the old folder while it is the one in use). `Err` says what was left.
pub fn run() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find this program ({e})"))?;
    let cache = crate::paths::Paths::new().ok().map(|paths| paths.cache);
    let left = clean(&exe, &WindowsStartAtLogin, cache.as_deref());
    if left.is_empty() { Ok(()) } else { Err(left.join("; ")) }
}

/// Removes the start-at-login entry when it starts `exe`, and the caches in `cache`; says what it
/// couldn't remove. An entry that starts another copy of the launcher is that copy's, and stays.
pub(crate) fn clean(exe: &Path, login: &dyn StartAtLogin, cache: Option<&Path>) -> Vec<String> {
    let mut left = Vec::new();
    if login.read().is_some_and(|value| starts(&value, exe))
        && let Err(e) = login.remove()
    {
        left.push(e);
    }
    let Some(cache) = cache else { return left };
    let mut names: Vec<PathBuf> = CACHES.iter().map(|name| cache.join(name)).collect();
    if let Ok(entries) = std::fs::read_dir(cache) {
        names.extend(entries.flatten().map(|entry| entry.path()).filter(|path| is_icon(path)));
    }
    for path in names {
        if let Err(e) = remove(&path) {
            left.push(format!("{} ({e})", path.display()));
        }
    }
    left
}

/// Whether the start-at-login `value` starts the program `exe`: its first part, in quotes or not.
pub(crate) fn starts(value: &str, exe: &Path) -> bool {
    let value = value.trim();
    let program = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or_default(),
        None => value.split(' ').next().unwrap_or_default(),
    };
    !program.is_empty() && same_file(Path::new(program), exe)
}

/// Whether two paths name the same file, however each is spelled: Windows ignores case, and a
/// path may come in its short form.
fn same_file(left: &Path, right: &Path) -> bool {
    let plain = |p: &Path| {
        let full = dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        full.to_string_lossy().replace('/', "\\").to_lowercase()
    };
    plain(left) == plain(right)
}

/// `catalog-icon-<…>.png` in the cache folder.
fn is_icon(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(ICON_PREFIX) && name.ends_with(".png"))
}

/// Removes `path` and what is in it; a link is removed itself, never followed. Nothing there is
/// no error.
fn remove(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    /// A junction or a symbolic link.
    const REPARSE_POINT: u32 = 0x400;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.file_attributes() & REPARSE_POINT != 0 {
        return std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path));
    }
    if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A start-at-login entry held in memory.
    struct FakeLogin {
        value: RefCell<Option<String>>,
        fails: bool,
    }

    impl FakeLogin {
        fn with(value: Option<&str>) -> Self {
            Self { value: RefCell::new(value.map(str::to_string)), fails: false }
        }
    }

    impl StartAtLogin for FakeLogin {
        fn read(&self) -> Option<String> {
            self.value.borrow().clone()
        }
        fn remove(&self) -> Result<(), String> {
            if self.fails {
                return Err("couldn't change the startup list (access denied)".into());
            }
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }

    /// The launcher's exe and its cache folder, with every cache, and beside them what must stay.
    fn launcher(root: &Path) -> (PathBuf, PathBuf) {
        let exe = root.join("Programs").join("Open3D").join("Launcher").join("open3d-launcher.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"exe").unwrap();
        let cache = root.join("Local").join("Open3D").join("Launcher");
        for dir in ["downloads", "screenshots/r1/openref", "launcher-update-helper", "launcher-update", "logs"] {
            std::fs::create_dir_all(cache.join(dir)).unwrap();
        }
        for file in [
            "downloads/openref-0.2.0-windows-x64.msi",
            "downloads/openview-0.3.0-windows-x64.zip.12.part",
            "screenshots/r1/openref/board.jpg",
            "launcher-update-helper/open3d-launcher.exe",
            "releases.json",
            "feed.json",
            "feed-throttle.json",
            "launcher-catalog.json",
            "catalog-icon-0123abcd.png",
            // What stays: the record of the apps, the logs, the instance token, the self-update record.
            "installed.json",
            "logs/open3d-launcher.log",
            ".instance-token",
            "self-update-done.txt",
        ] {
            std::fs::write(cache.join(file), b"x").unwrap();
        }
        (exe, cache)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        found.sort();
        found
    }

    #[test]
    fn removes_the_caches_and_its_own_start_at_login_entry_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let (exe, cache) = launcher(tmp.path());
        let settings = tmp.path().join("Roaming").join("Open3D").join("Launcher");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(settings.join("app.ron"), b"()").unwrap();
        let login = FakeLogin::with(Some(&format!("\"{}\" --hidden", exe.display())));
        assert_eq!(clean(&exe, &login, Some(&cache)), Vec::<String>::new());
        assert_eq!(login.read(), None);
        assert_eq!(names(&cache), [".instance-token", "installed.json", "logs", "self-update-done.txt"]);
        assert_eq!(names(&cache.join("logs")), ["open3d-launcher.log"]);
        assert!(settings.join("app.ron").is_file(), "its settings stay");
        assert!(exe.is_file(), "the MSI removes the program itself");
        // Again: nothing left to remove is no error.
        assert_eq!(clean(&exe, &login, Some(&cache)), Vec::<String>::new());
        assert_eq!(clean(&exe, &login, None), Vec::<String>::new());
    }

    #[test]
    fn a_start_at_login_entry_of_another_copy_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let (exe, cache) = launcher(tmp.path());
        let portable = r#""D:\Suite\open3d-launcher.exe" --hidden"#;
        let login = FakeLogin::with(Some(portable));
        assert!(clean(&exe, &login, Some(&cache)).is_empty());
        assert_eq!(login.read().as_deref(), Some(portable));
        // One that can't be removed is said.
        let login = FakeLogin { value: RefCell::new(Some(format!("\"{}\" --hidden", exe.display()))), fails: true };
        assert_eq!(clean(&exe, &login, None), ["couldn't change the startup list (access denied)"]);
    }

    #[test]
    fn the_entry_is_read_as_the_launcher_writes_it_and_as_people_do() {
        let exe = Path::new(r"Q:\Local\Programs\Open3D\Launcher\open3d-launcher.exe");
        for value in [
            r#""Q:\Local\Programs\Open3D\Launcher\open3d-launcher.exe" --hidden"#,
            r#""q:\local\programs\open3d\launcher\OPEN3D-LAUNCHER.EXE""#,
            r"Q:\Local\Programs\Open3D\Launcher\open3d-launcher.exe --hidden",
            r#"  "Q:/Local/Programs/Open3D/Launcher/open3d-launcher.exe" --hidden  "#,
        ] {
            assert!(starts(value, exe), "{value}");
        }
        for value in [
            r#""Q:\Local\Programs\Open3D Launcher\open3d-launcher.exe" --hidden"#,
            r#""D:\Suite\open3d-launcher.exe" --hidden"#,
            r#""" --hidden"#,
            "",
        ] {
            assert!(!starts(value, exe), "{value}");
        }
    }

    #[test]
    fn a_cache_that_is_a_link_is_unlinked_and_what_it_points_at_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let (exe, cache) = launcher(tmp.path());
        let elsewhere = tmp.path().join("Elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("keep.txt"), b"keep").unwrap();
        std::fs::remove_dir_all(cache.join("screenshots")).unwrap();
        // A junction needs no administrator rights.
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(cache.join("screenshots")).arg(&elsewhere).output().unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stdout));
        assert!(clean(&exe, &FakeLogin::with(None), Some(&cache)).is_empty());
        assert!(std::fs::symlink_metadata(cache.join("screenshots")).is_err());
        assert!(elsewhere.join("keep.txt").is_file());
    }

    #[test]
    fn what_cannot_be_removed_is_said_and_the_rest_still_goes() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let (exe, cache) = launcher(tmp.path());
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(cache.join("feed.json")).unwrap();
        let left = clean(&exe, &FakeLogin::with(None), Some(&cache));
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].contains("feed.json"), "{left:?}");
        assert!(!cache.join("downloads").exists() && !cache.join("releases.json").exists());
        drop(held);
    }
}
