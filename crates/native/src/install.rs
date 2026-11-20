// SPDX-License-Identifier: GPL-3.0-or-later
//! Installing, updating, removing and starting Open3D apps.
//!
//! An Open3D app is a 3ds Max plug-in bundle, unpacked into
//! `%APPDATA%\Autodesk\ApplicationPlugins\<App>.bundle` ([`crate::max_bundle`]), or a program of
//! its own: installed for everyone by its Windows Installer package into
//! `Program Files\Open3D\<App>` when the release has one ([`crate::msi`], D-047), else unpacked
//! from its zip into `%LOCALAPPDATA%\Programs\Open3D\<App>` ([`crate::standalone`]). A portable
//! launcher always takes the zip, into its own folder (D-153).
//!
//! Every download is checked against its published SHA-256 before anything is unpacked, a bundle
//! is never changed while 3ds Max runs nor a program while it runs itself, and a new copy is fully
//! in place before the old one goes.

use std::path::{Path, PathBuf};

use open3d_launcher_core::Arch;
use open3d_launcher_core::protocol::{CANCELLED, INSTALL_CANCELLED, InstallLocations, JobPhase};
use open3d_launcher_core::release::{self, Asset, Release};
use open3d_launcher_core::{InstallSource, Installed, PackageKind, SuiteApp, version};

use crate::http::{Http, Stop, Transfer};
use crate::manifest::Record;
use crate::paths::Paths;
use crate::process;

/// What an install needs from the backend.
pub(crate) struct Job<'a> {
    pub http: &'a Http,
    pub paths: &'a Paths,
    pub stop: &'a Stop,
    pub phase: &'a mut dyn FnMut(JobPhase),
    pub install_locations: &'a InstallLocations,
    /// Write over whatever [`occupied`] found.
    pub replace: bool,
    /// Set only when the person chose this older version by name over a newer installed copy.
    pub downgrade: Option<Downgrade>,
}

/// How a downgrade the person chose is undone if it fails: the installed copy stays until the
/// older one is in place.
#[derive(Clone, Copy)]
pub(crate) enum Downgrade {
    KeepOldCopy,
}

type RecordCommit<'a> = Option<&'a mut dyn FnMut(&Record) -> Result<(), String>>;

/// Something the user should hear along with the news that an install or a removal worked.
pub(crate) type Note = Option<&'static str>;

#[allow(clippy::too_many_arguments)]
pub(crate) fn install_at(
    app: &SuiteApp,
    release: &Release,
    asset: &Asset,
    kind: PackageKind,
    existing: Option<&Installed>,
    previous: Option<&Record>,
    job: &mut Job<'_>,
    commit: RecordCommit<'_>,
) -> Result<(Option<Record>, Note), String> {
    #[cfg(windows)]
    let _commit = commit;
    let _destination = destination(app, kind, existing, previous, job.install_locations)?;
    // A program is asked about below, by its own copies: another copy of it, started from
    // somewhere else, has the same process name and is no reason to wait.
    if let Some(inst) = existing
        && !app.is_standalone()
        && inst.source.launcher_can_update()
        && process::is_running(Path::new(&inst.location))
    {
        return Err(format!("{} is running. Quit it, then try again.", app.name));
    }
    if kind == PackageKind::MaxBundle && crate::max_bundle::max_running() {
        return Err(crate::max_bundle::max_running_error());
    }
    // Never a program that is open (D-157).
    if app.is_standalone() {
        program_closed(app)?;
    }
    bind_artifact_version(app, release, asset)?;
    // A feed can't send an app back to an older build; only a version the person chose by name
    // (a downgrade with a way back if it fails) gets past this.
    if !job.replace
        && job.downgrade.is_none()
        && let Some(inst) = existing
        && let (Some(rel_v), Some(inst_v)) = (release.version.as_ref(), inst.version.as_ref())
        && version::is_newer(inst_v, rel_v)
    {
        return Err(format!("Cannot update {} to {}: installed version {} is newer", app.name, release.version_label(), inst_v));
    }
    // A program's package never goes back to an older version (it stops with a message), so a
    // version chosen by name that is older than the copy it installed is refused before anything
    // is downloaded.
    if kind == PackageKind::Msi
        && let Some(inst) = existing.filter(|inst| inst.source == InstallSource::Msi)
        && let (Some(rel_v), Some(inst_v)) = (release.version.as_ref(), inst.version.as_ref())
        && version::is_newer(inst_v, rel_v)
    {
        return Err(format!(
            "{} {} is older than the version installed ({inst_v}), and the Windows Installer doesn't go back: uninstall it first, then install this version.",
            app.name,
            release.version_label()
        ));
    }
    log::info!("{}: starting {} to {} ({})", app.name, if existing.is_some() { "update" } else { "install" }, release.version_label(), kind.label());
    let (file, sha256) = download(release, asset, job)?;
    log::info!("{}: download ready ({}), running installer...", app.name, file.display());
    // Past this point the install can't be interrupted halfway. A package says its own phases:
    // waiting for its turn, then Windows' permission prompt.
    if kind != PackageKind::Msi {
        (job.phase)(JobPhase::Installing);
    }
    let version = release.version_label();
    let result = match kind {
        _ if !app.kinds().contains(&kind) => Err(format!("{} isn't installed from a {}", app.name, kind.label())),
        // Windows Installer keeps its own record of the package: the launcher keeps none.
        PackageKind::Msi => install_msi(app, &file, &sha256, job).map(|note| (None, note)),
        PackageKind::PortableZip => Err(format!("{} isn't installed from a {}", app.name, kind.label())),
        PackageKind::MaxBundle => {
            crate::max_bundle::install(app, &file, &version).map(|(record, old)| (Some(record), crate::max_bundle::still_running_note(old)))
        }
        PackageKind::AppZip => {
            crate::standalone::install(app, &file, &version).map(|(record, old)| (Some(record), crate::standalone::still_running_note(old)))
        }
    };
    match &result {
        Ok(_) => log::info!("{}: successfully installed/updated to {}", app.name, release.version_label()),
        Err(e) if e == CANCELLED => log::info!("{}: installation cancelled", app.name),
        Err(e) => log::error!("{}: installation failed: {e}", app.name),
    }
    // A failed install keeps the download: the next try checks it against its published checksum
    // again and skips the transfer.
    if result.is_ok() {
        let _ = std::fs::remove_file(&file);
    }
    // The system's own prompt was dismissed: the download itself wasn't cancelled.
    result.map_err(|e| if e == CANCELLED { INSTALL_CANCELLED.to_string() } else { e })
}

/// The copies of a program the launcher looks after: the one its Windows Installer package puts
/// in `Program Files`, and the one a zip put in this account's programs folder (a portable
/// launcher's: only its own).
pub(crate) fn program_exes(app: &SuiteApp) -> Vec<PathBuf> {
    let for_everyone = if crate::paths::portable_root().is_none() { crate::msi::suite_folder() } else { None };
    for_everyone.map(|suite| crate::msi::exe_in(app, &suite)).into_iter().chain(crate::standalone::exe_path(app)).collect()
}

/// `Ok` unless a copy of the program `app` is open (D-157).
fn program_closed(app: &SuiteApp) -> Result<(), String> {
    closed_now(app, &program_exes(app))
}

/// `Ok` unless one of `exes`, the copies of `app`, runs now.
fn closed_now(app: &SuiteApp, exes: &[PathBuf]) -> Result<(), String> {
    use crate::running::Processes;
    if crate::running::is_open(exes, &crate::running::SystemProcesses.list()) {
        return Err(crate::running::open_error(app));
    }
    Ok(())
}

/// What a finished install or update adds once the program's package has replaced the copy a zip
/// had put in the user's folder (D-156).
pub(crate) const OLD_COPY_REMOVED: &str = "Its earlier copy in your user folder was removed, with its shortcuts; your settings and data stay.";
/// The same, when that copy couldn't be removed.
pub(crate) const OLD_COPY_KEPT: &str = "Its earlier copy in your user folder couldn't be removed; delete it yourself if it is still there.";

/// Installs or updates the program from its Windows Installer package, for everyone, and once
/// that has worked removes the copy an earlier zip left in the user's folder (D-156). A package
/// that fails or is cancelled leaves that copy as it was.
fn install_msi(app: &SuiteApp, msi: &Path, sha256: &str, job: &mut Job<'_>) -> Result<Note, String> {
    let exes = program_exes(app);
    let closed = || closed_now(app, &exes);
    let log = job.paths.logs().join(format!("{}-install.log", app.id));
    let run = crate::msi::Run {
        msiexec: &crate::msi::SystemMsiexec,
        turns: &crate::msi::INSTALLER,
        stop: job.stop,
        phase: &mut *job.phase,
        log,
        closed: &closed,
    };
    crate::msi::install(app, msi, sha256, run)?;
    Ok(crate::standalone::Places::system().and_then(|places| remove_old_copy(app, &places)))
}

/// Removes `app`'s copy in `places`, with its shortcuts, and says so; `None` when there is none.
/// It is removed the way an uninstall removes it: moved aside first, then deleted as far as nothing
/// holds it. Its settings and data live elsewhere and stay.
pub(crate) fn remove_old_copy(app: &SuiteApp, places: &crate::standalone::Places) -> Note {
    let found = crate::standalone::detect_in(app, places, &|_: &Path| None)?;
    match crate::standalone::remove(app, Path::new(&found.location), places) {
        Ok(_) => {
            log::info!("{}: removed the earlier copy in {}", app.name, found.location);
            Some(OLD_COPY_REMOVED)
        }
        Err(e) => {
            log::warn!("{}: couldn't remove the earlier copy in {}: {e}", app.name, found.location);
            Some(OLD_COPY_KEPT)
        }
    }
}

/// Rejects an artifact whose own identity disagrees with the release that authorized it.
///
/// A feed or GitHub converter supplies the release version, the asset filename, and the download
/// URL independently. The downgrade guard and the persisted record both trust `release.version`,
/// so a controller could advertise a new version while pointing at an older official artifact —
/// bypassing the guard and recording a false installed version (finding: version not bound to the
/// release). We require the version encoded in the asset filename and the release tag embedded in
/// the download URL to agree with the release version before any bytes are fetched.
pub(crate) fn bind_artifact_version(app: &SuiteApp, release: &Release, asset: &Asset) -> Result<(), String> {
    let mismatch = || format!("{}'s download does not match the advertised version {}.", app.name, release.version_label());
    let release_version = release.version.as_ref();

    // The version in the asset filename (`<stem>-<version>-<os>-<arch>.<ext>`), when it carries one.
    let stems: Vec<&str> = app.stems().collect();
    if let Some(release_version) = release_version
        && let Some(parsed) = open3d_launcher_core::platform::parse_asset_name(&asset.name, &stems)
        && version::parse(&parsed.version).as_ref() != Some(release_version)
    {
        return Err(mismatch());
    }

    // The release tag and filename embedded in a release download URL (`.../releases/download/<tag>/<name>`).
    if let Some(rest) = asset.url.split("/releases/download/").nth(1) {
        let mut parts = rest.split('/');
        let tag = parts.next().unwrap_or("");
        let url_filename = parts.next().unwrap_or("");
        if tag.is_empty()
            || release_version
                .map_or(tag != release.tag, |release_version| tag != release.tag && version::parse(tag).as_ref() != Some(release_version))
        {
            return Err(mismatch());
        }
        if !url_filename.is_empty() && url_filename != asset.name {
            return Err(format!("{}'s download URL filename does not match selected asset {}.", app.name, asset.name));
        }
    }
    Ok(())
}

/// What a new install would write over: something already at its destination that the launcher
/// didn't put there.
pub(crate) fn occupied(
    app: &SuiteApp,
    _asset: &Asset,
    kind: PackageKind,
    existing: Option<&Installed>,
    previous: Option<&Record>,
    locations: &InstallLocations,
    paths: &Paths,
) -> Result<Option<PathBuf>, String> {
    if existing.is_some() || previous.is_some() {
        return Ok(None);
    }
    let folder = destination(app, kind, None, None, locations)?;
    let target = match kind {
        PackageKind::Msi => folder,
        PackageKind::PortableZip => Some(folder.unwrap_or_else(|| paths.apps()).join(app.id)),
        PackageKind::MaxBundle => crate::max_bundle::bundle_dir(app),
        // The app's folder with something else in it: a copy with its program would have been detected.
        PackageKind::AppZip => crate::standalone::Places::system().map(|places| crate::standalone::app_dir(app, &places)),
    };
    Ok(target.filter(|t| std::fs::symlink_metadata(t).is_ok()))
}

/// The folder a new copy goes in: beside the launcher's copy it replaces, else the user's choice,
/// else the platform default (`None`). macOS updates find their bundle's folder themselves.
fn destination(
    app: &SuiteApp,
    kind: PackageKind,
    existing: Option<&Installed>,
    previous: Option<&Record>,
    locations: &InstallLocations,
) -> Result<Option<PathBuf>, String> {
    // A program's package always installs into `Program Files\Open3D\<App>`: it takes no folder.
    if matches!(kind, PackageKind::MaxBundle | PackageKind::AppZip | PackageKind::Msi) {
        return Ok(None);
    }
    if kind == PackageKind::PortableZip && previous.is_some_and(|record| record.kind == PackageKind::PortableZip) {
        return Ok(None);
    }
    if existing.is_some() {
        return Ok(None);
    }
    let Some(folder) = locations.folder(app.id) else { return Ok(None) };
    if process::in_sandbox() {
        return Err("custom install folders aren't supported on this system".into());
    }
    selected_folder(folder, true).map(Some)
}

/// The folder a new install goes inside when the person hasn't chosen one, so the UI can show
/// that folder instead of only saying "System default". The answer follows the package: a
/// Windows MSI installs for everyone into `Program Files\Open3D\<App>`, and a 32-bit package
/// into the 32-bit Program Files, so `arch` is the package's architecture when the release names
/// one and `None` (the release is unknown) answers nothing. A portable ZIP unpacks into the
/// launcher's own `apps` folder. A macOS app goes wherever the installer would put it
/// (`/Applications`, or `~/Applications` when the system folder is not writable). Formats the
/// system itself manages (the distro's packages on Linux, Flatpak) have no single folder, so
/// they answer `None`.
pub fn default_folder(kind: PackageKind, arch: Option<Arch>, paths: &Paths) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        match kind {
            PackageKind::Msi => match arch {
                Some(Arch::X86) => {
                    Some(PathBuf::from(std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles"))?).join("Open3D"))
                }
                Some(_) => crate::msi::suite_folder(),
                None => None,
            },
            PackageKind::PortableZip => Some(paths.apps()),
            PackageKind::MaxBundle => crate::max_bundle::plugins_dir(),
            PackageKind::AppZip => crate::standalone::Places::system().map(|places| places.apps),
        }
    }
}

fn selected_folder(folder: &str, writable: bool) -> Result<PathBuf, String> {
    let path = if folder == "~" {
        dirs::home_dir().ok_or("no home folder")?
    } else if let Some(relative) = folder.strip_prefix("~/") {
        dirs::home_dir().ok_or("no home folder")?.join(relative)
    } else {
        PathBuf::from(folder)
    };
    let invalid = |reason: &str| format!("couldn't use install folder {}: {reason}", path.display());
    if folder.chars().any(|c| c.is_control() || (cfg!(windows) && c == '"')) {
        return Err(invalid("the path contains invalid characters"));
    }
    if !path.is_absolute() {
        return Err(invalid("choose an absolute path"));
    }
    if !path.is_dir() {
        return Err(invalid("the folder is unavailable. Create it or reconnect its drive first"));
    }
    if writable {
        write_probe(&path).map_err(|e| invalid(&e.to_string()))?;
    }
    Ok(path)
}

/// Creates and removes a scratch file in `dir`, the way a real write would be refused.
fn write_probe(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(format!(
        ".open3d-launcher-write-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
    ));
    drop(std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?);
    std::fs::remove_file(&probe)
}

/// The folder a new macOS app goes inside: `system` (`/Applications`) when this account can
/// create files there, else `Applications` in `home`. It creates nothing, so the install-folder
/// hint can ask on a redraw: `write_probe` takes a fresh name with `create_new`, so a link
/// planted in `/Applications` is never followed and no existing file is touched. The installer
/// creates `~/Applications` itself when it gets that answer.
#[cfg(any(target_os = "macos", test))]
fn mac_applications_dir(system: &Path, home: Option<&Path>) -> Option<PathBuf> {
    if write_probe(system).is_ok() {
        return Some(system.to_path_buf());
    }
    home.map(|home| home.join("Applications"))
}

/// Where `asset` is downloaded to (and its partial file kept while unfinished).
pub(crate) fn download_path(paths: &Paths, asset: &Asset) -> PathBuf {
    paths.downloads().join(&asset.name)
}

/// The package format to install `app` with: the format of the copy that's there (an update
/// stays a `.deb` update), else the user's choice, else the best this system supports.
///
/// Returns an empty `Vec` when `app` is already installed outside the launcher (such as an AUR
/// or repository Arch package) and must not be updated or replaced by the launcher.
pub(crate) fn choose_kinds(app: &SuiteApp, existing: Option<&Installed>, record: Option<&Record>, preferred: &[PackageKind]) -> Vec<PackageKind> {
    choose_kinds_on(app, existing, record, preferred, &SystemKinds::now())
}

/// What this system can install: its package formats ([`available_kinds`]) and whether pacman can
/// take the release's own Arch package. Asking can run a program on the host (inside the Flatpak
/// sandbox), so a question asked for every app on every check keeps one for a while.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SystemKinds {
    pub available: Vec<PackageKind>,
    pub can_pacman: bool,
}

impl SystemKinds {
    /// Asks this system now.
    pub(crate) fn now() -> Self {
        let available = available_kinds();
        Self { available, can_pacman: false }
    }
}

/// [`choose_kinds`] on what `system` can install, without asking it again.
pub(crate) fn choose_kinds_on(
    app: &SuiteApp,
    existing: Option<&Installed>,
    record: Option<&Record>,
    preferred: &[PackageKind],
    system: &SystemKinds,
) -> Vec<PackageKind> {
    choose_kinds_with(app, existing, record, preferred, &system.available, system.can_pacman)
}

pub(crate) fn choose_kinds_with(
    app: &SuiteApp,
    existing: Option<&Installed>,
    record: Option<&Record>,
    preferred: &[PackageKind],
    available: &[PackageKind],
    _can_pacman: bool,
) -> Vec<PackageKind> {
    // An app is installed only from its own kind of package: a plug-in never from a program's zip.
    let available: Vec<PackageKind> = available.iter().copied().filter(|k| app.kinds().contains(k)).collect();
    let available = available.as_slice();
    if let Some(inst) = existing {
        let kind = match inst.source {
            // A program's zip copy is updated from its package where the release has one, which
            // then removes that copy (D-156).
            InstallSource::Launcher => record.map(|r| r.kind).filter(|kind| *kind != PackageKind::AppZip),
            InstallSource::Bundle => Some(PackageKind::MaxBundle),
            InstallSource::AppFolder => None,
            // Never two copies: a copy its package installed is updated only by a package, and a
            // release without one has nothing to install over it.
            InstallSource::Msi => return available.iter().copied().filter(|kind| *kind == PackageKind::Msi).collect(),
        };
        if let Some(k) = kind.filter(|k| available.contains(k)) {
            return vec![k];
        }
    }
    // A program takes its package first, whatever an old setting prefers.
    let preferred = if app.is_standalone() { &[][..] } else { preferred };
    let mut kinds: Vec<PackageKind> = preferred.iter().copied().filter(|k| available.contains(k)).collect();
    for k in available {
        if !kinds.contains(k) {
            kinds.push(*k);
        }
    }
    kinds
}

/// Package formats this machine can install, best first.
pub(crate) fn available_kinds() -> Vec<PackageKind> {
    available_kinds_for(crate::paths::portable_root().is_some())
}

/// [`available_kinds`] for a portable launcher or an installed one: a portable launcher installs
/// programs from their zips, inside its own folder, and never asks for permission (D-153).
pub(crate) fn available_kinds_for(portable: bool) -> Vec<PackageKind> {
    if portable { vec![PackageKind::MaxBundle, PackageKind::AppZip] } else { vec![PackageKind::MaxBundle, PackageKind::Msi, PackageKind::AppZip] }
}

/// Fetches `asset` into the download folder, checked against its published SHA-256: the feed's
/// or GitHub's per-file digest, else the release's `SHA256SUMS.txt`. A release with neither is
/// refused before anything is fetched: a file nothing vouches for is never installed (#90).
/// Returns the file and the checksum it was checked against.
pub(crate) fn download(release: &Release, asset: &Asset, job: &mut Job<'_>) -> Result<(PathBuf, String), String> {
    if !release::valid_file_name(&asset.name) {
        return Err("the release has an unexpected file name".into());
    }
    let sha = match &asset.sha256 {
        Some(s) => Some(s.clone()),
        None => published_checksum(release, asset, job.http)?,
    };
    let Some(sha) = sha else {
        return Err(format!(
            "the release publishes no checksum for {}, so it can't be verified and wasn't installed. Try again later, or report it on GitHub.",
            asset.name
        ));
    };
    let dest = download_path(job.paths, asset);
    let phase = &mut *job.phase;
    job.http.download(&asset.url, &dest, asset.size, &sha, job.stop, &mut |t| match t {
        Transfer::Bytes { done, total } => phase(JobPhase::Downloading { done, total }),
        Transfer::Reconnecting { done, total } => phase(JobPhase::Reconnecting { done, total }),
    })?;
    (job.phase)(JobPhase::Verifying);
    Ok((dest, sha))
}

/// The file's line in the release's `SHA256SUMS.txt`. `Ok(None)` only when the release really
/// publishes no checksum for it; a list that exists but can't be fetched is an error, so a
/// network hiccup never downgrades the check.
fn published_checksum(release: &Release, asset: &Asset, http: &Http) -> Result<Option<String>, String> {
    let Some(sums) = release.sums_asset() else { return Ok(None) };
    let Some(text) = http.get_text(&sums.url).map_err(|e| format!("couldn't fetch the release's checksums ({e}). Try again"))? else {
        return Ok(None);
    };
    Ok(release::parse_sha256sums(&text).into_iter().find(|(_, n)| *n == asset.name).map(|(h, _)| h))
}

/// Removes an installed copy. `phase` hears when the removal has to wait for another install;
/// `stop` cancels it while it waits.
pub(crate) fn uninstall(
    app: &SuiteApp,
    installed: &Installed,
    record: Option<&Record>,
    paths: &Paths,
    stop: &Stop,
    phase: &mut dyn FnMut(JobPhase),
) -> Result<Note, String> {
    let path = PathBuf::from(&installed.location);
    // A standalone app is found by its own program, whoever installed it, and is never removed
    // while it is open (D-157).
    let remove_program = || crate::standalone::uninstall(app, &path).map(crate::standalone::still_running_note);
    if app.is_standalone() {
        program_closed(app)?;
        if installed.source == InstallSource::Msi {
            // Its package removes it, for everyone; Windows asks for permission.
            let exes = program_exes(app);
            let closed = || closed_now(app, &exes);
            let log = paths.logs().join(format!("{}-uninstall.log", app.id));
            let run = crate::msi::Run { msiexec: &crate::msi::SystemMsiexec, turns: &crate::msi::INSTALLER, stop, phase, log, closed: &closed };
            return crate::msi::uninstall(app, &crate::msi::WindowsRegistry, run).map(|()| None);
        }
        return remove_program();
    }
    if matches!(installed.source, InstallSource::Launcher) && process::is_running(&path) {
        return Err(format!("{} is running. Quit it, then try again.", app.name));
    }
    let remove_bundle = || crate::max_bundle::uninstall(app, &path).map(crate::max_bundle::still_running_note);
    match installed.source {
        #[cfg(windows)]
        InstallSource::Launcher if record.is_some_and(|r| r.kind == PackageKind::PortableZip) => {
            crate::portable::remove(app, record.ok_or("the launcher has no record of these files")?).map(|()| None)
        }
        InstallSource::Bundle => remove_bundle(),
        InstallSource::AppFolder => remove_program(),
        InstallSource::Launcher if record.is_some_and(|r| r.kind == PackageKind::MaxBundle) => remove_bundle(),
        #[cfg(windows)]
        // A tarball the launcher unpacked into its own folder: take the tree out and leave the
        // app's settings (`home/`) alone, so a reinstall finds its projects where they were.
        other => Err(format!("{} can't be removed from here", other.label())),
    }
}

/// Starts an installed app with `args` (the user's launch flags) on its command line.
pub(crate) fn launch(app: &SuiteApp, installed: &Installed, paths: &Paths, args: &[String]) -> Result<(), String> {
    let _ = paths;
    let path = Path::new(&installed.location);
    // Every app is told where the shared version-choice file is (see `craft_update::pins`), so
    // it finds it even when its own data folder points elsewhere: a redirected home, a sandbox,
    // or a launcher whose data folder moved.
    let pins = craft_update::pins::Pins::default_path();
    let mut env: Vec<(&str, &std::ffi::OsStr)> = pins.iter().map(|p| (craft_update::pins::PATH_ENV, p.as_os_str())).collect();
    // A portable launcher keeps the language every Open3D app shares in its own folder, and gives a
    // program opened from it both its folders inside that one (D-049): the program shows that
    // language and keeps its own files in the portable folder too.
    let portable = crate::paths::portable_variables();
    env.extend(portable.iter().map(|(variable, dir)| (*variable, dir.as_os_str())));
    if installed.source == InstallSource::Bundle || path.join("PackageContents.xml").is_file() {
        return crate::max_bundle::launch(app).map_err(|e| format!("Couldn't open {}: {e}", app.name));
    }
    match installed.source {
        // `open` starts the app through Launch Services, which gives it the user's real home, so
        // it finds the file at its default path; not every macOS `open` takes an environment.
        // A tarball the launcher unpacked into its own folder: run it here, against its own
        // home, rather than on the host through `flatpak-spawn` (which is what `launch_binary`
        // would do inside the sandbox). Its settings then land beside it instead of in the
        // launcher's or the host's home folder.
        _ if cfg!(target_os = "macos") && path.extension().is_some_and(|e| e == "app") => process::open_app(path, args),
        _ => {
            if !path.exists() {
                return Err(format!("{} isn't where it was installed ({})", app.name, path.display()));
            }
            process::launch_binary_with_env(path, args, &env)
        }
    }
    .map_err(|e| format!("Couldn't open {}: {e}", app.name))
}

#[cfg(windows)]
pub(crate) mod windows {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 32-bit package installs under the 32-bit Program Files; an unknown architecture answers
    /// nothing, so the UI says "System default" instead of guessing the destination.
    #[cfg(windows)]
    #[test]
    fn a_32_bit_package_uses_the_32_bit_program_files_and_an_unknown_architecture_answers_nothing() {
        let paths = Paths::under(std::path::Path::new(r"C:\scratch"));
        let x86 = std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles")).map(PathBuf::from).unwrap();
        assert_eq!(default_folder(PackageKind::Msi, Some(Arch::X86), &paths), Some(x86.join("Open3D")));
        assert_eq!(default_folder(PackageKind::Msi, None, &paths), None);
    }

    /// The macOS default-folder answer is asked on a redraw, so it must leave the disk as it was:
    /// nothing left in a writable `/Applications`, no `~/Applications` created when it is not.
    #[test]
    fn the_mac_default_folder_lookup_creates_nothing() {
        let system = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        assert_eq!(mac_applications_dir(system.path(), Some(home.path())), Some(system.path().to_path_buf()));
        assert_eq!(std::fs::read_dir(system.path()).unwrap().count(), 0, "the write probe was left behind");

        let unwritable = system.path().join("missing");
        assert_eq!(mac_applications_dir(&unwritable, Some(home.path())), Some(home.path().join("Applications")));
        assert!(!home.path().join("Applications").exists(), "a folder hint must not create ~/Applications");
        assert_eq!(mac_applications_dir(&unwritable, None), None);
    }

    /// A sample standalone app with a neutral name, beside the catalog's plug-ins.
    fn program() -> SuiteApp {
        let plugin = *SuiteApp::by_id("opentools").expect("OpenTools is in the catalog");
        SuiteApp { id: "phototool", name: "PhotoTool", package: open3d_launcher_core::AppPackage::Standalone, ..plugin }
    }

    fn found(source: InstallSource) -> Installed {
        Installed { version: None, source, location: String::new(), package: None, externally_managed: false, origin: None, installation: None }
    }

    #[test]
    fn each_app_is_installed_only_from_its_own_kind_of_package() {
        let plugin = *SuiteApp::by_id("opentools").expect("OpenTools is in the catalog");
        let system = SystemKinds { available: available_kinds_for(false), can_pacman: false };
        assert_eq!(available_kinds(), system.available, "the tests run an installed launcher");
        assert_eq!(system.available, [PackageKind::MaxBundle, PackageKind::Msi, PackageKind::AppZip]);
        assert_eq!(choose_kinds_on(&plugin, None, None, &[], &system), [PackageKind::MaxBundle]);
        // A program: its package where the release has one, else its zip (D-047).
        let both = [PackageKind::Msi, PackageKind::AppZip];
        assert_eq!(choose_kinds_on(&program(), None, None, &[], &system), both);
        // A saved preference for another format changes nothing.
        assert_eq!(choose_kinds_on(&plugin, None, None, &[PackageKind::AppZip], &system), [PackageKind::MaxBundle]);
        assert_eq!(choose_kinds_on(&program(), None, None, &[PackageKind::AppZip, PackageKind::MaxBundle], &system), both);
        // A copy a zip put in the user's folder, by hand or by the launcher, moves to the package
        // when the release has one (D-156).
        assert_eq!(choose_kinds_on(&program(), Some(&found(InstallSource::AppFolder)), None, &[], &system), both);
        let zip_record = Record {
            version: "0.1.0".into(),
            kind: PackageKind::AppZip,
            path: PathBuf::new(),
            installed_at: 0,
            files: Vec::new(),
            needs_root: false,
        };
        assert_eq!(choose_kinds_on(&program(), Some(&found(InstallSource::Launcher)), Some(&zip_record), &[], &system), both);
        assert_eq!(choose_kinds_on(&plugin, Some(&found(InstallSource::Bundle)), None, &[], &system), [PackageKind::MaxBundle]);
        // Never two copies: what its package installed is updated by a package or not at all.
        assert_eq!(choose_kinds_on(&program(), Some(&found(InstallSource::Msi)), None, &[], &system), [PackageKind::Msi]);
    }

    /// A portable launcher installs programs from their zips only (D-153).
    #[test]
    fn a_portable_launcher_installs_programs_from_their_zips() {
        let system = SystemKinds { available: available_kinds_for(true), can_pacman: false };
        assert_eq!(system.available, [PackageKind::MaxBundle, PackageKind::AppZip]);
        assert_eq!(choose_kinds_on(&program(), None, None, &[], &system), [PackageKind::AppZip]);
        assert_eq!(choose_kinds_on(&program(), Some(&found(InstallSource::AppFolder)), None, &[PackageKind::Msi], &system), [PackageKind::AppZip]);
        assert!(choose_kinds_on(&program(), Some(&found(InstallSource::Msi)), None, &[], &system).is_empty());
    }

    /// The package the backend picks from a release: the MSI when the release has one, else the zip;
    /// for a copy its package installed, nothing from a release without one.
    #[test]
    fn a_release_gives_its_package_then_its_zip() {
        let platform = open3d_launcher_core::Platform { os: open3d_launcher_core::Os::Windows, arch: Arch::X64 };
        let release = |names: &[&str]| Release {
            assets: names.iter().map(|n| Asset { name: (*n).into(), ..Asset::default() }).collect(),
            ..Release::default()
        };
        let both = release(&["phototool-0.5.0-windows-x64.zip", "phototool-0.5.0-windows-x64.msi"]);
        let zip_only = release(&["phototool-0.5.0-windows-x64.zip"]);
        let system = SystemKinds { available: available_kinds_for(false), can_pacman: false };
        let picked = |release: &Release, existing: Option<&Installed>| {
            let kinds = choose_kinds_on(&program(), existing, None, &[], &system);
            open3d_launcher_core::platform::pick_asset(release, &["phototool"], platform, &kinds).map(|(asset, kind)| (asset.name.clone(), kind))
        };
        assert_eq!(picked(&both, None), Some(("phototool-0.5.0-windows-x64.msi".into(), PackageKind::Msi)));
        assert_eq!(picked(&zip_only, None), Some(("phototool-0.5.0-windows-x64.zip".into(), PackageKind::AppZip)));
        assert_eq!(picked(&both, Some(&found(InstallSource::AppFolder))).map(|(_, kind)| kind), Some(PackageKind::Msi));
        assert_eq!(picked(&zip_only, Some(&found(InstallSource::Msi))), None);
    }

    /// D-156: once the package has installed, the zip's copy in the user's folder goes with its
    /// shortcuts, and nothing else does.
    #[test]
    fn the_old_copy_goes_with_its_shortcuts_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let places = crate::standalone::Places::under(&tmp.path().join("home"));
        assert_eq!(remove_old_copy(&program(), &places), None, "no copy, nothing to say");
        // The zip's copy, its shortcuts, and beside them another program and the settings and data.
        let dir = places.apps.join("PhotoTool");
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("PhotoTool.exe"), b"0.4.0").unwrap();
        std::fs::write(dir.join("docs").join("notes.txt"), b"notes").unwrap();
        let other = places.apps.join("FilmTool");
        std::fs::create_dir_all(&other).unwrap();
        let settings = tmp.path().join("home").join("Roaming").join("Open3D").join("PhotoTool");
        let data = tmp.path().join("home").join("Local").join("Open3D").join("PhotoTool");
        for folder in [&settings, &data] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("settings.json"), b"{}").unwrap();
        }
        for folder in &places.shortcuts {
            std::fs::create_dir_all(folder).unwrap();
        }
        let link = places.shortcuts[0].join("PhotoTool.lnk");
        crate::standalone::testing::shortcut(&link, &dir.join("PhotoTool.exe"));
        // Held as a running program holds its exe: it stays, and the person is told.
        {
            use std::os::windows::fs::OpenOptionsExt;
            let _held = std::fs::OpenOptions::new().read(true).share_mode(1).open(dir.join("PhotoTool.exe")).unwrap();
            assert_eq!(remove_old_copy(&program(), &places), Some(OLD_COPY_KEPT));
            assert!(dir.join("PhotoTool.exe").is_file() && link.is_file());
        }
        assert_eq!(remove_old_copy(&program(), &places), Some(OLD_COPY_REMOVED));
        assert!(!dir.exists() && !link.exists());
        assert!(other.is_dir() && settings.join("settings.json").is_file() && data.join("settings.json").is_file());
        assert_eq!(remove_old_copy(&program(), &places), None, "it is gone");
    }

    #[test]
    fn single_top_folder_rejects_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real");
        std::fs::create_dir_all(&real_dir).unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
    }
}
