# SPDX-License-Identifier: GPL-3.0-or-later
"""Build the signed Open3D launcher catalog and the launcher itself, ready for the owner to publish.

Each app is packaged and released from its own repository, with `python -I tools/repo.py package` and
`python -I tools/repo.py release X.Y.Z` there: this script keeps only what is the launcher's own.

It never pushes and never publishes a release. What it commits and tags stays in this computer until the owner
pushes it; every GitHub release it makes is a draft, which the owner publishes; a release that is already
published is never changed. Where `origin` lacks a commit or a tag that the next step needs, it stops and prints
the `git push` to run. Run again once `origin` has it (`git ls-remote` tells), it goes on from there.

    python -I tools\\publish.py catalog [--dry-run]
        Signs `crates/core/catalog.toml` as `launcher-catalog.toml` + `.sig` (Ed25519, the key in
        `%USERPROFILE%\\.open3d\\launcher-catalog-signing-key.pem`, never in git), copies the app icons into
        `launcher-app-icons/` and commits. Installed launchers read them from `origin`'s `main`: while that
        lacks the commit, it prints `git push origin main`. Needed only when the list of apps changes.
    python -I tools\\publish.py launcher [--dry-run]
        Builds the launcher's MSI and portable zip with `packaging/windows/package.ps1` (version from
        Cargo.toml; needs WiX 5 on PATH), writes `SHA256SUMS.txt` and tags the committed HEAD with the
        annotated tag `vX.Y.Z`. While `origin` lacks that commit on `main` or that tag, it stops there and
        prints `git push origin main` and `git push origin vX.Y.Z`. Once it has both, it makes the draft of
        the GitHub release with that version's CHANGELOG section and the three files. Installed launchers
        update themselves from it once it is published.
"""

import argparse
import collections
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

LAUNCHER = Path(__file__).resolve().parent.parent
DIST = LAUNCHER / "dist"
CATALOG_SOURCE = LAUNCHER / "crates" / "core" / "catalog.toml"
CATALOG = LAUNCHER / "launcher-catalog.toml"
ICONS = LAUNCHER / "launcher-app-icons"
APP_ICONS = LAUNCHER / "assets" / "app-icons"
PUBLIC_KEY = LAUNCHER / "crates" / "core" / "assets" / "launcher-catalog-public-key.pem"
SIGNING_KEY = Path(os.path.expandvars(r"%USERPROFILE%\.open3d\launcher-catalog-signing-key.pem"))
REMOTE = "origin"
BRANCH = "main"
LAUNCHER_REPO = "Open3D-Suite/Open3D"

# A tag as a checkout has it. `kind` is "tag" only for an annotated tag; a lightweight one is the commit itself.
Tag = collections.namedtuple("Tag", "object kind commit")


class PublishError(Exception):
    pass


def refuse_unsafe(cmd):
    """Stops what this script must never run: a push, a deleted or moved tag, a release that is not a draft."""
    words = [str(w) for w in cmd]
    program = Path(words[0]).stem.lower()
    unsafe = False
    if program == "git":
        unsafe = "push" in words or ("tag" in words and not {"-d", "--delete", "-f", "--force"}.isdisjoint(words))
    elif program == "gh" and "api" in words:
        # Only ever a question: `gh api <path>` reads, and every option but --jq could make it write.
        unsafe = any(w.startswith("-") and w != "--jq" for w in words[1:])
    elif program == "gh" and "release" in words:
        unsafe = not {"edit", "delete", "delete-asset"}.isdisjoint(words) or (
            "create" in words and ("--draft" not in words or "--latest" in words)
        )
    if unsafe:
        raise PublishError("Refused, this script never runs it: %s" % " ".join(words))


def call(cmd, cwd=None, env=None, capture=True):
    """Runs a command and returns its result, whatever its exit code. Every command of this script goes through here."""
    refuse_unsafe(cmd)
    # The commands are built by this script from fixed program names, never from outside input.
    return subprocess.run(cmd, cwd=cwd, env=env, capture_output=capture, text=True)  # noqa: S603


def run(cmd, cwd=None):
    result = call(cmd, cwd=cwd)
    if result.returncode != 0:
        raise PublishError("Command failed: %s\n%s" % (" ".join(map(str, cmd)), (result.stderr or "").strip()))
    return result.stdout


def find_tool(local, name):
    path = os.path.expandvars(local)
    if os.path.isfile(path):
        return path
    found = shutil.which(name)
    if not found:
        raise PublishError("%s not found" % name)
    return found


def gh():
    return find_tool(r"%LOCALAPPDATA%\Programs\GitHubCLI\bin\gh.exe", "gh")


def openssl():
    return find_tool(r"%ProgramFiles%\Git\usr\bin\openssl.exe", "openssl")


def changelog_section(path, version):
    """The body of the '## <version> ...' section, without its heading."""
    out, inside = [], False
    head = re.compile(r"^## %s\b" % re.escape(version))
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            if inside:
                break
            inside = bool(head.match(line))
            continue
        if inside:
            out.append(line)
    text = "\n".join(out).strip()
    if not text:
        raise PublishError("No '## %s' section in %s" % (version, path.name))
    return text


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_tag(line):
    """A `git for-each-ref` line `<object> <type> <peeled object>` as a Tag, or None for no line."""
    fields = line.split()
    if not fields:
        return None
    return Tag(fields[0], fields[1], fields[2] if len(fields) > 2 else fields[0])


def local_tag(git, tag):
    """The tag of that checkout as a Tag, or None when it has none. It only reads."""
    fmt = "--format=%(objectname) %(objecttype) %(*objectname)"
    return parse_tag(run(git + ["for-each-ref", fmt, "refs/tags/" + tag]))


def parse_refs(text):
    """The output of `git ls-remote` as {ref: object}."""
    refs = {}
    for line in text.splitlines():
        obj, _, ref = line.partition("\t")
        if ref:
            refs[ref.strip()] = obj.strip()
    return refs


def remote_refs(git, *refs):
    """What the remote has under those names, as {ref: object}. It only reads."""
    return parse_refs(run(git + ["ls-remote", REMOTE, *refs]))


def on_remote_branch(git, remote, commit):
    """Whether the remote's main has the commit: it is at it, or at a later commit that this checkout knows."""
    there = remote.get("refs/heads/" + BRANCH)
    if not there or not commit:
        return False
    return there == commit or call(git + ["merge-base", "--is-ancestor", commit, there]).returncode == 0


def check_tag(tag, found, commit):
    """Stops unless the tag is an annotated tag on that commit: this script never replaces or moves a tag."""
    if found.kind != "tag":
        raise PublishError("Tag %s exists and is not an annotated tag" % tag)
    if found.commit != commit:
        raise PublishError("Tag %s exists and is on %s, not on HEAD (%s)" % (tag, found.commit[:8], commit[:8]))


def check_remote_tag(tag, found, there):
    """Stops when the remote has a tag of that name (`there`) that is not the one of this checkout (`found`)."""
    if there and found is None:
        raise PublishError("%s has a tag %s and this checkout has none: fetch it first" % (REMOTE, tag))
    if there and there != found.object:
        raise PublishError("%s has a tag %s that is not the one of this checkout" % (REMOTE, tag))


def pushes_needed(branch_there, tag=None, tag_there=True):
    """The pushes the next step waits for, as the commands the owner runs: the tag by its name, never `--tags`."""
    pushes = []
    if not branch_there:
        pushes.append("git push %s %s" % (REMOTE, BRANCH))
    if tag and not tag_there:
        pushes.append("git push %s %s" % (REMOTE, tag))
    return pushes


def waiting(ready, folder, pushes, then):
    """What to tell when the remote lacks what the next step needs."""
    lines = [ready, "Nothing was pushed: this script never pushes. With the owner's OK, run in %s:" % folder]
    lines += ["    " + p for p in pushes]
    return "\n".join(lines + [then])


def release_state(repo, tag):
    """The repo's release of that tag as {"isDraft": ..., "url": ...}, or None when it has none. It only reads.

    GitHub is asked first for the release of the tag, which it only gives when it is published. Only without
    one does `gh release view` look for a draft: asked alone, it could answer with a draft that shares the
    tag of a published release, and the files would then go to either."""
    result = call([gh(), "api", "repos/%s/releases/tags/%s" % (repo, tag), "--jq", ".html_url"])
    if result.returncode == 0:
        return {"isDraft": False, "url": result.stdout.strip()}
    if "HTTP 404" in (result.stderr or ""):
        result = call([gh(), "release", "view", tag, "--repo", repo, "--json", "isDraft,url"])
        if result.returncode == 0:
            return json.loads(result.stdout)
        if "release not found" in (result.stderr or ""):
            return None
    raise PublishError("Could not read the release %s of %s: %s" % (tag, repo, (result.stderr or "").strip()))


def release_step(repo, tag, state):
    """What to do with the release of a tag: "create" a draft where there is none, "replace" the files of a draft.

    A published release stops it, and so does one that doesn't say it is a draft."""
    if state is None:
        return "create"
    if state.get("isDraft") is True:
        return "replace"
    raise PublishError("The release %s of %s is published: this script never changes a published release" % (tag, repo))


def create_command(tool, repo, tag, title, notes_file):
    """A draft, of a tag the remote already has, not marked as the latest: publishing it is the owner's."""
    notes = ["--title", title, "--notes-file", str(notes_file)]
    return [tool, "release", "create", tag, "--repo", repo, "--draft", "--verify-tag"] + notes


def upload_command(tool, repo, tag, files):
    return [tool, "release", "upload", tag] + [str(f) for f in files] + ["--repo", repo, "--clobber"]


def draft_release(repo, tag, title, notes, files):
    """Leaves the files in the draft release of the tag and returns its link. A published release gets nothing."""
    state = release_state(repo, tag)
    if release_step(repo, tag, state) == "create":
        with tempfile.TemporaryDirectory(prefix="open3d-notes-") as tmp:
            notes_file = Path(tmp) / "notes.md"
            notes_file.write_text(notes + "\n", encoding="utf-8")
            url = run(create_command(gh(), repo, tag, title, notes_file)).strip()
        print("  draft release %s created in %s" % (tag, repo))
    else:
        url = state.get("url")
        print("  the release %s of %s is a draft: replacing its files" % (tag, repo))
    run(upload_command(gh(), repo, tag, files))
    return url or "https://github.com/%s/releases" % repo


def say_draft(what, repo, tag, url):
    print("  %s attached to the draft release %s of %s" % (what, tag, repo))
    print("It is a DRAFT: nobody sees it until the owner publishes it on GitHub.\n  %s" % url)


def sign(path):
    """`<path>.sig`: the base64 Ed25519 signature of the file's exact bytes."""
    if not SIGNING_KEY.is_file():
        raise PublishError("No signing key at %s" % SIGNING_KEY)
    # Git for Windows' openssl can't open UNC paths (the repo is on a NAS): sign a local copy of the bytes.
    with tempfile.TemporaryDirectory() as tmp:
        local, pub, raw = Path(tmp) / "data", Path(tmp) / "public.pem", Path(tmp) / "sig.bin"
        shutil.copyfile(path, local)
        shutil.copyfile(PUBLIC_KEY, pub)
        run([openssl(), "pkeyutl", "-sign", "-inkey", str(SIGNING_KEY), "-rawin", "-in", str(local), "-out", str(raw)])
        run(
            [
                openssl(),
                "pkeyutl",
                "-verify",
                "-pubin",
                "-inkey",
                str(pub),
                "-rawin",
                "-in",
                str(local),
                "-sigfile",
                str(raw),
            ]
        )
        sig = __import__("base64").b64encode(raw.read_bytes()).decode("ascii")
    Path(str(path) + ".sig").write_bytes((sig + "\n").encode("ascii"))


def sign_catalog():
    data = CATALOG_SOURCE.read_bytes()
    if b"\r" in data:
        raise PublishError("%s has CR line ends; the signature covers LF bytes" % CATALOG_SOURCE)
    CATALOG.write_bytes(data)
    sign(CATALOG)
    ICONS.mkdir(exist_ok=True)
    for m in re.finditer(rb'(?m)^id = "([a-z0-9-]+)"', data):
        app_id = m.group(1).decode()
        icon = APP_ICONS / ("%s.png" % app_id)
        digest = sha256(icon)
        shutil.copyfile(icon, ICONS / ("%s-%s.png" % (app_id, digest)))
        if ('icon_sha256 = "%s"' % digest).encode() not in data:
            raise PublishError("%s's icon_sha256 in catalog.toml isn't %s" % (app_id, digest))
    print("launcher-catalog.toml signed and verified; icons in %s" % ICONS.name)


def commit_catalog(dry_run=False):
    """Commits the signed catalog and its icons, and tells whether the remote's main has that commit."""
    if dry_run:
        return
    git = ["git", "-C", str(LAUNCHER)]
    signed = [CATALOG.name, CATALOG.name + ".sig", ICONS.name]
    run(git + ["add"] + signed)
    if run(git + ["diff", "--cached", "--name-only"]).strip():
        run(git + ["commit", "-m", "Catalog: sign launcher-catalog.toml"])
        print("  committed")
    else:
        print("  nothing changed")
    commit = run(git + ["log", "-1", "--format=%H", "--"] + signed).strip()
    pushes = pushes_needed(on_remote_branch(git, remote_refs(git, "refs/heads/" + BRANCH), commit))
    if not pushes:
        print("  %s's %s has it: installed launchers read it from there" % (REMOTE, BRANCH))
        return
    ready = "The signed catalog is committed here (%s) and %s's %s lacks it." % (commit[:8], REMOTE, BRANCH)
    then = "Installed launchers read the catalog from %s's %s." % (REMOTE, BRANCH)
    print(waiting(ready, LAUNCHER, pushes, then))


def publish_catalog(dry_run=False):
    sign_catalog()
    commit_catalog(dry_run)


def workspace_version():
    inside = False
    for line in (LAUNCHER / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            inside = line.strip() == "[workspace.package]"
        elif inside and (m := re.match(r'\s*version\s*=\s*"([^"]+)"', line)):
            return m.group(1)
    raise PublishError("No [workspace.package] version in Cargo.toml")


def cargo_target_dir():
    """Where cargo builds, as cargo itself resolves it (CARGO_TARGET_DIR, any `.cargo/config.toml` up the tree)."""
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=LAUNCHER))
    return meta["target_directory"]


def build_launcher():
    """The launcher's MSI, portable zip and checksums in dist/release/<version>. Returns what its release needs."""
    version = workspace_version()
    git = ["git", "-C", str(LAUNCHER)]
    if run(git + ["status", "--porcelain", "--untracked-files=no"]).strip():
        raise PublishError("Commit the launcher's changes first: the release is built from the committed HEAD")
    notes = changelog_section(LAUNCHER / "CHANGELOG.md", version)
    out = DIST / "release" / version
    env = dict(os.environ, DIST=str(out), CARGO_TARGET_DIR=cargo_target_dir())
    print("Open3D Launcher %s: building the MSI and the portable zip" % version)
    # Windows PowerShell is found through PATH on purpose: it is part of Windows.
    package = LAUNCHER / "packaging" / "windows" / "package.ps1"
    command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(package), "-Arch", "x64"]
    if call(command, env=env, capture=False).returncode != 0:
        raise PublishError("package.ps1 failed")
    assets = [
        out / ("open3d-launcher-%s-windows-x64.msi" % version),
        out / ("open3d-launcher-%s-windows-x64-portable.zip" % version),
    ]
    for a in assets:
        if not a.is_file():
            raise PublishError("package.ps1 made no %s" % a.name)
    sums = out / "SHA256SUMS.txt"
    sums.write_text("".join("%s  %s\n" % (sha256(a), a.name) for a in assets), encoding="utf-8", newline="\n")
    for a in assets:
        print("  %s: %.1f MB" % (a.name, a.stat().st_size / 1e6))
    return version, out, assets, sums, notes


def dry_run_plan(version, tag, out, files):
    """What a dry run tells: what a real run would do from here, which it finds out without running anything."""
    title = "Open3D Launcher %s" % version
    lines = ["Dry run: nothing was tagged or uploaded. The files are in %s." % out]
    lines.append("A real run would put the annotated tag %s on HEAD, unless it is there already," % tag)
    lines.append("and stop while %s lacks HEAD on %s or that tag, printing for the owner:" % (REMOTE, BRANCH))
    lines += ["    " + p for p in pushes_needed(False, tag, False)]
    lines.append("Run again once %s has both, it would build again and make the draft release %s" % (REMOTE, tag))
    lines.append('of %s, "%s", with the CHANGELOG section of %s and:' % (LAUNCHER_REPO, title, version))
    lines += ["    " + Path(f).name for f in files]
    return "\n".join(lines)


def release_launcher(version, out, assets, sums, notes, dry_run=False):
    """The annotated tag on HEAD and, once the remote has that commit and that tag, the draft release."""
    tag = "v%s" % version
    if dry_run:
        print(dry_run_plan(version, tag, out, assets + [sums]))
        return
    git = ["git", "-C", str(LAUNCHER)]
    head = run(git + ["rev-parse", "HEAD"]).strip()
    found = local_tag(git, tag)
    if found:
        check_tag(tag, found, head)
    remote = remote_refs(git, "refs/heads/" + BRANCH, "refs/tags/" + tag)
    there = remote.get("refs/tags/" + tag)
    check_remote_tag(tag, found, there)
    if not found:
        run(git + ["tag", "-a", tag, "-m", "Open3D Launcher %s" % version])
        print("  annotated tag %s created on %s" % (tag, head[:8]))
    pushes = pushes_needed(on_remote_branch(git, remote, head), tag, bool(there))
    if pushes:
        ready = "Open3D Launcher %s is built in %s and tagged %s on %s." % (version, out, tag, head[:8])
        then = "and then this same command again: it builds again and makes the draft release."
        print(waiting(ready, LAUNCHER, pushes, then))
        return
    url = draft_release(LAUNCHER_REPO, tag, "Open3D Launcher %s" % version, notes, assets + [sums])
    say_draft("%s and %s" % (", ".join(a.name for a in assets), sums.name), LAUNCHER_REPO, tag, url)


def publish_launcher(dry_run=False):
    release_launcher(*build_launcher(), dry_run=dry_run)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("catalog", help="sign the launcher catalog and commit it")
    c.add_argument("--dry-run", action="store_true", help="sign and check, and commit nothing")
    la = sub.add_parser("launcher", help="build the launcher, tag it and draft its release")
    la.add_argument("--dry-run", action="store_true", help="build the packages, and tag and upload nothing")
    args = p.parse_args(argv)
    try:
        if args.cmd == "launcher":
            publish_launcher(args.dry_run)
        else:
            publish_catalog(args.dry_run)
    except PublishError as e:
        print("error: %s" % e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
