# Changelog

What each version of the Open3D Launcher adds, newest first.

## 0.5.0 — 22 November 2026

- **Install this version by hand.** Open3D moved to the [Open3D-Suite](https://github.com/Open3D-Suite)
  organization on GitHub, and launchers up to 0.4.0 can no longer install or update anything, themselves
  included: GitHub now names every download under Open3D-Suite, and they only take downloads from the old
  address. Your installed apps keep working. Download the MSI from the
  [latest release](https://github.com/Open3D-Suite/Open3D-Launcher/releases/latest) and run it: it replaces
  the old launcher and keeps your settings.
- **The launcher's repository is now
  [Open3D-Suite/Open3D-Launcher](https://github.com/Open3D-Suite/Open3D-Launcher).** The launcher looks
  for every app's releases in the organization, and for its own, and its Website, GitHub, releases and help
  links point there, as do the links of its entry in Windows' installed apps.
- **An app's page and the README say which 3ds Max versions each app was tested on,** and which should work
  but are not tested.
- **The Platforms box of an app's page is in Spanish too** when the launcher is; it was always in English.

## 0.4.0 — 21 November 2026

- **One language for the launcher and the Open3D apps.** The language you choose in Settings is now the
  one the Open3D apps use too (OpenRef for now, the others as they are updated): they show it the next time
  they start. A language chosen in one of those apps shows here the next time the launcher starts. A
  language you had already chosen in the launcher is kept and passed on to them.
- **Auto follows the language Windows is shown in:** Spanish if Windows is in Spanish, English otherwise.
  Choosing Auto also makes the Open3D apps follow Windows again.
- **A portable launcher keeps that language in its own folder,** beside it, and the programs you open from
  it use it too.
- **The launcher's files move to the Open3D folders:** its settings to `%APPDATA%\Open3D\Launcher`, and the
  record of the apps it installed, its downloads and its logs to `%LOCALAPPDATA%\Open3D\Launcher`. The first
  time this version starts it copies your settings and the record of your apps from the old folders,
  `%APPDATA%\io.github.carlesllavata.open3d-launcher` and `%LOCALAPPDATA%\Open3D Launcher`, so nothing has
  to be set up again; downloads and logs start afresh. The old folders are left as they were, and an older
  launcher still finds its files there.
- **A portable launcher keeps the files of the programs it opens in its own folder too:** their settings
  in `Open3DLauncherData` and the rest in `Open3DLauncherData\Local` (OpenRef for now, the others as they
  are updated).
- **Programs install for everyone on the computer, with their Windows Installer package.** When a release
  of OpenView, OpenRef, OpenFarm or OpenGaussian publishes its `.msi`, the launcher installs, updates and
  removes the program with it, in `C:\Program Files\Open3D\<App>`, and Windows asks for permission each
  time. The packages aren't signed yet, so Windows' prompt says "Unknown publisher". Each package adds one
  shortcut to the Start menu, for everyone on the computer, and none to the desktop. Update all runs them
  one after another, each with its own prompt; the card says when Windows will ask. A cancelled prompt
  keeps the download for the next try, and an error names the installer's log in the launcher's `logs`
  folder. Releases without a package install from their zip, as before.
- **A program installed with its package goes back to an older version only once it is uninstalled.**
  Windows Installer doesn't put an older version over a newer one, so the launcher says so before it
  downloads anything: uninstall the program, then install the version you want. Nor does it put a version
  that has no package over a copy that came from one.
- **Once a program's package has installed, its earlier copy in your user folder goes,** with its
  shortcuts, and the launcher says so. Its settings and data stay where they are. If the package fails or
  you cancel it, the earlier copy stays and keeps working.
- **A portable launcher installs the programs inside its own folder,** in `Open3DLauncherData\Programs`,
  and adds no shortcut to the Start menu or the desktop, so it writes nothing outside its folder. Programs
  it installed in your user folder before stay there and keep working; it no longer lists them.
- **A program that is open is never updated or removed,** also when another account on the computer has
  it open: the launcher looks at the programs that run and says "… is open; close it and try again".
- **Uninstalling a program can delete its settings and data too.** The uninstall dialog of OpenView,
  OpenRef, OpenFarm and OpenGaussian has a box, unticked, "Also delete its settings and data": once the
  program is removed, its folders `%APPDATA%\Open3D\<App>` and `%LOCALAPPDATA%\Open3D\<App>` go (for a
  portable launcher, those inside its own folder), and nothing else. Unticked, they stay as before.
- **Close OpenFarm and update.** OpenFarm keeps running without its window, so when it is open the launcher
  offers to close it, waits for it to end and then updates it. If it doesn't close, nothing is changed.
- **Uninstalling the launcher cleans up after it.** It also takes away its "Start Open3D Launcher when I
  log in" entry, if it starts that launcher, and its caches: downloads, saved release answers and pictures.
  Its settings, the record of the apps it installed and its logs stay, and so do the apps. Updating the
  launcher changes none of this.
- **The setting "Ask for permission once when updating all apps" is gone.** It never did anything in
  Open3D: Update all updates each app on its own, and Windows asks for permission for each program that
  needs it.

## 0.3.4 — 16 November 2026

- **OpenScatter is in the launcher.** Install and update OpenScatter, a scatter for 3ds Max 2027: one
  object that places millions of render-time instances on surfaces. It is an early version, without an
  interface yet, and the first app with a compiled 3ds Max plug-in. Launchers already installed see it
  through the signed catalog; this version also lists it without it.
- **An update or removal no longer ends as failed over a file that is still loaded.** Windows won't
  delete a compiled plug-in that a running program still has loaded. If that happened while a 3ds Max
  plug-in was being updated or removed, the new version was put in place but the job was reported as
  failed, and later updates of it were refused until that program ended. Now the update or removal
  finishes, the launcher deletes what is left of the old copy later, once nothing uses it, and tells you
  that what is still running keeps what it had loaded until it restarts. It is a rare case: as before, a
  plug-in is only changed with 3ds Max closed, and that includes a 3ds Max that renders in the background
  (a batch or farm render).
- **The same for apps that are programs of their own.** Updating or removing one (OpenView, OpenGaussian,
  OpenRef, OpenFarm) no longer ends as failed when another program still has one of its files loaded: the
  new version is in place, or the app and its shortcuts are gone, and the launcher tells you that a program
  still running keeps using files of the old copy until it closes. The app itself must still be closed
  first.
- **OpenFarm is in the launcher.** Install, update and open OpenFarm, a render farm for your own computers
  (3ds Max and Blender scenes on every machine of the studio). It is a program of its own. Launchers
  already installed see it through the signed catalog; this version also lists it without it.
- **OpenRef is in the launcher.** Install, update and open OpenRef (reference-image boards that float over
  3ds Max or any other program). It is a program of its own. Launchers already installed see it through
  the signed catalog; this version also lists it without it.

## 0.3.3 — 13 November 2026

- **OpenGaussian is in the launcher.** Install, update and open OpenGaussian (train, view, edit and export
  3D Gaussian Splatting scenes from your photos) from the launcher. It is a program of its own and needs an
  NVIDIA graphics card (GTX 16 or RTX 20 series or newer).
- **Standalone apps whose program sits in a folder of their own.** An app can now keep its program in a
  subfolder of its install folder (OpenGaussian's is in `bin`): the launcher checks its version there,
  points the shortcuts at it, and removes the whole app folder when you uninstall.

## 0.3.2 — 12 November 2026

- **OpenElements is no longer in the launcher: it is now the Render Elements tool of OpenTools.**
  Install or update OpenTools (0.6.0 or later) to get it. This launcher no longer lists OpenElements, so if
  you installed it, delete the `OpenElements.bundle` folder from `%APPDATA%\Autodesk\ApplicationPlugins`
  with 3ds Max closed.

## 0.3.1 — 12 November 2026

- **OpenElements is in the launcher.** Install and update OpenElements (render only the render elements you
  choose) from the launcher, like the other 3ds Max apps.
- **Only one launcher at a time, again.** Another program on your computer could use the same local port as the
  launcher, and then a second launcher could open next to the first. The launcher now has a port of its own, and
  asks a launcher up to 0.3.0 that is still running to make way.
- **Release notes read better.** Bold text shows in bold instead of between asterisks, and a change that runs
  over several lines stays one item of the list.

## 0.3.0 — 12 November 2026

- **OpenView is in the launcher.** Install, update and open OpenView (360 panoramas and photos from map
  applications at full resolution, with a viewer, crops and HDRIs) from the launcher. It is the first app
  that is a program of its own: it doesn't need 3ds Max.
- **Standalone apps.** The launcher can now install, update, open and remove an app that is a program of
  its own rather than a 3ds Max plug-in. It goes in `Programs\Open3D\<App>` in your user folder, without
  administrator rights, with shortcuts in the Start menu and on the desktop, and **Open** starts it.
  Downloads are checked and updates put in place as for plug-ins: the version you have stays until the new
  one is ready, and comes back if the update fails. Close the app before updating or removing it; the
  launcher tells you when it is still open. Removing it keeps its settings.

## 0.2.1 — 11 November 2026

- **OpenManager is in the launcher.** Install and update OpenManager (scene setups and batch rendering)
  from the launcher, like OpenTools.
- **A warning for duplicate copies of an app.** When 3ds Max's plug-in folder holds another copy of an
  app, such as a development bundle (`OpenTools-dev.bundle` next to `OpenTools.bundle`), the app's card
  names it and reminds you that 3ds Max loads only one of them.
- **Going back to an older version works.** Choosing an older version of an app was always refused; the
  plug-in you have now stays in place until the older one is installed, so nothing is lost if it fails.
- **The launcher's own versions show in Preferences.** The list of launcher versions was empty on
  Windows.
- **A smaller, simpler launcher.** Code it inherited for other systems (macOS, Linux) and for
  install formats Open3D doesn't use is gone, along with the install-format and install-folder
  settings, which never applied to 3ds Max plug-ins.

## 0.2.0 — 11 November 2026

A new launcher, rebuilt on the open-source code of [ArtCraft Launcher](https://github.com/storytold/craft-launcher)
and adapted to 3ds Max. It doesn't update from 0.1.x on its own: remove the old launcher from Windows'
*Settings > Apps > Installed apps* and install this one. The apps you installed stay where they are and the
new launcher finds them.

- **Installer or portable, your choice.** `open3d-launcher-0.2.0-windows-x64.msi` installs the launcher for
  you only (no administrator rights) with a setup wizard, a Start menu shortcut and an optional desktop one.
  The portable zip runs from any folder and keeps its settings next to it.
- **Each app's versions come from its own GitHub repository**, OpenTools from
  [carlesllavata/OpenTools](https://github.com/carlesllavata/OpenTools). Every download is checked against the
  SHA-256 published with the release before anything is installed.
- **Install any published version** of an app, not just the latest, and **update all** your apps at once.
- **Open in 3ds Max** starts the newest 3ds Max on your computer.
- **3ds Max must be closed** to install, update or remove an app; the launcher says so instead of failing
  halfway. An interrupted update puts the previous version back.
- **A signed catalog.** The list of apps is built into the launcher and can be extended from this
  repository only with a matching signature.
- **Grid or table view, light or dark theme**, and release notes for every version.
- **Notification area and start with Windows:** closing the window keeps the launcher running in the
  notification area, where it can tell you about updates; both are optional in *Preferences*.
- **English and Spanish.** The launcher follows Windows' language; change it in *Preferences*.
- **The launcher updates itself** from the same kind of package you installed (MSI or portable). It isn't
  code-signed yet, so each update is checked against the release's published checksum.
- Free software under the GNU GPL v3 or later. ArtCraft Launcher's MIT / Apache-2.0 notices are kept in
  `NOTICE` and `LICENSES/`.

## 0.1.2 — 11 November 2026

- **New app versions now really show up within a minute.** The launcher reads the catalog through GitHub's API,
  whose cache lasts at most a minute; if that fails, it reads it the old way. The fix in 0.1.1 didn't work:
  GitHub's 5-minute cache for the catalog file ignores the trick 0.1.1 used.

## 0.1.1 — 11 November 2026

- **New app versions show up right away.** The launcher now always reads the current catalog: before, a
  version published a moment ago could take up to five minutes to appear, because GitHub keeps the catalog
  in a short-lived cache.

## 0.1.0 — 11 November 2026

The first version of the launcher: one place to install, update and remove the Open3D apps for 3ds Max.

- **Install the launcher with one click.** Run the downloaded `Open3D-Launcher-0.1.0.exe` and choose
  **Install**: it goes in your user folder (no administrator rights), with a Start menu shortcut and, if you
  want, a desktop one. **Run without installing** works too.
- **Remove it like any other program**, from Windows' *Settings > Apps > Installed apps*. The apps you
  installed with it stay in 3ds Max.
- **Scripted installs:** `--install` installs the launcher without a window, and `--uninstall --quiet` removes
  it without asking.
- **The launcher updates itself.** When a newer launcher is published, a bar at the top offers to update it;
  the new version is checked before it replaces the old one, and it restarts on its own.
- **Install, update and remove apps.** The launcher lists the Open3D apps with the version you have and the
  latest one, and installs each app as a 3ds Max plug-in bundle in `ApplicationPlugins`: per user, no
  administrator rights, and every 3ds Max from 2025 on picks it up. Removing an app keeps its settings.
- **Safe updates.** Every download is checked against the catalog before anything changes; a damaged
  download is refused and the version you had stays in place. An update interrupted halfway is put right
  the next time the launcher starts.
- **3ds Max must be closed** to install, update or remove an app; the launcher says so and waits.
- **Development copies are flagged.** If a second copy of an app is in the plug-in folder (for example
  `OpenTools-dev.bundle`), the launcher warns you, because 3ds Max loads only one of them.
- **What's new in each version** is shown next to the app, taken from its changelog.
- Free software under the GNU GPL v3 or later.

The launcher isn't code-signed yet: the first time you run it, Windows SmartScreen may show "Windows
protected your PC". Choose **More info > Run anyway**.
