# SPDX-License-Identifier: GPL-3.0-or-later
"""Signs the catalog of an approved pull request. `.github/workflows/sign-catalog.yml` runs it, from main.

    python -I tools/sign_pr.py approved NUMBER    the pull request is open, of this repository, and somebody else who
                                                  can write here approved its last commit: prints that commit and its
                                                  branch, for the steps that follow ($GITHUB_OUTPUT)
    python -I tools/sign_pr.py sign COMMIT        signs the catalog of that commit with the key in
                                                  $CATALOG_SIGNING_KEY, and prints a new commit on top of it

What the pull request holds is only ever data: this script and tools/publish.py are main's, nothing of the pull
request is run or written to the disk by its own name, and only regular files are read (a link named as an icon could
otherwise publish the key). It pushes nothing: the workflow pushes the commit it prints.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import publish  # noqa: E402 - beside this script, put on the path just above

SOURCE = "crates/core/catalog.toml"
ICONS = "assets/app-icons"
FIXTURE = "crates/core/tests/fixtures/hosted-catalog.toml"
MAX_CATALOG = 1 << 20  # what the launcher reads at most
MAX_ICON = 256 << 10
CAN_WRITE = {"admin", "maintain", "write"}
COMMIT = re.compile(r"[0-9a-f]{40}")
BRANCH = re.compile(r"[A-Za-z0-9][A-Za-z0-9._/-]{0,200}")
ICON = re.compile(r"[a-z][a-z0-9-]{0,63}\.png")


class Refused(Exception):
    """Why this pull request's catalog is not signed."""


# ---------------------------------------------------------------------------------------------- the approval


def approvers(reviews, author, commit):
    """Who approves that commit: the logins whose last verdict is an approval of it, the author left out."""
    last = {}
    for review in reviews:
        if review["state"] in ("APPROVED", "CHANGES_REQUESTED", "DISMISSED"):
            last[review["user"]["login"]] = review
    return sorted(
        login
        for login, review in last.items()
        if review["state"] == "APPROVED" and review["commit_id"] == commit and login != author
    )


def check_approved(repo, number, ask):
    """The commit and the branch to sign. `ask(path)` reads a path of GitHub's API."""
    pull = ask("repos/%s/pulls/%d" % (repo, number))
    head, commit = pull["head"], pull["head"]["sha"]
    if pull["state"] != "open" or pull["base"]["ref"] != publish.BRANCH:
        raise Refused("Pull request %d is not open to %s" % (number, publish.BRANCH))
    if not head["repo"] or head["repo"]["full_name"] != repo:
        raise Refused("Pull request %d comes from another repository" % number)
    if not COMMIT.fullmatch(commit) or not BRANCH.fullmatch(head["ref"]) or head["ref"] == publish.BRANCH:
        raise Refused("Pull request %d has a branch or a commit this script does not take" % number)
    found = approvers(ask("repos/%s/pulls/%d/reviews?per_page=100" % (repo, number)), pull["user"]["login"], commit)
    for login in found:
        if ask("repos/%s/collaborators/%s/permission" % (repo, login))["permission"] in CAN_WRITE:
            return commit, head["ref"]
    raise Refused(
        "Nobody else who can write here approved the last commit (%s) of pull request %d" % (commit[:8], number)
    )


def ask_github(path):
    return json.loads(publish.run([publish.gh(), "api", path]))


# ------------------------------------------------------------------------------------------- the files of a commit


def git(*args, data=None, env=None):
    """What git prints, as bytes."""
    words = ["git", "-C", str(publish.LAUNCHER), *args]
    publish.refuse_unsafe(words)
    # The commands are built by this script; what comes from a pull request is checked first, and is never an option.
    done = subprocess.run(words, input=data, env=env, capture_output=True)  # noqa: S603
    if done.returncode != 0:
        raise Refused("git %s failed: %s" % (args[0], done.stderr.decode("utf-8", "replace").strip()))
    return done.stdout


def files_of(commit, path):
    """{name: object} of the regular files of a commit at that path. Anything else there (a link) is refused."""
    out = {}
    for line in git("ls-tree", "-z", commit, "--", path).decode("utf-8").split("\0"):
        if line:
            about, _, name = line.partition("\t")
            mode, kind, obj = about.split()
            if mode not in ("100644", "100755") or kind != "blob":
                raise Refused("%s is not a regular file" % name)
            out[name] = obj
    return out


def read(obj, limit):
    if int(git("cat-file", "-s", obj)) > limit:
        raise Refused("A file is bigger than the %d bytes the launcher reads" % limit)
    return git("cat-file", "blob", obj)


def catalog_of(commit):
    """The catalog of a commit and its icons, {file name: bytes}, checked as far as signing needs."""
    found = files_of(commit, SOURCE)
    if list(found) != [SOURCE]:
        raise Refused("The pull request has no %s" % SOURCE)
    catalog = read(found[SOURCE], MAX_CATALOG)
    if b"\r" in catalog:
        raise Refused("%s has CR line ends; the signature covers LF bytes" % SOURCE)
    try:
        apps = tomllib.loads(catalog.decode("utf-8"))["apps"]
        names = [app["id"] for app in apps]
        repos = [app["repo"] for app in apps]
    except (ValueError, KeyError, TypeError) as error:
        raise Refused("%s is not a catalog: %s" % (SOURCE, error)) from error
    if len(set(names)) != len(names) or not all(isinstance(n, str) and ICON.fullmatch(n + ".png") for n in names):
        raise Refused("%s has an id that is repeated or is not a plain name" % SOURCE)
    owner = publish.LAUNCHER_REPO.partition("/")[0]
    if not all(isinstance(r, str) and r.startswith(owner + "/") for r in repos):
        raise Refused("%s has an app that is not under %s" % (SOURCE, owner))
    icons = {}
    for name, obj in files_of(commit, ICONS + "/").items():
        file = name.rpartition("/")[2]
        if not ICON.fullmatch(file):
            raise Refused("%s is not the name of an icon" % name)
        icons[file] = read(obj, MAX_ICON)
    return catalog, icons


# ------------------------------------------------------------------------------------------------------- signing


def sign(commit, key):
    """The commit that adds the signed catalog to `commit`."""
    if not COMMIT.fullmatch(commit):
        raise Refused("%s is not a commit" % commit)
    if not key.strip():
        raise Refused("There is no key: the secret CATALOG_SIGNING_KEY of the environment `catalog` is missing")
    catalog, icons = catalog_of(commit)
    with tempfile.TemporaryDirectory(prefix="open3d-sign-") as tmp:
        root = Path(tmp)
        for folder in ("key", "icons"):
            (root / folder).mkdir()
        (root / "key" / "key.pem").write_text(key.strip() + "\n", encoding="ascii", newline="\n")
        (root / "catalog-source.toml").write_bytes(catalog)
        for name, data in icons.items():
            (root / "icons" / name).write_bytes(data)
        # tools/publish.py signs as it does on a maintainer's computer, on these copies
        publish.SIGNING_KEY = root / "key" / "key.pem"
        publish.CATALOG_SOURCE = root / "catalog-source.toml"
        publish.CATALOG = root / "launcher-catalog.toml"
        publish.APP_ICONS = root / "icons"
        publish.ICONS = root / "launcher-app-icons"
        publish.sign_catalog()
        signature = Path(str(publish.CATALOG) + ".sig").read_bytes()
        made = {"launcher-app-icons/" + p.name: p.read_bytes() for p in publish.ICONS.iterdir()}
    made |= {"launcher-catalog.toml": catalog, "launcher-catalog.toml.sig": signature}
    made |= {FIXTURE: catalog, FIXTURE + ".sig": signature}
    return commit_files(commit, made)


def commit_files(parent, files):
    """A commit on top of `parent` with these files, made in Git's own objects: nothing is checked out."""
    with tempfile.TemporaryDirectory(prefix="open3d-index-") as tmp:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(tmp) / "index"))
        git("read-tree", parent, env=env)
        for name in sorted(files):
            obj = git("hash-object", "-w", "--stdin", data=files[name]).decode("ascii").strip()
            git("update-index", "--add", "--cacheinfo", "100644,%s,%s" % (obj, name), env=env)
        tree = git("write-tree", env=env).decode("ascii").strip()
    if tree == git("rev-parse", parent + "^{tree}").decode("ascii").strip():
        return parent  # signed already
    made = git("commit-tree", tree, "-p", parent, "-m", "Catalog: sign launcher-catalog.toml")
    return made.decode("ascii").strip()


def main(argv=None):
    parser = argparse.ArgumentParser(description="Signs the catalog of an approved pull request.")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("approved").add_argument("number", type=int)
    commands.add_parser("sign").add_argument("commit")
    args = parser.parse_args(argv)
    try:
        if args.command == "approved":
            commit, branch = check_approved(publish.LAUNCHER_REPO, args.number, ask_github)
            lines = ["commit=" + commit, "branch=" + branch]
        else:
            lines = ["signed=" + sign(args.commit, os.environ.get("CATALOG_SIGNING_KEY", ""))]
    except (Refused, publish.PublishError) as error:
        print("Not signed: %s" % error)
        return 1
    print("\n".join(lines))
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
