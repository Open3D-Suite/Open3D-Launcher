# Open3D Launcher

Installs, updates and removes the Open3D apps (3ds Max plug-ins and programs of their own) and updates
itself. One Windows program: a Rust workspace with eframe/egui, forked from ArtCraft Launcher.

This file holds facts about this repo only, each one checkable in the code. Update it in the same commit
as a change that makes it old.

## Layout

- `apps/open3d-launcher/`: the program. `crates/ui-egui/`: the interface. `crates/update/`: updating itself.
- `crates/core/`: data and logic, with no input or output. `catalog.toml` is the built-in list of apps;
  `hosted_catalog.rs` checks the signed one; `platform.rs` reads the names of the packages.
- `crates/native/`: the backend. `source.rs` asks GitHub for the releases, `http.rs` downloads, `install.rs`
  installs through `max_bundle.rs` (3ds Max bundles), `msi.rs` and `standalone.rs` (programs).
- `xtask/`: `cargo xtask`. `packaging/windows/`: the launcher's own packages. `tools/publish.py`: publishing.

## Build and check

- Build: `cargo build --release -p open3d-launcher`. `.cargo/config.toml` links the Visual C++ runtime in
  (`+crt-static`: no `VCRUNTIME140.dll`); an ignored `.cargo/config.local.toml` is read if present.
- Checks: `tools\check.ps1` runs them all, and all must pass. It needs Python 3.11 or newer and the ruff
  that `ruff.toml` names. First what `tools/repo.py` has built in: the version agrees in its three places,
  every source has its licence line, the common lints, no path of somebody's computer (`tools/repo.toml`
  allows the made-up users of the tests). Then the commands that `tools/repo.toml` lists:
  - `cargo fmt --all --check`, then `ruff format --check .` and `ruff check .` (`tools/`, `dev/make_icon.py`)
  - `python -I -m unittest discover -s tools`: `tools/test_publish.py`, which runs no command
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `cargo test --workspace --locked` (4 tests are ignored: 2 need the network, 2 are started by another)
  - only with `tools\check.ps1 -Full`: the ignored test of the live signed catalog. The other network test
    reads `Open3D-Suite/wordtool`, which does not exist, and is left out.
- CI: `.github/workflows/check.yml` runs `tools\check.ps1`, without `-Full`, on every push and pull request
  to `main`, on GitHub's Windows runner with Python 3.14. `cargo xtask ci` runs fmt, clippy and the tests.
- `.editorconfig`, `.gitattributes`, `rustfmt.toml`, `clippy.toml`, `ruff.toml`, `LICENSE`, `tools/check.ps1`
  and `tools/repo.py` are the same in every Open3D repo, and `crates/ui-egui/src/open3d_settings.rs` with
  `open3d_settings_cases.txt` the same in every repo that has them. Do not change them here. This repo's own part:
  `disallowed-types` in `clippy.toml`, and in `.gitattributes` the lines below
  `# Specific to this repository.`: Git never converts the signed files.

## Version and release

- The version is in `[workspace.package]` of the root `Cargo.toml`. `python -I tools/repo.py version set
  X.Y.Z` writes it there, in `Cargo.lock` and in the README's `Status: early (vX.Y.Z)` line;
  `cargo xtask version set` writes only the first two. By hand: a `## X.Y.Z` section in `CHANGELOG.md`.
- `python -I tools\publish.py launcher --dry-run` needs that section and WiX 5: `package.ps1` builds the
  per-user MSI, whose UpgradeCode and custom actions must be those of `install-policy.json`, and the portable
  zip. A real uninstall runs it with `--uninstall-cleanup` (`uninstall_cleanup.rs`): its login value and caches go.
- Each app is packaged and released from its own repo (`tools/repo.py package` / `release`), not from here.
  `tools/publish.py` never pushes and never publishes a release. With `--dry-run` it also commits, tags and
  uploads nothing. Without it:
  - `launcher` builds, then tags HEAD with the annotated tag `vX.Y.Z`. While `git ls-remote origin` shows
    no such tag, or a `main` without that commit, it stops and prints `git push origin main` and
    `git push origin vX.Y.Z`. Run again with both there, it builds again and makes the release of
    `Open3D-Suite/Open3D-Launcher` as a draft, with the MSI, the portable zip and `SHA256SUMS.txt`.
  - `catalog` signs `crates/core/catalog.toml` as `launcher-catalog.toml` and `.sig`, commits them, and
    prints `git push origin main` while `origin`'s `main` lacks that commit.
  - A release it makes is a draft, never marked as the latest. A draft that exists gets its files replaced.
    On a release that is already published it stops and uploads nothing.
  - It stops on a tag `vX.Y.Z` of this repo that is not annotated or not on HEAD, and on a tag that `origin`
    has as another object than the checkout. It deletes and moves no tag.
  - Every command it runs goes through `call`, where `refuse_unsafe` stops a `git push`, a deleted or
    forced tag, a `gh api` with any option but `--jq`, and a `gh release` that edits, deletes, or creates
    without `--draft` or with `--latest`.

## Rules of this code

- A bug that is fixed comes with a test that fails without the fix, and new logic comes with its tests. The
  interface is tried by hand before a release.
- A 3ds Max app's `<App>.bundle` comes from `<id>-<ver>-windows-x64-3dsmax.zip` into
  `%APPDATA%\Autodesk\ApplicationPlugins`. A program comes from its per-machine `<id>-<ver>-windows-x64.msi`
  when the release has one (`msi.rs`: one `msiexec` at a time, started normally so Windows asks; found by
  its Uninstall key, in `Program Files\Open3D\<App>`; then the zip's copy goes), else from its zip (`<App>.exe`
  at its root or where the catalog's `exe` says) into `%LOCALAPPDATA%\Programs\Open3D\<App>`; a portable
  launcher always from the zip, into its `Open3DLauncherData\Programs\<App>`, with no shortcut. The bundle's
  `AppVersion` or the exe's file version must be the release's; no published SHA-256, no install.
- No plug-in is changed while a `3dsmax.exe` runs, batch and farm renders included, no program while a
  copy of it runs (`running.rs`; it can ask OpenFarm to quit). Old copies wait in `.open3d-<id>` till replaced.
- The hosted catalog counts only if its signature matches the built-in public key; the private key is not
  in the repo. The workflow `sign-catalog` (by hand, from `main`) signs the catalog of an approved pull
  request in its branch: `tools/sign_pr.py` takes what the pull request holds as data only, and the key is the
  secret `CATALOG_SIGNING_KEY` of the environment `catalog`, which only `main` may use. It can add apps, but not change the name, repo, package or exe of a built-in one. The apps
  live under `catalog::GITHUB_OWNER` (`Open3D-Suite`); a built-in app's repo, an icon or a site it names
  under `FORMER_GITHUB_OWNER` (`carlesllavata`) is read as the same under `Open3D-Suite`
  (`hosted_catalog::under_current_owner`). A new app must be `Open3D-Suite/<id>`.
- Interface texts are English in the code (`t`, `tl!`, `tn`), with their Spanish in `es.tsv`. The test for
  missing rows reads a fixed list of files; `versions.rs` and `theme.rs` are not on it.
- Its own files (`apps/open3d-launcher/src/folders.rs`, the `paths.rs` of `crates/native` and
  `crates/update`): eframe's `app.ron` in `%APPDATA%\Open3D\Launcher`, the rest (`installed.json`, logs,
  downloads, caches) in `%LOCALAPPDATA%\Open3D\Launcher`; `OPEN3D_SETTINGS_DIR` and `OPEN3D_DATA_DIR` move
  them. Right after the single-instance claim, `folders::carry_over` copies once all of
  `%APPDATA%\io.github.carlesllavata.open3d-launcher\data` and, of `%LOCALAPPDATA%\Open3D Launcher`, only
  `folders::KEPT` (`installed.json`, `.instance-token`, the self-update records); both are left as they
  were. Until then, or while a copy fails, the old folder is the one in use (`in_use`), for the update
  helper and the token too; what was not copied is said in a toast. Only debug builds read
  `OPEN3D_LAUNCHER_TEST_OLD_FOLDERS`, whose `Roaming` and `Local` stand for the old folders. A portable
  launcher keeps everything in `Open3DLauncherData` and copies nothing.
- The language is the one every Open3D app shares, in `settings.json` (`open3d_settings.rs`): in
  `%APPDATA%\Open3D`, in the folder `OPEN3D_SETTINGS_DIR` names, or, for a portable launcher, in
  `Open3DLauncherData`, which it gives as `OPEN3D_SETTINGS_DIR`, with its `Local` as `OPEN3D_DATA_DIR`, to
  the programs it opens (`paths::portable_variables`). At start `Settings::take_shared_language` puts its
  preference in `Settings.language`, or with none carries the launcher's own `en` or `es` over once
  (`language_shared`), or leaves `auto`. `auto` is the language of Windows: Spanish if it is Spanish, else
  English; `OPEN3D_LAUNCHER_LOCALE` overrides it. The picker writes the file (`LauncherUi::choose_language`).
- `unsafe` is forbidden in the workspace but for `crates/ui-egui`, where it is denied and allowed by name
  only in `open3d_settings.rs`, to ask Windows for its language. Outside tests every crate denies `unwrap`,
  `expect` and `panic`.
- It comes from ArtCraft Launcher (MIT or Apache-2.0): `NOTICE` and `LICENSES/` say what comes from where.

## Traps

- A release made by `tools/publish.py` is a draft: launchers see that version only once the draft is
  published on GitHub. Its exit code is 0 too when it stops to wait for a push.
- `publish.py launcher` builds twice, before the push and after it: the release carries the second build.
- Installed launchers read the signed catalog from this repo's `main` branch at every start.
- Launchers before 0.5.0 take a catalog entry only with its repo and icon under `carlesllavata`, and the
  signed `launcher-catalog.toml` names them there. `crates/core/catalog.toml` names `Open3D-Suite`, so
  what `publish.py catalog` signs from it, those launchers refuse whole: they keep their saved copy.
- The commit `sign-catalog` adds dismisses the approval (a rule of `main`): approve again, then merge. The push
  starts no check, so it starts `check.yml` itself.
- Adding an app: its entry in `crates/core/catalog.toml`, `assets/app-icons/<id>.png` and its line in
  `catalog::app_icon_png`, its `es.tsv` rows, and the catalog signed again. `publish.py catalog` does not
  update the copy in `crates/core/tests/fixtures/`, which a test checks.
- Only debug builds obey `OPEN3D_LAUNCHER_IGNORE_MAX`, `OPEN3D_LAUNCHER_APPS_ROOT`,
  `OPEN3D_LAUNCHER_TEST_MSIEXEC` and `OPEN3D_LAUNCHER_TEST_OLD_FOLDERS`; only `local-mirror` builds obey
  `OPEN3D_LAUNCHER_DOWNLOAD_BASE`. Comments name a `cargo xtask local serve` that does not exist.
