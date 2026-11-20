// SPDX-License-Identifier: GPL-3.0-or-later
//! The programs' Windows Installer packages (D-047): installing, updating and removing a program
//! for everyone on the computer, in `Program Files\Open3D\<App>`, and finding the copy installed.
//!
//! Each program's MSI (made by the OPEN3D packager's `repo_msi.py`) is per machine, so Windows asks
//! for permission at each install, update and removal. `msiexec` is started the way any program is,
//! never elevated by the launcher, and waited for: `/i "<msi>" /passive /norestart /l*v "<log>"`, or
//! `/x {ProductCode}` to remove. Only one runs at a time ([`INSTALLER`]): "Update all" runs them one
//! after another, each with its own prompt. From the checksum check to `msiexec`'s end the package
//! is held open so that nobody can change it ([`hold`]).
//!
//! The package records no folder: the program is in `%ProgramW6432%\Open3D\<App>`. Its ProductCode
//! is new with each build, so it is always read from its Uninstall key ([`find`]).
//!
//! Based on ArtCraft Launcher's `install.rs`, `detect.rs` and `install/msi_batch.rs` (MIT OR
//! Apache-2.0, see NOTICE): the `msiexec` command, its exit codes, the Uninstall keys and the held
//! package. Not its single elevated prompt for several packages, which D-047 rules out.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use open3d_launcher_core::protocol::{CANCELLED, JobPhase};
use open3d_launcher_core::{InstallSource, Installed, SuiteApp, version};

use crate::http::Stop;

/// Who the programs' packages name as their Manufacturer, which Windows shows as their Publisher.
pub(crate) const PUBLISHER: &str = "Open3D";

/// Only debug builds read it: a program started in place of `msiexec`, with the same arguments,
/// for trying the launcher by hand without installing anything. Release builds ignore it.
pub(crate) const TEST_MSIEXEC: &str = "OPEN3D_LAUNCHER_TEST_MSIEXEC";

// ---- what is installed ------------------------------------------------------------------------

/// A key under `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`, as read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UninstallEntry {
    /// The key's name: a product code for a Windows Installer package.
    pub key: String,
    pub display_name: String,
    pub publisher: String,
    pub display_version: String,
}

/// The Uninstall keys. Tests give their own.
pub(crate) trait Registry {
    fn uninstall_entries(&self) -> Vec<UninstallEntry>;
}

/// This computer's Uninstall keys: the machine's, in its 64-bit view (where a per-machine x64
/// package registers), and for safety its 32-bit view and this account's own.
pub(crate) struct WindowsRegistry;

impl Registry for WindowsRegistry {
    fn uninstall_entries(&self) -> Vec<UninstallEntry> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
        const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
        let roots = [(HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY), (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY), (HKEY_CURRENT_USER, 0)];
        let mut found = Vec::new();
        for (hive, view) in roots {
            let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | view) else { continue };
            for key in root.enum_keys().flatten() {
                if !product_code(&key) {
                    continue;
                }
                let Ok(entry) = root.open_subkey_with_flags(&key, KEY_READ | view) else { continue };
                let text = |name: &str| entry.get_value::<String, _>(name).unwrap_or_default();
                found.push(UninstallEntry {
                    display_name: text("DisplayName"),
                    publisher: text("Publisher"),
                    display_version: text("DisplayVersion"),
                    key,
                });
            }
        }
        found
    }
}

/// Whether `key` is a Windows Installer product code: a GUID in braces.
pub(crate) fn product_code(key: &str) -> bool {
    let Some(inner) = key.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')) else { return false };
    let parts: Vec<&str> = inner.split('-').collect();
    parts.iter().map(|part| part.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|part| part.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A program's package as its Uninstall key says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Registered {
    pub product_code: String,
    pub version: Option<version::Version>,
}

/// `app`'s package among `entries`: a key that is a product code, whose DisplayName is the app's
/// name and whose Publisher is the catalog's (`Open3D`). With more than one, never seen as a major
/// upgrade replaces the one installed, the newest.
pub(crate) fn find(app: &SuiteApp, entries: &[UninstallEntry]) -> Option<Registered> {
    let publisher = app.msi_publisher.unwrap_or(PUBLISHER);
    entries
        .iter()
        .filter(|entry| product_code(&entry.key) && entry.publisher == publisher && app.names().any(|name| name == entry.display_name))
        .map(|entry| Registered { product_code: entry.key.clone(), version: version::parse(&entry.display_version) })
        .max_by(|a, b| match (&a.version, &b.version) {
            (Some(a), Some(b)) => version::compare(a, b),
            (a, b) => a.is_some().cmp(&b.is_some()),
        })
}

/// `Open3D` in the 64-bit Program Files (`%ProgramW6432%`, the package's `ProgramFiles64Folder`),
/// where every program's package puts its folder. `None` when Windows names no such folder.
pub(crate) fn suite_folder() -> Option<PathBuf> {
    ["ProgramW6432", "ProgramFiles"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|folder| folder.is_absolute())
        .map(|folder| folder.join("Open3D"))
}

/// The program's exe in `suite` (see [`suite_folder`]): the catalog's `exe` in `<suite>\<App>`.
pub(crate) fn exe_in(app: &SuiteApp, suite: &Path) -> PathBuf {
    crate::standalone::exe_in(&suite.join(app.name), app)
}

/// The copy `app`'s package installed, if any, with its version and product code.
pub(crate) fn detect(app: &SuiteApp) -> Option<Installed> {
    detect_in(app, &WindowsRegistry, &suite_folder()?)
}

/// [`detect`] with `registry`'s keys and the programs in `suite`.
pub(crate) fn detect_in(app: &SuiteApp, registry: &dyn Registry, suite: &Path) -> Option<Installed> {
    let found = find(app, &registry.uninstall_entries())?;
    Some(Installed {
        version: found.version,
        source: InstallSource::Msi,
        location: exe_in(app, suite).display().to_string(),
        package: Some(found.product_code),
        externally_managed: false,
        origin: None,
        installation: None,
    })
}

// ---- running msiexec --------------------------------------------------------------------------

/// What a package is asked to do, which the messages say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Install,
    Remove,
}

/// What a Windows Installer exit code means. 0, 3010 and 1641 are success (the last two ask for a
/// restart, which `/norestart` leaves to the person); 1602 is the permission prompt or the progress
/// window cancelled ([`CANCELLED`]); the rest are errors, with the log for 1603 and the others.
pub(crate) fn explain(app: &SuiteApp, action: Action, code: Option<i32>, log: &Path) -> Result<(), String> {
    let not = match action {
        Action::Install => "wasn't installed",
        Action::Remove => "wasn't removed",
    };
    match code {
        Some(0 | 3010 | 1641) => Ok(()),
        Some(1602) => Err(CANCELLED.into()),
        Some(1618) => Err(format!("{} {not}: another installation is in progress. Wait for it to finish, then try again.", app.name)),
        Some(1925 | 1303) => Err(format!("{} {not}: Windows didn't allow it, administrator permission is needed.", app.name)),
        Some(code) => Err(format!("{} {not}: the Windows Installer stopped with error {code}; its log is at {}", app.name, log.display())),
        None => Err(format!("{} {not}: the Windows Installer was interrupted; its log is at {}", app.name, log.display())),
    }
}

/// `msiexec`'s arguments to install or update from `msi`, with a progress window and no restart,
/// and a full log: Windows Installer's own errors (1603 above all) say nothing without it.
pub(crate) fn install_args(msi: &Path, log: &Path) -> Vec<OsString> {
    vec!["/i".into(), msi.into(), "/passive".into(), "/norestart".into(), "/l*v".into(), log.into()]
}

/// `msiexec`'s arguments to remove the product `code`.
pub(crate) fn uninstall_args(code: &str, log: &Path) -> Vec<OsString> {
    vec!["/x".into(), code.into(), "/passive".into(), "/norestart".into(), "/l*v".into(), log.into()]
}

/// Starts `msiexec` and waits for it. Tests give their own.
pub(crate) trait Msiexec {
    /// Runs it with `args` and gives its exit code; `None` when it ended without one.
    fn run(&self, args: &[OsString]) -> Result<Option<i32>, String>;
}

/// Windows' own `msiexec`, started as any program is: Windows itself asks for permission.
pub(crate) struct SystemMsiexec;

impl Msiexec for SystemMsiexec {
    fn run(&self, args: &[OsString]) -> Result<Option<i32>, String> {
        let program = msiexec_program()?;
        log::info!("running {} {}", program.display(), args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "));
        let status = crate::process::command(&program).args(args).status().map_err(|e| format!("couldn't start the Windows Installer ({e})"))?;
        Ok(status.code())
    }
}

/// `%SystemRoot%\System32\msiexec.exe`; in a debug build, the program [`TEST_MSIEXEC`] names.
fn msiexec_program() -> Result<PathBuf, String> {
    if cfg!(debug_assertions)
        && let Some(fake) = std::env::var_os(TEST_MSIEXEC).filter(|fake| !fake.is_empty())
    {
        return Ok(PathBuf::from(fake));
    }
    let root =
        std::env::var_os("SystemRoot").map(PathBuf::from).filter(|root| root.is_absolute()).ok_or("Windows didn't say where its own folder is")?;
    Ok(root.join("System32").join("msiexec.exe"))
}

/// One Windows Installer job at a time: a second `msiexec` would stop with 1618. Jobs that come
/// while one runs wait their turn.
pub(crate) struct Turns {
    busy: Mutex<bool>,
    freed: Condvar,
}

/// The launcher's one queue for the Windows Installer.
pub(crate) static INSTALLER: Turns = Turns::new();

impl Turns {
    pub(crate) const fn new() -> Self {
        Self { busy: Mutex::new(false), freed: Condvar::new() }
    }

    /// Runs `work` once no other job runs a package, saying [`JobPhase::Waiting`] to `phase` while
    /// it waits, and gives `phase` on to `work`. Cancelled (`stop`) while it waits, it never runs.
    pub(crate) fn alone<T>(
        &self,
        stop: &Stop,
        phase: &mut dyn FnMut(JobPhase),
        work: impl FnOnce(&mut dyn FnMut(JobPhase)) -> T,
    ) -> Result<T, String> {
        {
            let mut busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
            let mut told = false;
            while *busy {
                stop.check().map_err(|_| CANCELLED.to_string())?;
                if !told {
                    told = true;
                    phase(JobPhase::Waiting);
                }
                busy = self.freed.wait_timeout(busy, Duration::from_millis(100)).unwrap_or_else(PoisonError::into_inner).0;
            }
            *busy = true;
        }
        let _turn = Turn(self);
        Ok(work(phase))
    }
}

/// Gives the turn back when the work ends, however it ends.
struct Turn<'a>(&'a Turns);

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.0.freed.notify_all();
    }
}

/// A checked package, kept exactly as checked until its installer has run.
///
/// The download is checked against its published SHA-256 in the launcher's own folder, and
/// `msiexec` reads it later. Holding it open while letting others only read it stops anyone from
/// changing, moving or deleting it meanwhile, and hashing it again through that same handle proves
/// that the bytes held are the ones checked. `msiexec` can still open it to read.
pub(crate) struct Held {
    _file: std::fs::File,
}

/// Opens `msi` so that it cannot be changed, moved or deleted while held, and checks that it still
/// has the `sha256` it was downloaded against.
pub(crate) fn hold(msi: &Path, sha256: &str) -> Result<Held, String> {
    use std::os::windows::fs::OpenOptionsExt;

    use sha2::{Digest, Sha256};

    /// Others may read the file, but not write, rename or delete it.
    const FILE_SHARE_READ: u32 = 0x1;
    let mut file = std::fs::File::options()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(msi)
        .map_err(|e| format!("couldn't keep the checked package {} safe until it is installed ({e})", msi.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("couldn't read {} ({e})", msi.display()))?;
    let digest: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if !digest.eq_ignore_ascii_case(sha256.trim()) {
        return Err(format!("{} changed after it was checked, so it wasn't installed. Try again.", msi.display()));
    }
    Ok(Held { _file: file })
}

/// What a package's job needs: who runs `msiexec`, the queue, the job's stop and phase, where the
/// log goes, and a last look that the program is still closed (D-157) once its turn has come.
pub(crate) struct Run<'a> {
    pub msiexec: &'a dyn Msiexec,
    pub turns: &'a Turns,
    pub stop: &'a Stop,
    pub phase: &'a mut dyn FnMut(JobPhase),
    pub log: PathBuf,
    pub closed: &'a dyn Fn() -> Result<(), String>,
}

/// Installs or updates `app` from the downloaded `msi`, checked against `sha256`, and waits for it.
pub(crate) fn install(app: &SuiteApp, msi: &Path, sha256: &str, run: Run<'_>) -> Result<(), String> {
    // Windows Installer can fail on a path through a link; dunce keeps it in ordinary drive syntax.
    let msi = dunce::canonicalize(msi).map_err(|e| format!("couldn't find the downloaded package {} ({e})", msi.display()))?;
    let _held = hold(&msi, sha256)?;
    let Run { msiexec, turns, stop, phase, log, closed } = run;
    turns.alone(stop, phase, |phase| {
        closed()?;
        prepare_log(&log)?;
        phase(JobPhase::Permission);
        log::info!("{}: installing {}; Windows asks for permission", app.name, msi.display());
        let code = msiexec.run(&install_args(&msi, &log))?;
        explain(app, Action::Install, code, &log)
    })?
}

/// Removes `app`'s package, the one `registry` names now, and waits for it.
pub(crate) fn uninstall(app: &SuiteApp, registry: &dyn Registry, run: Run<'_>) -> Result<(), String> {
    let Run { msiexec, turns, stop, phase, log, closed } = run;
    turns.alone(stop, phase, |phase| {
        closed()?;
        let found = find(app, &registry.uninstall_entries()).ok_or_else(|| format!("{} isn't registered with the Windows Installer", app.name))?;
        prepare_log(&log)?;
        phase(JobPhase::Permission);
        log::info!("{}: removing {}; Windows asks for permission", app.name, found.product_code);
        let code = msiexec.run(&uninstall_args(&found.product_code, &log))?;
        explain(app, Action::Remove, code, &log)
    })?
}

/// Makes the folder of `log`: `msiexec` can't write a log into a folder that isn't there (1622).
fn prepare_log(log: &Path) -> Result<(), String> {
    let folder = log.parent().ok_or("the log has no folder")?;
    std::fs::create_dir_all(folder).map_err(|e| format!("couldn't create {} ({e})", folder.display()))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    const CODE: &str = "{3F2B1C4D-5E6F-4A7B-8C9D-0E1F2A3B4C5D}";

    fn openref() -> &'static SuiteApp {
        SuiteApp::by_id("openref").unwrap()
    }

    fn entry(key: &str, name: &str, publisher: &str, version: &str) -> UninstallEntry {
        UninstallEntry { key: key.into(), display_name: name.into(), publisher: publisher.into(), display_version: version.into() }
    }

    struct FakeRegistry(Vec<UninstallEntry>);

    impl Registry for FakeRegistry {
        fn uninstall_entries(&self) -> Vec<UninstallEntry> {
            self.0.clone()
        }
    }

    /// `msiexec` that answers `code` and keeps the arguments it was given.
    struct FakeMsiexec {
        code: Option<i32>,
        calls: RefCell<Vec<Vec<OsString>>>,
    }

    impl FakeMsiexec {
        fn answering(code: Option<i32>) -> Self {
            Self { code, calls: RefCell::new(Vec::new()) }
        }
    }

    impl Msiexec for FakeMsiexec {
        fn run(&self, args: &[OsString]) -> Result<Option<i32>, String> {
            self.calls.borrow_mut().push(args.to_vec());
            Ok(self.code)
        }
    }

    #[test]
    fn a_product_code_is_a_guid_in_braces() {
        assert!(product_code(CODE));
        assert!(product_code("{3f2b1c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d}"));
        for key in [
            "OpenRef",
            "3F2B1C4D-5E6F-4A7B-8C9D-0E1F2A3B4C5D",
            "{3F2B1C4D-5E6F-4A7B-8C9D-0E1F2A3B4C5}",
            "{3F2B1C4D-5E6F-4A7B-8C9D-0E1F2A3B4C5DX}",
            "{3F2B1C4D5E6F-4A7B-8C9D-0E1F2A3B4C5D}",
            "{G F2B1C4D-5E6F-4A7B-8C9D-0E1F2A3B4C5D}",
            "",
        ] {
            assert!(!product_code(key), "{key}");
        }
    }

    #[test]
    fn finds_the_program_by_its_product_code_name_and_publisher() {
        let app = openref();
        let theirs = [
            entry("OpenRef", "OpenRef", "Open3D", "0.9.0"),
            entry("{11111111-2222-3333-4444-555555555555}", "OpenRef", "Someone else", "0.9.0"),
            entry("{22222222-2222-3333-4444-555555555555}", "OpenRef Viewer", "Open3D", "0.9.0"),
            entry("{33333333-2222-3333-4444-555555555555}", "OpenView", "Open3D", "0.9.0"),
        ];
        assert_eq!(find(app, &theirs), None);
        let mut entries = theirs.to_vec();
        entries.push(entry(CODE, "OpenRef", "Open3D", "0.2.0"));
        assert_eq!(find(app, &entries), Some(Registered { product_code: CODE.into(), version: version::parse("0.2.0") }));
        // Two, which a major upgrade never leaves: the newest.
        entries.push(entry("{44444444-2222-3333-4444-555555555555}", "OpenRef", "Open3D", "0.3.0"));
        assert_eq!(find(app, &entries).map(|r| r.product_code), Some("{44444444-2222-3333-4444-555555555555}".into()));
    }

    #[test]
    fn the_copy_found_is_in_program_files_with_the_catalogs_exe() {
        let suite = Path::new(r"C:\Program Files\Open3D");
        let registry = FakeRegistry(vec![entry(CODE, "OpenRef", "Open3D", "0.2.0")]);
        let found = detect_in(openref(), &registry, suite).unwrap();
        assert_eq!(found.source, InstallSource::Msi);
        assert_eq!(found.version, version::parse("0.2.0"));
        assert_eq!(found.location, r"C:\Program Files\Open3D\OpenRef\OpenRef.exe");
        assert_eq!(found.package.as_deref(), Some(CODE));
        // OpenGaussian's program sits in `bin`.
        let gaussian = SuiteApp::by_id("opengaussian").unwrap();
        assert_eq!(exe_in(gaussian, suite), Path::new(r"C:\Program Files\Open3D\OpenGaussian\bin\OpenGaussian.exe"));
        assert_eq!(detect_in(gaussian, &registry, suite), None);
    }

    #[test]
    fn every_exit_code_says_what_happened() {
        let (app, log) = (openref(), Path::new(r"C:\Logs\openref-install.log"));
        for code in [0, 3010, 1641] {
            assert_eq!(explain(app, Action::Install, Some(code), log), Ok(()));
        }
        assert_eq!(explain(app, Action::Install, Some(1602), log), Err(CANCELLED.into()));
        assert_eq!(
            explain(app, Action::Install, Some(1618), log),
            Err("OpenRef wasn't installed: another installation is in progress. Wait for it to finish, then try again.".into())
        );
        for code in [1925, 1303] {
            assert_eq!(
                explain(app, Action::Install, Some(code), log),
                Err("OpenRef wasn't installed: Windows didn't allow it, administrator permission is needed.".into())
            );
        }
        assert_eq!(
            explain(app, Action::Remove, Some(1603), log),
            Err(r"OpenRef wasn't removed: the Windows Installer stopped with error 1603; its log is at C:\Logs\openref-install.log".into())
        );
        assert!(explain(app, Action::Install, None, log).unwrap_err().contains("was interrupted"));
    }

    #[test]
    fn msiexec_installs_and_removes_quietly_with_a_log_and_no_restart() {
        let (msi, log) = (Path::new(r"C:\Down loads\openref-0.2.0-windows-x64.msi"), Path::new(r"C:\Logs\openref-install.log"));
        let words = |args: Vec<OsString>| args.into_iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(
            words(install_args(msi, log)),
            ["/i", r"C:\Down loads\openref-0.2.0-windows-x64.msi", "/passive", "/norestart", "/l*v", r"C:\Logs\openref-install.log"]
        );
        assert_eq!(words(uninstall_args(CODE, log)), ["/x", CODE, "/passive", "/norestart", "/l*v", r"C:\Logs\openref-install.log"]);
        assert!(msiexec_program().unwrap().ends_with(r"System32\msiexec.exe") || std::env::var_os(TEST_MSIEXEC).is_some());
    }

    /// A downloaded package with its checksum.
    fn package(dir: &Path) -> (PathBuf, String) {
        use sha2::{Digest, Sha256};
        let msi = dir.join("openref-0.2.0-windows-x64.msi");
        std::fs::write(&msi, b"an MSI").unwrap();
        let sha = Sha256::digest(b"an MSI").iter().map(|b| format!("{b:02x}")).collect();
        (msi, sha)
    }

    /// Runs [`install`] with `msiexec`, saying each phase to `phases`.
    fn install_with(
        msi: &Path,
        sha: &str,
        msiexec: &FakeMsiexec,
        log: &Path,
        closed: &dyn Fn() -> Result<(), String>,
    ) -> (Result<(), String>, Vec<JobPhase>) {
        let mut phases = Vec::new();
        let stop = Stop::default();
        let result = install(
            openref(),
            msi,
            sha,
            Run { msiexec, turns: &Turns::new(), stop: &stop, phase: &mut |p| phases.push(p), log: log.to_path_buf(), closed },
        );
        (result, phases)
    }

    #[test]
    fn installing_runs_the_held_package_and_says_that_windows_asks() {
        let tmp = tempfile::tempdir().unwrap();
        let (msi, sha) = package(tmp.path());
        let log = tmp.path().join("logs").join("openref-install.log");
        for (code, ended) in [(Some(0), Ok(())), (Some(1602), Err(CANCELLED.to_string()))] {
            let msiexec = FakeMsiexec::answering(code);
            let (result, phases) = install_with(&msi, &sha, &msiexec, &log, &|| Ok(()));
            assert_eq!(result, ended);
            assert_eq!(phases, [JobPhase::Permission]);
            let calls = msiexec.calls.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0][0], "/i");
            assert_eq!(PathBuf::from(&calls[0][1]), dunce::canonicalize(&msi).unwrap());
            assert!(log.parent().unwrap().is_dir(), "msiexec can't write a log into a missing folder");
        }
        // Once it has run the package is free again: the next install can hold it.
        assert!(hold(&msi, &sha).is_ok());
    }

    #[test]
    fn a_program_opened_meanwhile_stops_the_install_before_msiexec() {
        let tmp = tempfile::tempdir().unwrap();
        let (msi, sha) = package(tmp.path());
        let msiexec = FakeMsiexec::answering(Some(0));
        let (result, _) =
            install_with(&msi, &sha, &msiexec, &tmp.path().join("install.log"), &|| Err("OpenRef is open; close it and try again.".into()));
        assert_eq!(result, Err("OpenRef is open; close it and try again.".into()));
        assert!(msiexec.calls.borrow().is_empty());
    }

    #[test]
    fn a_package_changed_after_its_check_is_never_run() {
        let tmp = tempfile::tempdir().unwrap();
        let (msi, sha) = package(tmp.path());
        std::fs::write(&msi, b"another MSI").unwrap();
        let msiexec = FakeMsiexec::answering(Some(0));
        let (result, _) = install_with(&msi, &sha, &msiexec, &tmp.path().join("install.log"), &|| Ok(()));
        assert!(result.unwrap_err().contains("changed after it was checked"));
        assert!(msiexec.calls.borrow().is_empty());
    }

    #[test]
    fn a_held_package_can_be_read_but_not_changed_moved_or_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let (msi, sha) = package(tmp.path());
        assert!(hold(&msi, "0".repeat(64).as_str()).is_err());
        let held = hold(&msi, &sha).unwrap();
        assert_eq!(std::fs::read(&msi).unwrap(), b"an MSI");
        assert!(std::fs::OpenOptions::new().write(true).open(&msi).is_err());
        assert!(std::fs::rename(&msi, tmp.path().join("moved.msi")).is_err());
        assert!(std::fs::remove_file(&msi).is_err());
        drop(held);
        std::fs::remove_file(&msi).unwrap();
    }

    #[test]
    fn removing_runs_msiexec_with_the_product_code_read_now() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("openref-uninstall.log");
        let registry = FakeRegistry(vec![entry(CODE, "OpenRef", "Open3D", "0.2.0")]);
        for (code, ended) in [(Some(0), Ok(())), (Some(1602), Err(CANCELLED.to_string()))] {
            let msiexec = FakeMsiexec::answering(code);
            let (stop, mut phases) = (Stop::default(), Vec::new());
            let run =
                Run { msiexec: &msiexec, turns: &Turns::new(), stop: &stop, phase: &mut |p| phases.push(p), log: log.clone(), closed: &|| Ok(()) };
            assert_eq!(uninstall(openref(), &registry, run), ended);
            assert_eq!(phases, [JobPhase::Permission]);
            assert_eq!(msiexec.calls.borrow()[0], uninstall_args(CODE, &log));
        }
        let msiexec = FakeMsiexec::answering(Some(0));
        let (stop, mut phases) = (Stop::default(), Vec::new());
        let run = Run { msiexec: &msiexec, turns: &Turns::new(), stop: &stop, phase: &mut |p| phases.push(p), log, closed: &|| Ok(()) };
        assert_eq!(uninstall(openref(), &FakeRegistry(Vec::new()), run), Err("OpenRef isn't registered with the Windows Installer".into()));
        assert!(msiexec.calls.borrow().is_empty());
    }

    /// One package at a time: a second job waits, saying so, and runs once the first ends; a job
    /// cancelled while it waits never runs.
    #[test]
    fn one_package_runs_at_a_time() {
        let turns = Arc::new(Turns::new());
        let first_running = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let first = {
            let (turns, running, release) = (turns.clone(), first_running.clone(), release.clone());
            std::thread::spawn(move || {
                turns
                    .alone(&Stop::default(), &mut |_| {}, |_| {
                        running.store(true, Ordering::SeqCst);
                        while !release.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    })
                    .unwrap();
            })
        };
        while !first_running.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Cancelled while it waits: it never runs.
        let stop = Stop::default();
        stop.cancel();
        let mut phases = Vec::new();
        assert_eq!(turns.alone(&stop, &mut |p| phases.push(p), |_| panic!("ran while another package ran")), Err(CANCELLED.to_string()));
        // Waiting, then its turn.
        let second = {
            let turns = turns.clone();
            std::thread::spawn(move || {
                let mut phases = Vec::new();
                turns.alone(&Stop::default(), &mut |p| phases.push(p), |_| ()).unwrap();
                phases
            })
        };
        std::thread::sleep(Duration::from_millis(150));
        assert!(!second.is_finished());
        release.store(true, Ordering::SeqCst);
        first.join().unwrap();
        assert_eq!(second.join().unwrap(), [JobPhase::Waiting]);
        // Free again, and a work that panics gives its turn back.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| turns.alone(&Stop::default(), &mut |_| {}, |_| panic!("boom"))));
        assert!(panicked.is_err());
        assert_eq!(turns.alone(&Stop::default(), &mut |_| {}, |_| 7), Ok(7));
    }
}
