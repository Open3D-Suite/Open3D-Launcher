# Open3D

Open-source tools that make 3D work faster, starting with 3ds Max.

This repository holds the **Open3D Launcher**: one place to install, update and remove the Open3D apps and
to see what's new in each version. It is a fork of [ArtCraft Launcher](https://github.com/storytold/craft-launcher)
adapted to the Open3D apps: 3ds Max plug-ins and standalone programs (see [Credits](#credits)).

## Open3D Launcher

- Lists the Open3D apps with the version you have and the latest one, with each version's release notes.
- Installs each 3ds Max app as a plug-in bundle in `%APPDATA%\Autodesk\ApplicationPlugins`: per user, no
  administrator rights, and every 3ds Max the app supports loads it (the versions are under Requirements).
  **Open in 3ds Max** starts the newest 3ds Max you have.
- From version 0.4.0, installs each program of its own (OpenView, OpenRef, OpenFarm, OpenGaussian) for
  everyone on the computer, in `C:\Program Files\Open3D\<App>`, with its Windows Installer package and one
  shortcut in the Start menu. Windows asks for permission at each install, update and removal; the packages
  aren't signed yet, so its prompt says "Unknown publisher". Once a package has installed, the copy of that
  program an older launcher left in your user folder goes, with its shortcuts; its settings and data stay.
  A release with no package is unpacked from its zip into `%LOCALAPPDATA%\Programs\Open3D\<App>`, for you
  only, with shortcuts in the Start menu and on the desktop. **Open** starts the program.
- Reads each app's versions from that app's own GitHub releases, and checks every download against its
  published SHA-256 before touching anything; a failed or interrupted update puts the previous version back.
- Never changes a plug-in while 3ds Max is open, nor a program while it is running: close it first and the
  launcher tells you so. OpenFarm keeps running without its window, so for it the launcher offers **Close
  OpenFarm and update**. A 3ds Max that renders in the background (a batch or farm render) counts as open:
  on a machine that renders, update the plug-ins between renders.
- Updates itself. It keeps running in the notification area when you close its window and can start with
  Windows (both in *Preferences*).
- English and Spanish. No account, no sign-in, no telemetry.

Requirements: Windows 10 or 11 (64-bit). For what runs inside 3ds Max:

- OpenTools: tested on 3ds Max 2027; 2025 and 2026 should work, but are not tested.
- OpenManager: tested on 3ds Max 2026.3 and 2027; 2025 should work, but is not tested.
- OpenScatter: 3ds Max 2027 only (tested).
- OpenFarm's button in 3ds Max: tested with 3ds Max 2027; 2025 and 2026 should work, but are not tested.

## Download and install

From the [latest release](https://github.com/Open3D-Suite/Open3D-Launcher/releases/latest) download one of:

- `open3d-launcher-<version>-windows-x64.msi`: the installer. It installs for you only, without
  administrator rights, in `%LOCALAPPDATA%\Programs\Open3D\Launcher`, with a Start menu shortcut and, if you
  tick it, a desktop one. Remove it from Windows' *Settings > Apps > Installed apps*: that also takes away
  its start-at-login entry and its caches, and keeps its settings.
- `open3d-launcher-<version>-windows-x64-portable.zip`: no installation. Unzip it anywhere and run
  `open3d-launcher.exe`; its settings stay in `Open3DLauncherData` next to it (see `portable.txt`). It
  installs the programs from their zips inside that folder, with no shortcut and no permission prompt.

The launcher isn't code-signed, so the first time Windows SmartScreen may say "Windows protected your PC":
choose **More info > Run anyway**. Updates are checked against the checksums published with each release.

Removing the launcher leaves the apps you installed with it where they are: remove them from the launcher
first if you want them gone. Removing a program keeps its settings and data, unless you tick *Also delete
its settings and data*; removing a 3ds Max app keeps its settings.

> Status: early (v0.5.0). What each version adds: [CHANGELOG.md](CHANGELOG.md).

### Apps

- **OpenTools**: a tool hub for 3ds Max (scene checks and repairs, asset / proxy / XRef managers, rendering
  only the render elements you choose, modeling, material and clipboard tools). [Open3D-Suite/OpenTools](https://github.com/Open3D-Suite/OpenTools)

- **OpenManager**: scene setups and batch rendering for 3ds Max (Corona and V-Ray; reads Pulze Scene Manager
  setups). [Open3D-Suite/OpenManager](https://github.com/Open3D-Suite/OpenManager)

- **OpenView**: downloads 360 panoramas and photos from map applications at full resolution, with a 360
  viewer, crops and HDRIs. A program of its own: it doesn't need 3ds Max.
  [Open3D-Suite/OpenView](https://github.com/Open3D-Suite/OpenView)

- **OpenGaussian**: trains, views, edits and exports 3D Gaussian Splatting scenes from your photos, based on
  LichtFeld Studio. A program of its own; it needs an NVIDIA graphics card (GTX 16 or RTX 20 series or newer).
  [Open3D-Suite/OpenGaussian](https://github.com/Open3D-Suite/OpenGaussian)

- **OpenRef**: reference-image boards that float over 3ds Max or any other program: paste, crop, arrange
  and annotate images, see-through or letting the mouse through. A program of its own: it doesn't need
  3ds Max. [Open3D-Suite/OpenRef](https://github.com/Open3D-Suite/OpenRef)

- **OpenFarm**: a render farm for your own computers: send 3ds Max (Corona, V-Ray, V-Ray GPU, FStorm) and
  Blender (Cycles, Eevee) scenes to every machine of the studio. A program of its own, installed on every
  computer of the farm. [Open3D-Suite/OpenFarm](https://github.com/Open3D-Suite/OpenFarm)

- **OpenScatter**: a scatter for 3ds Max 2027: one object that places millions of render-time instances of
  your models on surfaces (tested in Arnold and Corona, not yet in V-Ray). An early version, with no
  interface yet. [Open3D-Suite/OpenScatter](https://github.com/Open3D-Suite/OpenScatter)

More apps are added to the catalog as they are published.

## How it works

- **Catalog.** The list of apps is `crates/core/catalog.toml`, built into the launcher. The launcher also
  reads `launcher-catalog.toml` from this repository's `main` branch, but only when its Ed25519 signature
  (`launcher-catalog.toml.sig`) matches the public key built into the launcher
  (`crates/core/assets/launcher-catalog-public-key.pem`); otherwise it keeps the built-in list.
- **Versions.** Each app lives in its own public repository (`Open3D-Suite/<App>`), and each release tagged
  `vX.Y.Z` carries the app's package and `SHA256SUMS.txt`. The launcher asks GitHub for the latest release,
  at most once an hour unless you press *Check for updates*.
- **3ds Max apps.** The package is `<id>-<version>-windows-x64-3dsmax.zip`, with the `.bundle` folder at the
  zip's root; the bundle's `PackageContents.xml` must say the release's version.
- **Programs.** An app the catalog marks `package = "standalone"` is a program of its own (OpenView,
  OpenGaussian, OpenRef, OpenFarm). Its release carries `<id>-<version>-windows-x64.msi`, a Windows
  Installer package for everyone on the computer, and `<id>-<version>-windows-x64.zip`. The launcher
  installs, updates and removes the program with the package, which it starts like any other program so
  that Windows asks for permission, one at a time; it finds that copy by its entry in Windows' installed
  apps, and once the package has installed it removes the copy an older launcher unpacked in your user
  folder. A portable launcher, or a release with no package, unpacks the zip instead: `<App>.exe` at the
  zip's root, or where the catalog's `exe` says (`exe = "bin/OpenGaussian.exe"`, for a program that needs
  the folders around it), whose file properties must say the release's version. A portable launcher puts
  it in `Open3DLauncherData\Programs\<App>` with no shortcut; otherwise it goes in
  `%LOCALAPPDATA%\Programs\Open3D\<App>`, with Start menu and desktop shortcuts on a first install.
- **Launcher updates.** The launcher's own releases, in this repository, carry the MSI, the portable zip
  and `SHA256SUMS.txt`; an installed launcher downloads the same format it was installed from.

## Building from source

Needs [Rust](https://rustup.rs) (stable, MSVC toolchain) and the Visual Studio Build Tools with the C++
workload.

```
cargo build --release -p open3d-launcher     # target\release\open3d-launcher.exe
tools\check.ps1                               # every check; -Full adds one that needs the network
cargo xtask ci                                # only fmt --check, clippy -D warnings, tests
```

`tools\check.ps1` also needs Python 3.11 or newer and the ruff that `ruff.toml` names.

The MSI and the portable zip are made by `packaging\windows\package.ps1`, which needs
[WiX Toolset 5](https://wixtoolset.org) with its UI and Util extensions (see the script's header).

### Publishing (members of Open3D-Suite)

Each app is packaged and released from its own repository, with `python -I tools\repo.py package` and
`python -I tools\repo.py release X.Y.Z` there; its author publishes the draft.

An app enters the launcher through a pull request to this repository: its entry in
`crates\core\catalog.toml`, its icon, its Spanish texts. Another member reviews it and approves it; then
any member runs *Actions → sign-catalog → Run workflow* with the pull request's number, and a member other
than whoever started it approves the run (*Review deployments*). GitHub signs the catalog with the key
kept as a secret that only `main` may use and adds the signed files to the pull request, which is approved
again and merged. Launchers read the new catalog at their next start. Every member writes in this
repository; the last push to a pull request must be approved by someone other than whoever pushed it.
[Open3D-SDK's guide](https://github.com/Open3D-Suite/Open3D-SDK/blob/main/docs/distribution.md#entering-the-launcher)
says every step.

`tools\publish.py` (`python -I tools\publish.py ...`) releases the launcher and can sign the catalog by
hand, and needs the GitHub CLI. It never pushes and never publishes a release: where GitHub lacks a commit
or a tag it stops and prints the `git push` to run, and every release it makes is a draft. It never changes
a release that is already published.

- `catalog`: signs `crates\core\catalog.toml` as `launcher-catalog.toml` + `.sig` with the catalog's
  private key (never in this repository), copies the app icons into `launcher-app-icons\` and commits.
  Launchers read them once that commit is pushed to `main`. Only needed when the list of apps changes,
  and only by whoever holds a copy of the key: the `sign-catalog` workflow is the usual way.
- `launcher`: builds the MSI and the portable zip and tags the commit with the annotated tag `vX.Y.Z`.
  Run again once `main` and that tag are pushed, it builds again and makes a draft of this repository's
  release with the version's CHANGELOG section.

Each takes `--dry-run` to sign or build without committing, tagging or uploading anything.

`tools\check.ps1` runs the script's tests (`tools\test_publish.py`), and so does
`python -I -m unittest discover -s tools`: they check what it would send out, and run no command.

## Repository layout

| path | what it is |
|---|---|
| `apps/open3d-launcher/` | the desktop program: window, tray, single instance, logging |
| `crates/core/` | catalog, releases, versions, platforms and package formats (no I/O) |
| `crates/native/` | downloads, install / update / remove, 3ds Max bundles (`max_bundle.rs`), programs' Windows Installer packages (`msi.rs`) and zips (`standalone.rs`), self-update |
| `crates/ui-egui/` | the interface (egui) and its translations (`src/i18n/*.tsv`) |
| `crates/update/` | the self-update library (a modified copy of craft-update) |
| `packaging/windows/` | the MSI (WiX), the portable zip and their checks |
| `assets/` | the Open3D icon (made by `dev/make_icon.py`), the app icons and the fonts |
| `launcher-catalog.toml`, `.sig`, `launcher-app-icons/` | the signed catalog the launcher reads from `main` |
| `tools/` | the checks (`check.ps1`); signing the catalog and releasing the launcher (maintainers) |
| `xtask/` | `cargo xtask ci`, `version` and `ico` |

## Credits

Open3D Launcher is based on [ArtCraft Launcher](https://github.com/storytold/craft-launcher) and
[craft-update](https://github.com/storytold/craft-libs) by the ArtCraft Team and their contributors,
released under the MIT License or the Apache License 2.0. Their licence texts are in [LICENSES/](LICENSES),
and [NOTICE](NOTICE) says what comes from where. ArtCraft is a trademark of the ArtCraft Team; this project
uses none of its brand assets and isn't affiliated with or endorsed by them. Fonts and other bundled
material are listed in [ATTRIBUTION.md](ATTRIBUTION.md).

## License

Copyright (C) 2026 Carles Llavata and Open3D contributors.

The Open3D Launcher is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the License, or (at
your option) any later version. It is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
[LICENSE](LICENSE) for the full text of the GNU General Public License v3 (SPDX: `GPL-3.0-or-later`).
