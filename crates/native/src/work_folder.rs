// SPDX-License-Identifier: GPL-3.0-or-later
//! The work folder an app is installed, updated and removed through: `.open3d-<id>` beside the
//! app's own folder, for 3ds Max bundles ([`crate::max_bundle`]) and for programs of their own
//! ([`crate::standalone`]) alike.
//!
//! - `new\...`: the new copy, while it is unpacked and checked.
//! - `backup\<name>`: the installed copy, from the moment an update moves it aside until the new
//!   one is in place. Found with no copy installed, it is put back.
//! - `old-<n>`: a copy that only waits to be deleted.
//!
//! Windows refuses to delete a program file (an exe, a library, a compiled plug-in) that a running
//! process has loaded, though it lets the file and the folders above it be renamed. So an update or
//! a removal is done once the old copy is out of the way, and what can't be deleted yet waits here
//! for a later [`recover`]: see [`OldCopy`].
//!
//! Open3D's own code, not part of ArtCraft Launcher.

use std::path::{Path, PathBuf};

/// What became of the copy an update or a removal took out of the way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OldCopy {
    /// Deleted, or there was none.
    Gone,
    /// Out of the way, but a running process still has a file of it loaded, so Windows won't delete
    /// it yet. It waits in the work folder for a later [`recover`].
    InUse,
}

/// Finishes what an earlier install or removal of the app at `target` left in its work folder
/// `work`: puts back the old copy of an update that was interrupted, then deletes everything else
/// as far as it can be deleted.
///
/// `backup\<name>` is put back only while no copy is installed. Beside an installed copy it is the
/// old copy of an update that finished and couldn't be deleted: that one is renamed to `old-<n>`
/// first, so that it is never taken for a copy to put back once the installed one is removed, and
/// never in the way of the next update's backup.
///
/// A file a running process still has loaded is no error: it stays for the next call
/// ([`OldCopy::InUse`]). An error means the app itself couldn't be put right.
pub(crate) fn recover(app_name: &str, work: &Path, target: &Path) -> Result<OldCopy, String> {
    if !work.exists() {
        return Ok(OldCopy::Gone);
    }
    let Some(name) = target.file_name() else { return Err(format!("{} has no name", target.display())) };
    let backup_parent = work.join("backup");
    let backup = backup_parent.join(name);
    if backup.is_dir() && target.exists() {
        // Nothing of it is deleted until it has its new name: a copy that still looks like one to
        // put back must stay whole.
        std::fs::rename(&backup_parent, spare_name(work)).map_err(|e| format!("Couldn't clean {}; is a file in use? ({e})", backup.display()))?;
    } else if backup.is_dir() {
        std::fs::rename(&backup, target).map_err(|e| format!("Couldn't restore {} ({e})", target.display()))?;
    }
    Ok(sweep(app_name, work))
}

/// A free name in the work folder for a copy that only waits to be deleted.
pub(crate) fn spare_name(work: &Path) -> PathBuf {
    (0u32..).map(|n| work.join(format!("old-{n}"))).find(|p| std::fs::symlink_metadata(p).is_err()).unwrap_or_else(|| work.join("old"))
}

/// Deletes the work folder as far as it can be deleted, and says whether anything of it stays.
pub(crate) fn sweep(app_name: &str, work: &Path) -> OldCopy {
    if remove_what_can_go(work) {
        return OldCopy::Gone;
    }
    log::info!("{app_name}: files of an earlier copy are still in use; {} stays until they are free", work.display());
    OldCopy::InUse
}

/// Deletes `dir` and everything in it that can be deleted; true when nothing of it is left. A file
/// a running process has loaded stays, and so do the folders above it. The rest goes, so that what
/// waits is as little as it can be.
fn remove_what_can_go(dir: &Path) -> bool {
    if std::fs::remove_dir_all(dir).is_ok() {
        return true;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // A link is removed itself, never followed: it is no directory to `file_type`.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                remove_what_can_go(&path);
            } else if std::fs::remove_file(&path).is_err() {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
    std::fs::remove_dir(dir).is_ok() || std::fs::symlink_metadata(dir).is_err()
}

/// What the tests of both kinds of app share.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};

    /// A program file held the way a running process holds one it loaded: mapped as a program
    /// image, with no file handle left open. Windows then lets the file and the folders above it be
    /// renamed but not deleted, until the mapping goes. That is what was measured with a compiled
    /// plug-in loaded in `3dsmaxbatch.exe`. A file that is merely open behaves differently: opened
    /// without `FILE_SHARE_DELETE` its folder can't be renamed either, opened with it the file can
    /// be deleted, and so can a file mapped as plain data.
    pub(crate) struct LoadedLibrary {
        _view: winsafe::guard::UnmapViewOfFileGuard,
        _image: winsafe::guard::CloseHandleGuard<winsafe::HFILEMAP>,
    }

    /// Writes a library at `file` and loads it. Any real library will do: none of its code runs.
    pub(crate) fn load_library(file: &Path) -> LoadedLibrary {
        use winsafe::{HFILE, co};
        let library = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot is set")).join("System32").join("version.dll");
        std::fs::copy(library, file).unwrap();
        let share = co::FILE_SHARE::READ | co::FILE_SHARE::DELETE;
        let (handle, _) = HFILE::CreateFile(
            file.to_str().unwrap(),
            co::GENERIC::READ,
            Some(share),
            None,
            co::DISPOSITION::OPEN_EXISTING,
            co::FILE_ATTRIBUTE::NORMAL,
            None,
            None,
            None,
        )
        .unwrap();
        let image = handle.CreateFileMapping(None, co::PAGE::READONLY, Some(co::SEC::IMAGE), None, None).unwrap();
        let view = image.MapViewOfFile(co::FILE_MAP::READ, 0, None).unwrap();
        drop(handle);
        assert!(std::fs::remove_file(file).is_err(), "a loaded library must refuse to be deleted, or these tests test nothing");
        LoadedLibrary { _view: view, _image: image }
    }

    /// The names of the files in `dir` and in every folder under it, sorted.
    pub(crate) fn files_under(dir: &Path) -> Vec<String> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.file_type().unwrap().is_dir() {
                found.extend(files_under(&entry.path()));
            } else {
                found.push(entry.file_name().into_string().unwrap());
            }
        }
        found.sort();
        found
    }
}
