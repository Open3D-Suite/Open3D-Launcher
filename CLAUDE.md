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
  installs through `max_bundle.rs` (3ds Max bundles) and `standalone.rs` (programs).
- `xtask/`: `cargo xtask`. `packaging/windows/`: the launcher's own packages. `tools/publish.py`: publishing.

## Build and check

- Build: `cargo build --release -p open3d-launcher`. An ignored `.cargo/config.local.toml` is read if present.
- Checks, all of which must pass (`cargo xtask ci` runs the first three, without `--locked`):
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `cargo test --workspace --locked` (4 tests are ignored: 2 need the network, 2 are started by another)
  - `ruff format --check .` and `ruff check .`, for the Python files in `tools/` and `dev/make_icon.py`
  - `python -m unittest` from the root: `tools/test_publish.py`, which runs no command (no network, no
    git, no gh) and is found because `tools/` has an `__init__.py`
- `.editorconfig`, `.gitattributes`, `rustfmt.toml`, `clippy.toml` and `ruff.toml` are the same in every
  Open3D repo. Do not change them here. This repo's own part: `disallowed-types` in `clippy.toml`, and in
  `.gitattributes` the lines below `# Specific to this repository.`: Git never converts the signed files.

## Version and release

- The version is in `[workspace.package]` of the root `Cargo.toml`; `cargo xtask version set X.Y.Z` writes
  it. By hand: the README's `Status: early (vX.Y.Z)` line and a `## X.Y.Z` section in `CHANGELOG.md`.
- `python -I tools\publish.py launcher --dry-run` needs that section and WiX 5: `package.ps1` builds the
  per-user MSI, whose UpgradeCode must be the one in `install-policy.json`, and the portable zip.
- `publish.py app <id>` builds a 3ds Max app's zip from its latest `vX.Y.Z` tag (`tools/apps.json`);
  `publish.py catalog` signs `crates/core/catalog.toml` as `launcher-catalog.toml` and `.sig`.
- `tools/publish.py` never pushes and never publishes a release. With `--dry-run` it also commits, tags and
  uploads nothing. Without it:
  - `launcher` builds, then tags HEAD with the annotated tag `vX.Y.Z`. While `git ls-remote origin` shows
    no such tag, or a `main` without that commit, it stops and prints `git push origin main` and
    `git push origin vX.Y.Z`. Run again with both there, it builds again and makes the release of
    `carlesllavata/Open3D` as a draft, with the MSI, the portable zip and `SHA256SUMS.txt`.
  - `app <id>` stops and prints `git push origin vX.Y.Z` while the app's `origin` has no such tag. With the
    tag there, it puts the zip and `SHA256SUMS.txt` in a draft release of the app's repo.
  - `catalog` commits the signed files, and prints `git push origin main` while `origin`'s `main` lacks
    that commit.
  - A release it makes is a draft, never marked as the latest. A draft that exists gets its files replaced.
    On a release that is already published it stops and uploads nothing.
  - It stops on a tag `vX.Y.Z` of this repo that is not annotated or not on HEAD, and on a tag that `origin`
    has as another object than the checkout. It deletes and moves no tag.
  - Every command it runs goes through `call`, where `refuse_unsafe` stops a `git push`, a deleted or
    forced tag, a `gh api` with any option but `--jq`, and a `gh release` that edits, deletes, or creates
    without `--draft` or with `--latest`.

## Rules of this code

- Apps are installed from zips, per user, with no administrator rights: a 3ds Max app's `<App>.bundle`
  into `%APPDATA%\Autodesk\ApplicationPlugins`, a program into `%LOCALAPPDATA%\Programs\Open3D\<App>`.
- The zip is `<id>-<ver>-windows-x64-3dsmax.zip` with the bundle at its root, or `<id>-<ver>-windows-x64.zip`
  with `<App>.exe` at its root or where the catalog's `exe` says. `AppVersion` in `PackageContents.xml`, or
  the exe's file version, must be the release's, and a download with no published SHA-256 is refused.
- No plug-in is changed while a `3dsmax.exe` runs, batch and farm renders included; no program while its
  own exe runs. The old copy waits in `.open3d-<id>` beside it until the new one is in place.
- The hosted catalog counts only if its signature matches the built-in public key; the private key is not
  in the repo. It can add apps, but not change the name, repo, package or exe of a built-in one.
- Interface texts are English in the code (`t`, `tl!`, `tn`), with their Spanish in `es.tsv`. The test for
  missing rows reads a fixed list of files; `versions.rs` and `theme.rs` are not on it.
- `unsafe` is forbidden in the workspace; outside tests every crate denies `unwrap`, `expect` and `panic`.
- It comes from ArtCraft Launcher (MIT or Apache-2.0): `NOTICE` and `LICENSES/` say what comes from where.

## Traps

- A release made by `tools/publish.py` is a draft: launchers see that version only once the draft is
  published on GitHub. Its exit code is 0 too when it stops to wait for a push.
- `publish.py launcher` builds twice, before the push and after it: the release carries the second build.
- Installed launchers read the signed catalog from this repo's `main` branch at every start.
- Adding an app: its entry in `crates/core/catalog.toml`, `assets/app-icons/<id>.png` and its line in
  `catalog::app_icon_png`, its `es.tsv` rows, `tools/apps.json` (3ds Max apps only), and the catalog signed
  again. `publish.py catalog` does not update the copy in `crates/core/tests/fixtures/`, which a test checks.
- Only debug builds obey `OPEN3D_LAUNCHER_IGNORE_MAX` and `OPEN3D_LAUNCHER_APPS_ROOT`; only `local-mirror`
  builds obey `OPEN3D_LAUNCHER_DOWNLOAD_BASE`. Comments name a `cargo xtask local serve` that does not exist.
