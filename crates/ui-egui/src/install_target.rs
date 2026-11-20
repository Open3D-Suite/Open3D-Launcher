// SPDX-License-Identifier: GPL-3.0-or-later
//! Which package an install would use: the app's own kind, a 3ds Max bundle, or a program's
//! Windows Installer package before its zip (`native::install::choose_kinds`, then `pick_asset`),
//! so the UI can tell whether a release has something to install.

use open3d_launcher_core::platform::pick_asset;
use open3d_launcher_core::{Asset, InstallSource, Installed, Os, PackageKind, Release, SuiteApp};

use crate::app::LauncherUi;
use crate::model::AppEntry;

/// The formats a fresh install of `app` can use on `os`, best first: its own kind of package. A
/// launcher that installs no Windows Installer packages (a portable one) takes a program's zip.
fn platform_kinds(app: &SuiteApp, os: Os, windows_installer: bool) -> Vec<PackageKind> {
    match os {
        Os::Windows => app.kinds().iter().copied().filter(|kind| windows_installer || *kind != PackageKind::Msi).collect(),
    }
}

/// The order a fresh install of `app` tries formats in: `preferred` first, then the rest. A program
/// takes its package first, whatever an old setting prefers.
fn fresh_kinds(app: &SuiteApp, os: Os, preferred: &[PackageKind], windows_installer: bool) -> Vec<PackageKind> {
    let available = platform_kinds(app, os, windows_installer);
    let preferred = if app.is_standalone() { &[][..] } else { preferred };
    let mut kinds: Vec<PackageKind> = preferred.iter().copied().filter(|kind| available.contains(kind)).collect();
    for kind in available {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// The format the copy that's there must be updated with, when only one will do: a 3ds Max bundle
/// with a bundle, and a copy a program's package installed with a package. A program's zip copy
/// moves to the package where the release has one (D-156), so it is `None`, as for a fresh install.
fn installed_kind(app: &SuiteApp, os: Os, source: InstallSource) -> Option<PackageKind> {
    match (os, source) {
        (Os::Windows, InstallSource::Launcher) => (!app.is_standalone()).then_some(PackageKind::MaxBundle),
        (Os::Windows, InstallSource::Bundle) => Some(PackageKind::MaxBundle),
        (Os::Windows, InstallSource::AppFolder) => None,
        (Os::Windows, InstallSource::Msi) => Some(PackageKind::Msi),
    }
}

/// The formats an install or update of `app` tries, best first, as `native::install::choose_kinds`
/// orders them: the installed copy's format alone when only that one will do, else
/// [`fresh_kinds`].
fn install_kinds(app: &SuiteApp, os: Os, installed: Option<&Installed>, preferred: &[PackageKind], windows_installer: bool) -> Vec<PackageKind> {
    match installed.and_then(|inst| installed_kind(app, os, inst.source)) {
        Some(kind) => platform_kinds(app, os, windows_installer).into_iter().filter(|k| *k == kind).collect(),
        None => fresh_kinds(app, os, preferred, windows_installer),
    }
}

impl LauncherUi {
    /// The asset of `release` that installing or updating `entry` here would download. `None`
    /// when none would be, or the UI can't tell which (see [`install_kinds`]).
    pub(crate) fn install_asset<'r>(&self, entry: &AppEntry, release: &'r Release) -> Option<&'r Asset> {
        let platform = self.platform?;
        let kinds = install_kinds(entry.app, platform.os, entry.installed.as_ref(), &self.settings.preferred_kinds, self.caps.windows_installer);
        let stems: Vec<&str> = entry.app.stems().collect();
        pick_asset(release, &stems, platform, &kinds).map(|(asset, _)| asset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(source: InstallSource) -> Installed {
        Installed { version: None, source, location: String::new(), package: None, externally_managed: false, origin: None, installation: None }
    }

    #[test]
    fn a_plug_in_and_a_standalone_app_each_install_from_their_own_package() {
        let plugin = open3d_launcher_core::SUITE[0];
        let program = SuiteApp { id: "phototool", name: "PhotoTool", package: open3d_launcher_core::AppPackage::Standalone, ..plugin };
        let both = [PackageKind::Msi, PackageKind::AppZip];
        assert_eq!(install_kinds(&plugin, Os::Windows, None, &[], true), [PackageKind::MaxBundle]);
        assert_eq!(install_kinds(&program, Os::Windows, None, &[], true), both);
        // A preference saved for another format changes nothing.
        assert_eq!(install_kinds(&program, Os::Windows, None, &[PackageKind::AppZip, PackageKind::MaxBundle], true), both);
        // A zip's copy moves to the package where the release has one; a package's stays with it.
        assert_eq!(install_kinds(&program, Os::Windows, Some(&installed(InstallSource::Launcher)), &[], true), both);
        assert_eq!(install_kinds(&program, Os::Windows, Some(&installed(InstallSource::AppFolder)), &[], true), both);
        assert_eq!(install_kinds(&program, Os::Windows, Some(&installed(InstallSource::Msi)), &[], true), [PackageKind::Msi]);
        assert_eq!(install_kinds(&plugin, Os::Windows, Some(&installed(InstallSource::Bundle)), &[], true), [PackageKind::MaxBundle]);
        assert_eq!(install_kinds(&plugin, Os::Windows, Some(&installed(InstallSource::Launcher)), &[], true), [PackageKind::MaxBundle]);
        // A portable launcher takes the zip.
        assert_eq!(install_kinds(&program, Os::Windows, None, &[], false), [PackageKind::AppZip]);
        assert_eq!(install_kinds(&program, Os::Windows, Some(&installed(InstallSource::AppFolder)), &[], false), [PackageKind::AppZip]);
        assert!(install_kinds(&program, Os::Windows, Some(&installed(InstallSource::Msi)), &[], false).is_empty());
    }
}
