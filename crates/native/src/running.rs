// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a program of the suite is open, from the processes that run (D-157).
//!
//! A program is never updated or removed while it runs: Windows cannot replace the files of a
//! running program. Opening its exe for writing tells that only where the account may write the
//! file: in `Program Files` it fails with "access denied" whether the program runs or not. So the
//! launcher lists the processes (a ToolHelp snapshot) and reads each one's image path
//! (`QueryFullProcessImageNameW`). A process whose path it cannot read, such as another account's,
//! counts by its image name.
//!
//! Open3D's own code, not part of ArtCraft Launcher.

use std::path::{Path, PathBuf};

use open3d_launcher_core::SuiteApp;

/// A running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Process {
    /// Its image name (`OpenRef.exe`).
    pub name: String,
    /// Its exe, when this account may read it.
    pub path: Option<PathBuf>,
}

/// The processes that run now. Tests give their own list.
pub(crate) trait Processes {
    fn list(&self) -> Vec<Process>;
}

/// This computer's processes.
pub(crate) struct SystemProcesses;

impl Processes for SystemProcesses {
    fn list(&self) -> Vec<Process> {
        use winsafe::{HPROCESS, HPROCESSLIST, co};
        let mut snapshot = match HPROCESSLIST::CreateToolhelp32Snapshot(co::TH32CS::SNAPPROCESS, None) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::warn!("couldn't list the running programs ({e})");
                return Vec::new();
            }
        };
        let mut found = Vec::new();
        for entry in snapshot.iter_processes() {
            let Ok(entry) = entry else { break };
            let path = HPROCESS::OpenProcess(co::PROCESS::QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID)
                .ok()
                .and_then(|process| process.QueryFullProcessImageName(co::PROCESS_NAME::WIN32).ok())
                .filter(|path| !path.is_empty())
                .map(PathBuf::from);
            found.push(Process { name: entry.szExeFile(), path });
        }
        found
    }
}

/// Whether one of `processes` runs one of `exes`, the copies of a program the launcher looks after.
/// A process whose path is known counts only when it is one of them: the same program started from
/// somewhere else (a development build) is not this copy. One whose path is unknown counts when its
/// image name is theirs.
pub(crate) fn is_open(exes: &[PathBuf], processes: &[Process]) -> bool {
    processes.iter().any(|process| match &process.path {
        Some(path) => exes.iter().any(|exe| same_path(path, exe)),
        None => exes.iter().filter_map(|exe| exe.file_name()).any(|name| name.to_string_lossy().eq_ignore_ascii_case(&process.name)),
    })
}

/// What a job that would change an open program says (D-157).
pub(crate) fn open_error(app: &SuiteApp) -> String {
    format!("{} is open; close it and try again.", app.name)
}

/// Whether two paths name the same file: Windows ignores case and the direction of the slashes, and
/// reads `\\?\C:\…` as `C:\…`.
fn same_path(left: &Path, right: &Path) -> bool {
    let plain = |p: &Path| {
        let text = p.to_string_lossy().replace('/', "\\").to_lowercase();
        text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text).trim_end_matches('\\').to_string()
    };
    plain(left) == plain(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, path: Option<&str>) -> Process {
        Process { name: name.into(), path: path.map(PathBuf::from) }
    }

    #[test]
    fn a_program_is_open_when_one_of_its_copies_runs() {
        let exes = [PathBuf::from(r"C:\Program Files\Open3D\OpenRef\OpenRef.exe"), PathBuf::from(r"C:\Local\Programs\Open3D\OpenRef\OpenRef.exe")];
        let others = [process("explorer.exe", Some(r"C:\Windows\explorer.exe")), process("System", None)];
        assert!(!is_open(&exes, &others));
        for running in [
            r"C:\Program Files\Open3D\OpenRef\OpenRef.exe",
            r"c:\program files\open3d\openref\OPENREF.EXE",
            r"\\?\C:\Local\Programs\Open3D\OpenRef\OpenRef.exe",
        ] {
            let mut list = others.to_vec();
            list.push(process("OpenRef.exe", Some(running)));
            assert!(is_open(&exes, &list), "{running}");
        }
        // The same program started from elsewhere is another copy.
        assert!(!is_open(&exes, &[process("OpenRef.exe", Some(r"D:\dev\target\release\OpenRef.exe"))]));
        // Another account's copy, whose path can't be read, counts by its name.
        assert!(is_open(&exes, &[process("openref.exe", None)]));
        assert!(!is_open(&exes, &[process("OpenView.exe", None)]));
        assert!(!is_open(&[], &[process("OpenRef.exe", None)]));
    }

    /// The real list: this test's own process is in it, with its path.
    #[test]
    fn lists_this_process_with_its_path() {
        let me = std::env::current_exe().unwrap();
        let list = SystemProcesses.list();
        assert!(list.len() > 1);
        assert!(is_open(std::slice::from_ref(&me), &list), "{} is not in the list", me.display());
        assert!(list.iter().any(|p| p.path.as_deref().is_some_and(|path| same_path(path, &me))));
    }

    #[test]
    fn says_the_program_is_open() {
        let app = SuiteApp::by_id("openref").unwrap();
        assert_eq!(open_error(app), "OpenRef is open; close it and try again.");
    }
}
