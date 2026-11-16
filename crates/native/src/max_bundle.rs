// SPDX-License-Identifier: GPL-3.0-or-later
//! Open3D apps are 3ds Max plug-in bundles: installing, updating, finding and removing them.
//!
//! A release asset (`<app>-<version>-windows-x64-3dsmax.zip`) holds the bundle folder at its root
//! (`OpenTools.bundle/PackageContents.xml`, ...). It goes in `%APPDATA%\Autodesk\ApplicationPlugins`,
//! the per-user folder every 3ds Max from 2025 on reads, so no administrator rights are needed.
//!
//! Work happens in `.open3d-<id>` beside the bundles: 3ds Max only looks for `PackageContents.xml`
//! one level down, so the new copy (`new\<bundle>`) and the old one (`backup\<bundle>`) are never
//! loaded, even if the launcher stops halfway; [`recover`] puts a backup back before the next
//! install or removal.
//!
//! A bundle may hold compiled plug-ins, and Windows refuses to delete one a running process has
//! loaded, though it lets the file and its folders be renamed. So the old copy of an update or a
//! removal is first moved out of 3ds Max's way, which is what makes the job done, and then deleted
//! as far as it can be: a copy still in use waits in `old-<n>` for a later [`recover`]. See
//! [`crate::work_folder`], [`OldCopy`] and [`max_running`].
//!
//! Open3D's own code (from Open3D Launcher 0.1), not part of ArtCraft Launcher.

use std::path::{Path, PathBuf};

use open3d_launcher_core::{InstallSource, Installed, SuiteApp, version};

use crate::manifest::Record;
use crate::work_folder::{self, OldCopy};

/// `%APPDATA%\Autodesk\ApplicationPlugins`.
pub fn plugins_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Autodesk").join("ApplicationPlugins"))
}

/// The bundle folder's name: `<App name>.bundle` (`OpenTools.bundle`).
pub fn bundle_name(app: &SuiteApp) -> String {
    format!("{}.bundle", app.name)
}

/// Where this app's bundle lives.
pub fn bundle_dir(app: &SuiteApp) -> Option<PathBuf> {
    plugins_dir().map(|p| p.join(bundle_name(app)))
}

/// Value of an XML attribute in the first tag that has it (enough for `PackageContents.xml`).
pub fn xml_attr(text: &str, name: &str) -> Option<String> {
    let mut rest = text;
    while let Some(i) = rest.find(name) {
        let before_ok = i == 0 || rest[..i].ends_with(|c: char| c.is_whitespace());
        let after = rest[i + name.len()..].trim_start();
        if before_ok && let Some(eq) = after.strip_prefix('=') {
            let eq = eq.trim_start();
            if let Some(q) = eq.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let body = &eq[1..];
                return body.find(q).map(|end| body[..end].to_string());
            }
        }
        rest = &rest[i + name.len()..];
    }
    None
}

/// `AppVersion` of a bundle folder.
pub fn bundle_version(dir: &Path) -> Option<String> {
    xml_attr(&std::fs::read_to_string(dir.join("PackageContents.xml")).ok()?, "AppVersion")
}

/// The installed bundle, if there is one. A bundle the launcher didn't install (copied by hand) is
/// found too: it can be updated and removed the same way.
pub fn detect(app: &SuiteApp) -> Option<Installed> {
    let dir = bundle_dir(app)?;
    if !dir.join("PackageContents.xml").is_file() {
        return None;
    }
    Some(Installed {
        version: bundle_version(&dir).as_deref().and_then(version::parse),
        source: InstallSource::Bundle,
        location: dir.to_string_lossy().into_owned(),
        package: None,
        externally_managed: false,
        origin: None,
        installation: None,
    })
}

/// Other bundles of the same app in the plug-in folder (`OpenTools-dev.bundle`): 3ds Max loads only
/// the first one it finds.
pub fn other_copies(app: &SuiteApp) -> Vec<String> {
    plugins_dir().map(|plugins| other_copies_in(app, &plugins)).unwrap_or_default()
}

/// [other_copies] in the plug-in folder `plugins`, sorted by name.
pub(crate) fn other_copies_in(app: &SuiteApp, plugins: &Path) -> Vec<String> {
    let stem = app.name.to_ascii_lowercase();
    let own = bundle_name(app).to_ascii_lowercase();
    let Ok(entries) = std::fs::read_dir(plugins) else { return Vec::new() };
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l != own && l.starts_with(&stem) && l.ends_with(".bundle")
        })
        .collect();
    found.sort();
    found
}

/// True while a 3ds Max is running (`3dsmax.exe`): bundles are never changed under it. It caches
/// what it loaded and goes on loading scripts from the bundle's folder, so a swap would leave an
/// open session half old and half new, and whoever works in it can close it and try again.
///
/// That includes a 3ds Max without a window. `3dsmaxbatch.exe`, which render jobs and render farms
/// keep running, is a front end: it starts a `3dsmax.exe -batch` and that child is the process
/// that loads the plug-ins (measured with 3ds Max 2027 and a compiled plug-in). So a batch or farm
/// render is waited for like any other 3ds Max, and a machine that renders all day is only updated
/// between renders. `3dsmaxcmd.exe` is a front end too: it links nothing of 3ds Max but its network
/// library, and what it renders is rendered by a `3dsmax.exe`.
///
/// A file of the old copy can still be loaded when the swap happens (a 3ds Max started in between,
/// any other program that has it open as a library): the swap is safe then, see [`OldCopy::InUse`].
pub fn max_running() -> bool {
    // Development builds only: test installs while 3ds Max is open (the release build ignores it).
    if cfg!(debug_assertions) && std::env::var_os("OPEN3D_LAUNCHER_IGNORE_MAX").is_some() {
        return false;
    }
    image_running("3dsmax.exe")
}

/// True while `3dsmaxbatch.exe` is running. Its `3dsmax.exe` is what [`max_running`] sees and
/// waits for; this look only adds a sentence to a finished job's message in the rare case that
/// the front end is up without one.
pub fn max_batch_running() -> bool {
    image_running("3dsmaxbatch.exe")
}

/// Whether a process started from a program file called `image` is running.
fn image_running(image: &str) -> bool {
    #[cfg(windows)]
    {
        let out = crate::process::command("tasklist").args(["/FI", &format!("IMAGENAME eq {image}"), "/NH", "/FO", "CSV"]).output();
        out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&format!("\"{image}\"")))
    }
}

/// The error for anything that must wait until 3ds Max is closed.
pub fn max_running_error() -> String {
    "3ds Max is running. Close it, then try again.".into()
}

/// What a finished install, update or removal adds to its message: said when a file of the old
/// copy is still loaded (`old`), or when a 3ds Max nobody waited for is running. Either way those
/// processes go on with what they loaded, which is easy to miss on a machine that renders.
pub fn still_running_note(old: OldCopy) -> Option<&'static str> {
    still_running_note_when(old, max_batch_running())
}

fn still_running_note_when(old: OldCopy, batch_running: bool) -> Option<&'static str> {
    (old == OldCopy::InUse || batch_running)
        .then_some("3ds Max processes still running in the background (batch or farm renders) keep what they had loaded until they restart.")
}

fn work_dir(plugins: &Path, app: &SuiteApp) -> PathBuf {
    plugins.join(format!(".open3d-{}", app.id))
}

/// Finishes what an earlier install or removal left in the work folder: puts back the old copy of
/// an update that was interrupted, never the one a finished update left beside the installed
/// bundle, then deletes everything else as far as it can be deleted ([`work_folder::recover`]).
pub fn recover(app: &SuiteApp, plugins: &Path) -> Result<OldCopy, String> {
    work_folder::recover(app.name, &work_dir(plugins, app), &plugins.join(bundle_name(app)))
}

/// Installs or updates `app` from the downloaded `zip`; the package must say it is `version`.
/// Also says whether the copy it replaced is still in use.
pub fn install(app: &SuiteApp, zip: &Path, version: &str) -> Result<(Record, OldCopy), String> {
    if max_running() {
        return Err(max_running_error());
    }
    install_into(app, zip, version, &plugins_dir().ok_or("APPDATA is not set")?)
}

/// [install] into plugins, without the 3ds Max check (the caller has done it).
pub(crate) fn install_into(app: &SuiteApp, zip: &Path, version: &str, plugins: &Path) -> Result<(Record, OldCopy), String> {
    std::fs::create_dir_all(plugins).map_err(|e| format!("Couldn't create {} ({e})", plugins.display()))?;
    recover(app, plugins)?;
    let work = work_dir(plugins, app);
    let fresh = work.join("new");
    // What an earlier try unpacked must be gone, or two packages would be mixed.
    if fresh.exists() {
        return Err(format!("Couldn't clean {}; is a file in use?", fresh.display()));
    }
    std::fs::create_dir_all(&fresh).map_err(|e| format!("Couldn't create {} ({e})", fresh.display()))?;
    let target = plugins.join(bundle_name(app));
    let result = swap_in(app, zip, version, &work, &fresh, &target);
    // Puts the old copy back if the swap failed halfway, then deletes what is left in the work
    // folder. If even putting it back fails the work folder stays for the next try: the old copy
    // is never deleted unplaced.
    let cleanup = recover(app, plugins);
    result?;
    // The new version is in place, so the install worked whatever became of the old copy.
    let old = cleanup.unwrap_or_else(|e| {
        log::warn!("{}: {e}", app.name);
        OldCopy::InUse
    });
    let record = Record {
        version: version.to_string(),
        kind: open3d_launcher_core::PackageKind::MaxBundle,
        path: target,
        installed_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64),
        files: Vec::new(),
        needs_root: false,
    };
    Ok((record, old))
}

fn swap_in(app: &SuiteApp, zip: &Path, version: &str, work: &Path, fresh: &Path, target: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("Couldn't open the package ({e})"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("The package isn't a valid zip ({e})"))?;
    archive.extract(fresh).map_err(|e| format!("Couldn't unpack the package ({e})"))?;
    let new_bundle = fresh.join(bundle_name(app));
    let Some(found) = bundle_version(&new_bundle) else {
        return Err(format!("The package doesn't contain {}/PackageContents.xml", bundle_name(app)));
    };
    if version::parse(&found) != version::parse(version) {
        return Err(format!("The package is version {found}, the release says {version}"));
    }
    let backup_parent = work.join("backup");
    let backup = backup_parent.join(bundle_name(app));
    let had_old = target.exists();
    if had_old {
        std::fs::create_dir_all(&backup_parent).map_err(|e| format!("Couldn't prepare the update ({e})"))?;
        std::fs::rename(target, &backup).map_err(|e| format!("Couldn't move the installed version aside; is a file in use? ({e})"))?;
    }
    if let Err(e) = std::fs::rename(&new_bundle, target) {
        if had_old {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(format!("Couldn't put the new version in place ({e})"));
    }
    Ok(())
}

/// Removes the bundle folder, and says whether a file of it is still in use. The app's settings and
/// data live elsewhere and are kept.
pub fn uninstall(app: &SuiteApp, dir: &Path) -> Result<OldCopy, String> {
    if max_running() {
        return Err(max_running_error());
    }
    remove(app, dir)
}

/// [uninstall] without the 3ds Max check.
fn remove(app: &SuiteApp, dir: &Path) -> Result<OldCopy, String> {
    let plugins = dir.parent().ok_or("the bundle has no parent folder")?;
    if dir.file_name().and_then(|n| n.to_str()) != Some(bundle_name(app).as_str()) {
        return Err(format!("{} isn't {}'s bundle, so it was left alone", dir.display(), app.name));
    }
    let old = recover(app, plugins)?;
    if !dir.exists() {
        return Ok(old);
    }
    // Moved out of the way first, so a failure halfway never leaves a broken bundle Max would load.
    // Once it is out of the way the removal has worked, whatever of it can't be deleted yet.
    let work = work_dir(plugins, app);
    std::fs::create_dir_all(&work).map_err(|e| format!("Couldn't prepare the removal ({e})"))?;
    std::fs::rename(dir, work_folder::spare_name(&work)).map_err(|e| format!("Couldn't remove {}; is a file in use? ({e})", dir.display()))?;
    Ok(work_folder::sweep(app.name, &work))
}

/// The newest 3ds Max installed (`...\3dsmax.exe`), from its registry entries.
#[cfg(windows)]
pub fn newest_max() -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let root = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Autodesk\3dsMax").ok()?;
    let mut best: Option<(u32, PathBuf)> = None;
    for name in root.enum_keys().flatten() {
        let Some(major) = name.split('.').next().and_then(|m| m.parse::<u32>().ok()) else { continue };
        let Ok(key) = root.open_subkey(&name) else { continue };
        let Ok(dir) = key.get_value::<String, _>("Installdir") else { continue };
        let exe = PathBuf::from(dir).join("3dsmax.exe");
        if exe.is_file() && best.as_ref().is_none_or(|(m, _)| major > *m) {
            best = Some((major, exe));
        }
    }
    best.map(|(_, exe)| exe)
}

/// "Open" for a plug-in: start the newest 3ds Max, which loads the bundle.
pub fn launch(app: &SuiteApp) -> Result<(), String> {
    #[cfg(windows)]
    if let Some(max) = newest_max() {
        return crate::process::launch_binary(&max, &[]);
    }
    Err(format!("{} runs inside 3ds Max. Open 3ds Max to use it.", app.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_folder::testing::{LoadedLibrary, files_under, load_library};
    use std::io::Write;

    fn app() -> SuiteApp {
        *open3d_launcher_core::SuiteApp::by_id("opentools").expect("OpenTools is in the catalog")
    }

    fn package(dir: &Path, bundle: &str, version: &str, marker: &str) -> PathBuf {
        let path = dir.join(format!("{bundle}-{version}-{marker}.zip"));
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file(format!("{bundle}/PackageContents.xml"), o).unwrap();
        write!(z, r#"<ApplicationPackage SchemaVersion="1.0" AppVersion="{version}"><Components/></ApplicationPackage>"#).unwrap();
        z.start_file(format!("{bundle}/Contents/{marker}.txt"), o).unwrap();
        z.write_all(marker.as_bytes()).unwrap();
        z.finish().unwrap();
        path
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        v.sort();
        v
    }

    #[test]
    fn reads_app_version_not_component_version() {
        let xml = r#"<ApplicationPackage Name="x" AppVersion="0.5.1"><ComponentEntry Version="9.9.9"/></ApplicationPackage>"#;
        assert_eq!(xml_attr(xml, "AppVersion").as_deref(), Some("0.5.1"));
        assert_eq!(xml_attr("<a AppVersion = '1.2' />", "AppVersion").as_deref(), Some("1.2"));
    }

    #[test]
    fn installs_then_updates_without_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &plugins).unwrap();
        let (record, old) = install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.1", "new"), "0.5.1", &plugins).unwrap();
        assert_eq!(old, OldCopy::Gone);
        assert_eq!(record.path, plugins.join("OpenTools.bundle"));
        assert_eq!(bundle_version(&record.path).as_deref(), Some("0.5.1"));
        assert!(record.path.join("Contents/new.txt").exists());
        assert!(!record.path.join("Contents/old.txt").exists(), "old files must not survive an update");
        assert_eq!(names(&plugins), ["OpenTools.bundle"]);
    }

    #[test]
    fn a_bad_package_keeps_the_installed_version() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &plugins).unwrap();
        let wrong_folder = package(tmp.path(), "Other.bundle", "0.5.1", "x");
        assert!(install_into(&a, &wrong_folder, "0.5.1", &plugins).unwrap_err().contains("doesn't contain"));
        let wrong_version = package(tmp.path(), "OpenTools.bundle", "0.6.0", "y");
        assert!(install_into(&a, &wrong_version, "0.5.1", &plugins).unwrap_err().contains("release says"));
        assert_eq!(bundle_version(&plugins.join("OpenTools.bundle")).as_deref(), Some("0.5.0"));
        assert_eq!(names(&plugins), ["OpenTools.bundle"]);
    }

    #[test]
    fn recovers_from_an_interrupted_update() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &plugins).unwrap();
        let backup = plugins.join(".open3d-opentools").join("backup");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::rename(plugins.join("OpenTools.bundle"), backup.join("OpenTools.bundle")).unwrap();
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::Gone));
        assert_eq!(bundle_version(&plugins.join("OpenTools.bundle")).as_deref(), Some("0.5.0"));
        assert_eq!(names(&plugins), ["OpenTools.bundle"]);
    }

    #[test]
    fn finds_other_copies_of_the_app_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path();
        let a = app();
        assert!(other_copies_in(&a, &plugins.join("missing")).is_empty());
        for dir in ["OpenTools.bundle", "opentools-dev.bundle", "OpenTools-old.bundle", "OpenManager.bundle", "VRay3dsMax2027", ".open3d-opentools"] {
            std::fs::create_dir_all(plugins.join(dir)).unwrap();
        }
        // A file with a bundle's name is not a bundle.
        std::fs::write(plugins.join("OpenTools-copy.bundle"), b"").unwrap();
        assert_eq!(other_copies_in(&a, plugins), ["OpenTools-old.bundle", "opentools-dev.bundle"]);
        // Found whether or not the app itself is installed: installing it would make two copies.
        std::fs::remove_dir_all(plugins.join("OpenTools.bundle")).unwrap();
        assert_eq!(other_copies_in(&a, plugins), ["OpenTools-old.bundle", "opentools-dev.bundle"]);
    }

    #[test]
    fn uninstall_removes_only_the_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.1", "x"), "0.5.1", &plugins).unwrap();
        std::fs::create_dir_all(plugins.join("VRay3dsMax2027")).unwrap();
        assert_eq!(remove(&a, &plugins.join("OpenTools.bundle")), Ok(OldCopy::Gone));
        assert_eq!(names(&plugins), ["VRay3dsMax2027"]);
        assert!(remove(&a, &plugins.join("VRay3dsMax2027")).unwrap_err().contains("left alone"));
    }

    /// Adds a compiled plug-in to the bundle in `bundle` and loads it, the way a running 3ds Max
    /// holds one: it can then be renamed with its folders, but not deleted.
    fn load_plugin(bundle: &Path) -> LoadedLibrary {
        load_library(&bundle.join("Contents").join("plugin.dlo"))
    }

    #[test]
    fn updates_while_a_running_process_has_the_old_plugin_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &plugins).unwrap();
        let target = plugins.join("OpenTools.bundle");
        let loaded = load_plugin(&target);
        // The new version is in place, so the update worked, though the old copy can't be deleted yet.
        let (record, old) = install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.1", "new"), "0.5.1", &plugins).unwrap();
        assert_eq!((record.version.as_str(), old), ("0.5.1", OldCopy::InUse));
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.1"));
        assert!(target.join("Contents/new.txt").exists());
        assert!(!target.join("Contents/plugin.dlo").exists(), "old files must not survive an update");
        // Only the loaded file stays, where 3ds Max doesn't look and not as a backup to put back.
        let work = plugins.join(".open3d-opentools");
        assert_eq!(files_under(&work), ["plugin.dlo"]);
        assert!(!work.join("backup").exists());
        // It doesn't hold up the next update either.
        let (_, old) = install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.2", "newer"), "0.5.2", &plugins).unwrap();
        assert_eq!(old, OldCopy::InUse);
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.2"));
        assert_eq!(files_under(&work), ["plugin.dlo"]);
        // Once that process has ended the leftovers go, and the installed version stays.
        drop(loaded);
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::Gone));
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.2"));
        assert_eq!(names(&plugins), ["OpenTools.bundle"]);
    }

    #[test]
    fn uninstalls_while_a_running_process_has_the_plugin_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &plugins).unwrap();
        let target = plugins.join("OpenTools.bundle");
        let loaded = load_plugin(&target);
        // The bundle is gone from where 3ds Max looks, so the removal worked.
        assert_eq!(remove(&a, &target), Ok(OldCopy::InUse));
        assert_eq!(names(&plugins), [".open3d-opentools"]);
        assert_eq!(files_under(&plugins.join(".open3d-opentools")), ["plugin.dlo"]);
        // Still loaded: a later look can't delete it either, and doesn't take it for a copy to put back.
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::InUse));
        assert!(!target.exists(), "a removed bundle came back");
        // Installing again and removing that copy both work meanwhile.
        let (_, old) = install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.1", "new"), "0.5.1", &plugins).unwrap();
        assert_eq!(old, OldCopy::InUse);
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.1"));
        assert_eq!(remove(&a, &target), Ok(OldCopy::InUse));
        assert!(!target.exists());
        // Once that process has ended nothing is left.
        drop(loaded);
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::Gone));
        assert!(names(&plugins).is_empty());
    }

    #[test]
    fn the_old_copy_of_a_finished_update_is_never_put_back() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        // What a launcher from before this was handled left behind: the new version in place and
        // the old one, still loaded, where an interrupted update keeps the copy to put back.
        let elsewhere = tmp.path().join("elsewhere");
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &elsewhere).unwrap();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.1", "new"), "0.5.1", &plugins).unwrap();
        let target = plugins.join("OpenTools.bundle");
        let backup = plugins.join(".open3d-opentools").join("backup");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::rename(elsewhere.join("OpenTools.bundle"), backup.join("OpenTools.bundle")).unwrap();
        let loaded = load_plugin(&backup.join("OpenTools.bundle"));
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::InUse));
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.1"));
        assert!(!backup.exists(), "the old copy still looks like one to put back");
        // Removing the app doesn't bring the old copy back, now or on a later look.
        assert_eq!(remove(&a, &target), Ok(OldCopy::InUse));
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::InUse));
        assert!(!target.exists(), "the old copy of a finished update was put back");
        drop(loaded);
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::Gone));
        assert!(names(&plugins).is_empty());
    }

    /// A file of the old copy that a program holds open without sharing it for deletion refuses the
    /// rename of its folders as well as its own deletion: that copy can't be set aside, so nothing
    /// goes on over it, and the installed version is left as it is.
    #[test]
    fn an_old_copy_that_cannot_be_set_aside_stops_the_next_change() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let a = app();
        let elsewhere = tmp.path().join("elsewhere");
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.0", "old"), "0.5.0", &elsewhere).unwrap();
        install_into(&a, &package(tmp.path(), "OpenTools.bundle", "0.5.1", "new"), "0.5.1", &plugins).unwrap();
        let target = plugins.join("OpenTools.bundle");
        let backup = plugins.join(".open3d-opentools").join("backup");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::rename(elsewhere.join("OpenTools.bundle"), backup.join("OpenTools.bundle")).unwrap();
        let held_file = backup.join("OpenTools.bundle").join("PackageContents.xml");
        let held = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(held_file).unwrap();
        assert!(recover(&a, &plugins).unwrap_err().contains("in use"));
        assert!(remove(&a, &target).unwrap_err().contains("in use"));
        let newer = package(tmp.path(), "OpenTools.bundle", "0.5.2", "newer");
        assert!(install_into(&a, &newer, "0.5.2", &plugins).unwrap_err().contains("in use"));
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.1"));
        // While it still looks like a copy to put back, nothing of it is deleted.
        assert_eq!(files_under(&backup), ["PackageContents.xml", "old.txt"]);
        drop(held);
        assert_eq!(recover(&a, &plugins), Ok(OldCopy::Gone));
        assert_eq!(bundle_version(&target).as_deref(), Some("0.5.1"));
        assert_eq!(names(&plugins), ["OpenTools.bundle"]);
    }

    /// Both 3ds Max programs are looked for by the name of their file, in the list Windows gives of
    /// what is running: this test program is in that list, and a made-up one is not.
    #[test]
    fn finds_a_running_program_by_the_name_of_its_file() {
        let own = std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap().to_ascii_lowercase();
        assert!(image_running(&own), "{own} runs this test");
        assert!(!image_running("open3d-launcher-no-such-program.exe"));
    }

    /// The user hears about processes still running when a file of the old copy is loaded, or when
    /// a 3ds Max without a window is running: neither is waited for. The Spanish catalog spells
    /// this sentence out after each message it can follow.
    #[test]
    fn says_when_running_processes_keep_what_they_loaded() {
        let note = "3ds Max processes still running in the background (batch or farm renders) keep what they had loaded until they restart.";
        assert_eq!(still_running_note_when(OldCopy::Gone, false), None);
        assert_eq!(still_running_note_when(OldCopy::InUse, false), Some(note));
        assert_eq!(still_running_note_when(OldCopy::Gone, true), Some(note));
    }
}
