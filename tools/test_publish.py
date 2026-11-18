# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of what publish.py lets out of this computer: it never pushes, and it never publishes or changes a release.

    python -m unittest          (from the repository's root)

No command is run: `subprocess.run` is replaced by a fake that records each command and answers what the test
says, so the tests need no network, no git and no gh, and make no tag and no commit.
"""

import ast
import contextlib
import importlib.util
import io
import itertools
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("publish.py")
_spec = importlib.util.spec_from_file_location("publish", SCRIPT)
publish = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(publish)

HEAD = "a" * 40  # the commit to release
OLDER = "b" * 40  # a commit before it
TAG = "c" * 40  # the annotated tag v1.2.3 made on HEAD
OTHER = "d" * 40  # anything else

# The local tag v1.2.3, as `git for-each-ref` tells it.
ANNOTATED = "%s tag %s\n" % (TAG, HEAD)
LIGHTWEIGHT = "%s commit \n" % HEAD
ELSEWHERE = "%s tag %s\n" % (TAG, OLDER)

DRAFT = {"isDraft": True, "url": "https://github.com/o/r/releases/tag/untagged-1"}
PUBLISHED = {"isDraft": False, "url": "https://github.com/o/r/releases/tag/v1.2.3"}
NEW_DRAFT = "https://github.com/o/r/releases/tag/untagged-2"

# What GitHub answers about the release of the tag: to `gh api`, which only knows a published release, and to
# `gh release view`, which also sees a draft. "both" is a draft that shares its tag with a published release.
NOT_FOUND = (1, "gh: Not Found (HTTP 404)")
BROKEN = (1, "gh: Bad Gateway (HTTP 502)")
RELEASES = {
    None: (NOT_FOUND, (1, "release not found")),
    "draft": (NOT_FOUND, (0, json.dumps(DRAFT))),
    "published": ((0, PUBLISHED["url"] + "\n"), (0, json.dumps(PUBLISHED))),
    "both": ((0, PUBLISHED["url"] + "\n"), (0, json.dumps(DRAFT))),
    "broken": (BROKEN, BROKEN),
    "draft unseen": (NOT_FOUND, BROKEN),
    "published unseen": (BROKEN, (0, json.dumps(DRAFT))),
}
UNREADABLE = ("broken", "draft unseen", "published unseen")

# Every command the script may run after a build: the ones that only read, and the ones that write.
READS = {
    "git rev-parse",
    "git for-each-ref",
    "git ls-remote",
    "git merge-base",
    "git log",
    "git diff",
    "gh api",
    "gh release view",
}
WRITES = {"git add", "git commit", "git tag", "gh release create", "gh release upload"}


def name(words):
    """`git ls-remote`, `gh release view`: the program and what it is asked to do, without folders or options."""
    program = Path(words[0]).stem
    if program == "git":
        return "git " + (words[3] if words[1] == "-C" else words[1])
    if program == "gh":
        return "gh " + " ".join(words[1:2] if words[1] == "api" else words[1:3])
    return program


class Commands:
    """Stands in for `subprocess.run`: runs nothing, records every command and answers what the test says."""

    def __init__(self, answers):
        self.ran = []
        self.answers = answers

    def __call__(self, cmd, **_options):
        words = [str(w) for w in cmd]
        self.ran.append(words)
        code, text = self.answers.get(name(words), (0, ""))
        return subprocess.CompletedProcess(words, code, text if code == 0 else "", "" if code == 0 else text)

    def named(self, what):
        return [words for words in self.ran if name(words) == what]

    def names(self):
        return [name(words) for words in self.ran]

    def to_github(self):
        return [words for words in self.ran if Path(words[0]).stem == "gh"]


def world(local="", main=HEAD, tag=None, release=None, ancestor=False, changed=True):
    """What the commands find: the local tag, the remote's main and tag, the release (a key of RELEASES), whether
    the remote's main comes after the commit, and whether signing changed the catalog."""
    refs = ("%s\trefs/heads/main\n" % main if main else "") + ("%s\trefs/tags/v1.2.3\n" % tag if tag else "")
    by_tag, view = RELEASES[release]
    return Commands(
        {
            "git rev-parse": (0, HEAD + "\n"),
            "git for-each-ref": (0, local),
            "git ls-remote": (0, refs),
            "git merge-base": (0 if ancestor else 1, ""),
            "git log": (0, HEAD + "\n"),
            "git diff": (0, "launcher-catalog.toml\n" if changed else ""),
            "gh api": by_tag,
            "gh release view": view,
            "gh release create": (0, NEW_DRAFT + "\n"),
        }
    )


def go(commands, step, *args, **options):
    """Runs a step of the script on those commands. Returns what it printed and the error that stopped it, if any."""
    out, error = io.StringIO(), None
    with (
        mock.patch.object(publish.subprocess, "run", commands),
        mock.patch.object(publish, "gh", lambda: "gh"),
        contextlib.redirect_stdout(out),
        contextlib.redirect_stderr(out),
    ):
        try:
            step(*args, **options)
        except publish.PublishError as e:
            error = e
    return out.getvalue(), error


class Checks(unittest.TestCase):
    def assert_nothing_left(self, commands, out=""):
        """No push, by the script or told as `--tags`; no release that isn't a draft; nothing outside the list."""
        self.assertEqual([w for w in commands.ran if "push" in w], [])
        self.assertNotIn("--tags", out)
        self.assertLessEqual(set(commands.names()), READS | WRITES)
        for words in commands.named("gh api"):
            self.assertEqual([w for w in words[1:] if w.startswith("-")], ["--jq"])
        for words in commands.named("gh release create"):
            self.assertIn("--draft", words)
            self.assertIn("--verify-tag", words)
            self.assertNotIn("--latest", words)

    def assert_nothing_uploaded(self, commands):
        self.assertEqual(commands.named("gh release create") + commands.named("gh release upload"), [])


class Guard(Checks):
    def refused(self, *cmd):
        with self.assertRaises(publish.PublishError):
            publish.refuse_unsafe(list(cmd))

    def test_a_push_is_refused_however_it_is_written(self):
        self.refused("git", "push")
        self.refused("git", "push", "origin", "main")
        self.refused("git", "-C", "Z:/repo", "push", "origin", "v1.2.3")
        self.refused("git", "push", "--tags")
        self.refused("git", "-c", "push.default=current", "push", "--force")
        self.refused("C:/Program Files/Git/cmd/git.exe", "push")
        self.refused(Path("git"), "push")

    def test_a_release_that_is_not_a_draft_is_refused(self):
        self.refused("gh", "release", "create", "v1.2.3", "--repo", "o/r", "--verify-tag")
        self.refused("gh", "release", "create", "v1.2.3", "--draft", "--latest")
        self.refused("C:/Programs/GitHubCLI/bin/gh.exe", "release", "create", "v1.2.3")
        self.refused("gh", "release", "edit", "v1.2.3", "--draft=false")
        self.refused("gh", "release", "delete", "v1.2.3")
        self.refused("gh", "release", "delete-asset", "v1.2.3", "SHA256SUMS.txt")

    def test_github_is_only_asked_never_told(self):
        self.refused("gh", "api", "repos/o/r/releases", "-f", "tag_name=v1.2.3")
        self.refused("gh", "api", "repos/o/r/releases/1", "-X", "PATCH", "-F", "draft=false")
        self.refused("gh", "api", "--method", "DELETE", "repos/o/r/git/refs/tags/v1.2.3")
        self.refused("gh", "api", "-XDELETE", "repos/o/r/releases/1")
        self.refused("gh", "api", "repos/o/r/releases", "--input", "release.json")

    def test_a_tag_is_not_deleted_or_moved(self):
        self.refused("git", "tag", "-d", "v1.2.3")
        self.refused("git", "tag", "--delete", "v1.2.3")
        self.refused("git", "-C", "Z:/repo", "tag", "-f", "-a", "v1.2.3", "-m", "x")
        self.refused("git", "tag", "--force", "v1.2.3")

    def test_what_the_script_runs_is_let_through(self):
        git = ["git", "-C", "Z:/repo"]
        publish.refuse_unsafe(git + ["ls-remote", "origin", "refs/heads/main", "refs/tags/v1.2.3"])
        publish.refuse_unsafe(git + ["tag", "-a", "v1.2.3", "-m", "Open3D Launcher 1.2.3"])
        publish.refuse_unsafe(git + ["tag", "--list"])
        publish.refuse_unsafe(git + ["commit", "-m", "Catalog: sign launcher-catalog.toml"])
        publish.refuse_unsafe(["gh", "api", "repos/o/r/releases/tags/v1.2.3", "--jq", ".html_url"])
        publish.refuse_unsafe(["gh", "release", "view", "v1.2.3", "--repo", "o/r", "--json", "isDraft,url"])
        publish.refuse_unsafe(publish.create_command("gh", "o/r", "v1.2.3", "App 1.2.3", "notes.md"))
        publish.refuse_unsafe(publish.upload_command("gh", "o/r", "v1.2.3", ["a.zip"]))
        publish.refuse_unsafe(["powershell", "-File", "package.ps1"])

    def test_a_refused_command_is_never_run(self):
        commands = Commands({})
        with mock.patch.object(publish.subprocess, "run", commands):
            for runner in (publish.call, publish.run):
                for cmd in (["git", "push", "origin", "main"], ["gh", "release", "create", "v1.2.3"]):
                    with self.assertRaises(publish.PublishError):
                        runner(cmd)
        self.assertEqual(commands.ran, [])

    def test_no_command_written_in_the_script_names_a_push(self):
        """The script builds its commands as lists: none holds a push, `--tags`, `--latest` or what undoes a tag."""
        tree = ast.parse(SCRIPT.read_text(encoding="utf-8"))
        words = {
            item.value
            for node in ast.walk(tree)
            if isinstance(node, (ast.List, ast.Tuple))
            for item in node.elts
            if isinstance(item, ast.Constant) and isinstance(item.value, str)
        }
        self.assertLessEqual({"ls-remote", "release", "create", "--draft", "upload", "tag", "-a"}, words)
        self.assertEqual(words & {"push", "--tags", "--all", "--mirror", "--latest", "edit", "delete"}, set())
        self.assertEqual(words & {"-d", "--delete", "-f", "--force", "-X", "--method", "-F", "--field"}, set())

    def test_every_subprocess_goes_through_the_guard(self):
        """`subprocess` is used in one place only: `call`, which asks the guard first."""
        source = SCRIPT.read_text(encoding="utf-8")
        tree = ast.parse(source)
        uses = [node for node in ast.walk(tree) if isinstance(node, ast.Name) and node.id == "subprocess"]
        users = [
            function.name
            for function in ast.walk(tree)
            if isinstance(function, ast.FunctionDef) and any(use in ast.walk(function) for use in uses)
        ]
        self.assertEqual((len(uses), users), (1, ["call"]))
        for other_way in ("os.system", "os.popen", "os.spawn", "os.exec", "os.startfile", "Popen"):
            self.assertNotIn(other_way, source)


class Decisions(Checks):
    def test_a_tag_is_read_as_annotated_only_when_it_is(self):
        self.assertIsNone(publish.parse_tag(""))
        self.assertEqual(publish.parse_tag(ANNOTATED), publish.Tag(TAG, "tag", HEAD))
        self.assertEqual(publish.parse_tag(LIGHTWEIGHT), publish.Tag(HEAD, "commit", HEAD))

    def test_the_remote_is_read_ref_by_ref(self):
        text = "%s\trefs/heads/main\n%s\trefs/tags/v1.2.3\n" % (HEAD, TAG)
        self.assertEqual(publish.parse_refs(text), {"refs/heads/main": HEAD, "refs/tags/v1.2.3": TAG})
        self.assertEqual(publish.parse_refs(""), {})

    def test_a_tag_that_is_not_annotated_or_is_elsewhere_stops_it(self):
        publish.check_tag("v1.2.3", publish.parse_tag(ANNOTATED), HEAD)
        with self.assertRaisesRegex(publish.PublishError, "not an annotated tag"):
            publish.check_tag("v1.2.3", publish.parse_tag(LIGHTWEIGHT), HEAD)
        with self.assertRaisesRegex(publish.PublishError, "not on HEAD"):
            publish.check_tag("v1.2.3", publish.parse_tag(ELSEWHERE), HEAD)

    def test_a_remote_tag_that_is_not_the_local_one_stops_it(self):
        local = publish.parse_tag(ANNOTATED)
        publish.check_remote_tag("v1.2.3", local, None)
        publish.check_remote_tag("v1.2.3", local, TAG)
        publish.check_remote_tag("v1.2.3", None, None)
        for found, there in ((local, OTHER), (local, HEAD), (None, TAG)):
            with self.assertRaises(publish.PublishError):
                publish.check_remote_tag("v1.2.3", found, there)

    def test_the_pushes_are_one_branch_and_one_tag_by_name(self):
        self.assertEqual(publish.pushes_needed(True, "v1.2.3", True), [])
        self.assertEqual(publish.pushes_needed(True), [])
        self.assertEqual(publish.pushes_needed(False), ["git push origin main"])
        self.assertEqual(publish.pushes_needed(True, "v1.2.3", False), ["git push origin v1.2.3"])
        both = publish.pushes_needed(False, "v1.2.3", False)
        self.assertEqual(both, ["git push origin main", "git push origin v1.2.3"])

    def test_a_release_is_created_as_a_draft_replaced_as_a_draft_or_left_alone(self):
        self.assertEqual(publish.release_step("o/r", "v1.2.3", None), "create")
        self.assertEqual(publish.release_step("o/r", "v1.2.3", DRAFT), "replace")
        for state in (PUBLISHED, {"url": "x"}, {"isDraft": None}, {"isDraft": "true"}, {}):
            with self.assertRaisesRegex(publish.PublishError, "published"):
                publish.release_step("o/r", "v1.2.3", state)

    def test_the_command_that_creates_a_release_always_makes_a_draft(self):
        cmd = publish.create_command("gh", "o/r", "v1.2.3", "App 1.2.3", Path("notes.md"))
        self.assertEqual(cmd[:6], ["gh", "release", "create", "v1.2.3", "--repo", "o/r"])
        self.assertIn("--draft", cmd)
        self.assertIn("--verify-tag", cmd)
        self.assertNotIn("--latest", cmd)

    def test_the_command_that_uploads_names_the_files_and_the_release(self):
        cmd = publish.upload_command("gh", "o/r", "v1.2.3", [Path("a.zip"), Path("SHA256SUMS.txt")])
        self.assertEqual(
            cmd, ["gh", "release", "upload", "v1.2.3", "a.zip", "SHA256SUMS.txt", "--repo", "o/r", "--clobber"]
        )


class Release(Checks):
    """What GitHub has for the tag, read without git: the step every release goes through."""

    def state(self, release):
        commands = world(release=release)
        with mock.patch.object(publish.subprocess, "run", commands), mock.patch.object(publish, "gh", lambda: "gh"):
            try:
                return commands, publish.release_state("o/r", "v1.2.3")
            except publish.PublishError as e:
                return commands, e

    def test_it_tells_no_release_a_draft_and_a_published_one_apart(self):
        self.assertIsNone(self.state(None)[1])
        self.assertEqual(self.state("draft")[1], DRAFT)
        self.assertEqual(self.state("published")[1], PUBLISHED)

    def test_a_draft_that_shares_the_tag_of_a_published_release_counts_as_published(self):
        commands, state = self.state("both")
        self.assertEqual(state, PUBLISHED)
        self.assertEqual(commands.names(), ["gh api"])

    def test_a_release_that_cannot_be_read_is_not_taken_as_missing(self):
        for release in UNREADABLE:
            self.assertIsInstance(self.state(release)[1], publish.PublishError)

    def test_reading_it_changes_nothing(self):
        for release in RELEASES:
            commands, state = self.state(release)
            self.assertLessEqual(set(commands.names()), READS)
            self.assert_nothing_left(commands)


class Launcher(Checks):
    def release(self, commands, dry_run=False):
        files = [Path("open3d-launcher-1.2.3-windows-x64.msi"), Path("open3d-launcher-1.2.3-windows-x64-portable.zip")]
        return go(
            commands, publish.release_launcher, "1.2.3", Path("out"), files, Path("SHA256SUMS.txt"), "notes", dry_run
        )

    def test_a_dry_run_runs_no_command(self):
        commands = world(release="published")
        out, error = self.release(commands, dry_run=True)
        self.assertIsNone(error)
        self.assertEqual(commands.ran, [])
        self.assertIn("Dry run", out)

    def test_a_first_run_tags_and_stops_before_any_release_call(self):
        commands = world(local="", main=OLDER, tag=None)
        out, error = self.release(commands)
        self.assertIsNone(error)
        self.assertEqual(
            commands.named("git tag"),
            [["git", "-C", str(publish.LAUNCHER), "tag", "-a", "v1.2.3", "-m", "Open3D Launcher 1.2.3"]],
        )
        self.assertEqual(commands.to_github(), [])
        self.assertIn("    git push origin main\n    git push origin v1.2.3\n", out)
        self.assert_nothing_left(commands, out)

    def test_a_missing_remote_tag_stops_before_any_release_call(self):
        commands = world(local=ANNOTATED, main=HEAD, tag=None, release="draft")
        out, error = self.release(commands)
        self.assertIsNone(error)
        self.assertEqual(commands.named("git tag"), [])
        self.assertEqual(commands.to_github(), [])
        self.assertIn("    git push origin v1.2.3\n", out)
        self.assertNotIn("git push origin main", out)
        self.assert_nothing_left(commands, out)

    def test_a_commit_that_is_not_on_the_remote_stops_before_any_release_call(self):
        for main in (None, OLDER):
            commands = world(local=ANNOTATED, main=main, tag=TAG, release="draft")
            out, error = self.release(commands)
            self.assertIsNone(error)
            self.assertEqual(commands.to_github(), [])
            self.assertIn("    git push origin main\n", out)
            self.assertNotIn("git push origin v1.2.3", out)

    def test_a_remote_main_that_is_further_on_counts_as_having_the_commit(self):
        commands = world(local=ANNOTATED, main=OTHER, tag=TAG, release=None, ancestor=True)
        out, error = self.release(commands)
        self.assertIsNone(error)
        self.assertEqual(len(commands.named("gh release create")), 1)

    def test_a_tag_that_is_not_annotated_or_is_elsewhere_stops_everything(self):
        for local in (LIGHTWEIGHT, ELSEWHERE):
            commands = world(local=local, main=HEAD, tag=TAG)
            out, error = self.release(commands)
            self.assertIsNotNone(error)
            self.assertEqual(commands.names(), ["git rev-parse", "git for-each-ref"])

    def test_a_remote_tag_that_is_another_one_stops_everything(self):
        for local in ("", ANNOTATED):
            commands = world(local=local, main=HEAD, tag=OTHER)
            out, error = self.release(commands)
            self.assertIsNotNone(error)
            self.assertEqual(commands.named("git tag"), [])
            self.assertEqual(commands.to_github(), [])

    def test_with_everything_on_the_remote_it_creates_a_draft_and_says_so(self):
        commands = world(local=ANNOTATED, main=HEAD, tag=TAG, release=None)
        out, error = self.release(commands)
        self.assertIsNone(error)
        self.assertEqual(commands.names()[-4:], ["gh api", "gh release view", "gh release create", "gh release upload"])
        create, upload = commands.ran[-2:]
        self.assertEqual(
            create[:8],
            ["gh", "release", "create", "v1.2.3", "--repo", "carlesllavata/Open3D", "--draft", "--verify-tag"],
        )
        self.assertIn("open3d-launcher-1.2.3-windows-x64.msi", upload)
        self.assertIn("open3d-launcher-1.2.3-windows-x64-portable.zip", upload)
        self.assertIn("SHA256SUMS.txt", upload)
        self.assertIn("DRAFT", out)
        self.assertIn(NEW_DRAFT, out)
        self.assert_nothing_left(commands, out)

    def test_a_draft_gets_its_files_replaced(self):
        commands = world(local=ANNOTATED, main=HEAD, tag=TAG, release="draft")
        out, error = self.release(commands)
        self.assertIsNone(error)
        self.assertEqual(commands.named("gh release create"), [])
        self.assertEqual(len(commands.named("gh release upload")), 1)
        self.assertIn("--clobber", commands.named("gh release upload")[0])
        self.assertIn(DRAFT["url"], out)

    def test_a_published_release_is_never_uploaded_to(self):
        for release in ("published", "both"):
            commands = world(local=ANNOTATED, main=HEAD, tag=TAG, release=release)
            out, error = self.release(commands)
            self.assertRegex(str(error), "published")
            self.assertEqual(commands.names()[-1], "gh api")
            self.assert_nothing_uploaded(commands)

    def test_a_release_that_cannot_be_read_gets_nothing(self):
        for release in UNREADABLE:
            commands = world(local=ANNOTATED, main=HEAD, tag=TAG, release=release)
            out, error = self.release(commands)
            self.assertRegex(str(error), "502")
            self.assert_nothing_uploaded(commands)

    def test_whatever_it_finds_it_never_pushes_publishes_or_changes_a_published_release(self):
        found = itertools.product(
            ("", ANNOTATED, LIGHTWEIGHT, ELSEWHERE),
            (None, OLDER, HEAD, OTHER),
            (None, TAG, HEAD, OTHER),
            RELEASES,
            (False, True),
            (False, True),
        )
        seen = set()
        for local, main, tag, release, ancestor, dry_run in found:
            with self.subTest(local=local, main=main, tag=tag, release=release, ancestor=ancestor, dry_run=dry_run):
                commands = world(local, main, tag, release, ancestor)
                out, error = self.release(commands, dry_run)
                seen.update(commands.names())
                self.assert_nothing_left(commands, out)
                if dry_run:
                    self.assertEqual(commands.ran, [])
                if commands.named("git tag"):
                    self.assertEqual((local, tag), ("", None))
                if commands.to_github():
                    self.assertEqual((local, tag), (ANNOTATED, TAG))
                    self.assertTrue(main == HEAD or (main and ancestor))
                if commands.named("gh release create"):
                    self.assertIsNone(release)
                if commands.named("gh release upload"):
                    self.assertIn(release, (None, "draft"))
        self.assertLessEqual({"git tag", "gh api", "gh release view", "gh release create", "gh release upload"}, seen)


class App(Checks):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.zip = Path(folder.name) / "opentools-1.2.3-windows-x64-3dsmax.zip"
        self.sums = Path(folder.name) / "SHA256SUMS-opentools-1.2.3.txt"
        self.zip.write_bytes(b"zip")
        self.sums.write_text("0  %s\n" % self.zip.name, encoding="utf-8")

    def release(self, commands, dry_run=False):
        app = {"name": "OpenTools", "repo": "o/OpenTools"}
        args = (app, Path("checkout"), "1.2.3", "v1.2.3", self.zip, self.sums, "notes")
        return go(commands, publish.release_app, *args, dry_run=dry_run)

    def test_a_dry_run_runs_no_command(self):
        commands = world(local=ANNOTATED, tag=TAG, release="draft")
        out, error = self.release(commands, dry_run=True)
        self.assertIsNone(error)
        self.assertEqual(commands.ran, [])
        self.assertIn("Dry run", out)

    def test_a_missing_remote_tag_stops_before_any_release_call(self):
        for local in (ANNOTATED, LIGHTWEIGHT):
            commands = world(local=local, tag=None, release="draft")
            out, error = self.release(commands)
            self.assertIsNone(error)
            self.assertEqual(commands.names(), ["git ls-remote", "git for-each-ref"])
            self.assertIn("    git push origin v1.2.3\n", out)
            self.assertNotIn("git push origin main", out)
            self.assert_nothing_left(commands, out)

    def test_a_remote_tag_that_is_another_one_stops_before_any_release_call(self):
        commands = world(local=ANNOTATED, tag=OTHER, release="draft")
        out, error = self.release(commands)
        self.assertIsNotNone(error)
        self.assertEqual(commands.to_github(), [])

    def test_with_the_tag_on_the_remote_it_creates_a_draft_and_says_so(self):
        for local, tag in ((ANNOTATED, TAG), (LIGHTWEIGHT, HEAD)):
            commands = world(local=local, tag=tag, release=None)
            out, error = self.release(commands)
            self.assertIsNone(error)
            create, upload = commands.ran[-2:]
            self.assertEqual(
                create[:8], ["gh", "release", "create", "v1.2.3", "--repo", "o/OpenTools", "--draft", "--verify-tag"]
            )
            self.assertEqual(create[8:10], ["--title", "OpenTools 1.2.3"])
            self.assertEqual(upload[:5], ["gh", "release", "upload", "v1.2.3", str(self.zip)])
            self.assertEqual(Path(upload[5]).name, "SHA256SUMS.txt")
            self.assertIn("DRAFT", out)
            self.assertIn(NEW_DRAFT, out)
            self.assert_nothing_left(commands, out)

    def test_a_draft_gets_its_files_replaced(self):
        commands = world(local=ANNOTATED, tag=TAG, release="draft")
        out, error = self.release(commands)
        self.assertIsNone(error)
        self.assertEqual(commands.named("gh release create"), [])
        self.assertEqual(commands.named("gh release upload")[0][-3:], ["--repo", "o/OpenTools", "--clobber"])
        self.assertIn(DRAFT["url"], out)

    def test_a_published_release_is_never_uploaded_to(self):
        for release in ("published", "both"):
            commands = world(local=ANNOTATED, tag=TAG, release=release)
            out, error = self.release(commands)
            self.assertRegex(str(error), "published")
            self.assert_nothing_uploaded(commands)

    def test_whatever_it_finds_it_never_pushes_publishes_or_changes_a_published_release(self):
        found = itertools.product((ANNOTATED, LIGHTWEIGHT), (None, TAG, HEAD, OTHER), RELEASES, (False, True))
        seen = set()
        for local, tag, release, dry_run in found:
            with self.subTest(local=local, tag=tag, release=release, dry_run=dry_run):
                commands = world(local=local, tag=tag, release=release)
                out, error = self.release(commands, dry_run)
                seen.update(commands.names())
                self.assert_nothing_left(commands, out)
                self.assertEqual(commands.named("git tag"), [])
                if dry_run:
                    self.assertEqual(commands.ran, [])
                if commands.to_github():
                    self.assertEqual(tag, publish.parse_tag(local).object)
                if commands.named("gh release create"):
                    self.assertIsNone(release)
                if commands.named("gh release upload"):
                    self.assertIn(release, (None, "draft"))
        self.assertLessEqual({"gh api", "gh release view", "gh release create", "gh release upload"}, seen)


class Catalog(Checks):
    def test_a_dry_run_runs_no_command(self):
        commands = world()
        out, error = go(commands, publish.commit_catalog, dry_run=True)
        self.assertIsNone(error)
        self.assertEqual(commands.ran, [])

    def test_it_commits_and_leaves_the_push_to_the_owner(self):
        commands = world(main=OLDER, changed=True)
        out, error = go(commands, publish.commit_catalog)
        self.assertIsNone(error)
        self.assertEqual(commands.names()[:3], ["git add", "git diff", "git commit"])
        self.assertIn("    git push origin main\n", out)
        self.assert_nothing_left(commands, out)

    def test_it_commits_nothing_when_nothing_changed(self):
        commands = world(main=OLDER, changed=False)
        out, error = go(commands, publish.commit_catalog)
        self.assertIsNone(error)
        self.assertEqual(commands.named("git commit"), [])
        self.assertIn("    git push origin main\n", out)
        self.assert_nothing_left(commands, out)

    def test_it_asks_for_no_push_when_the_remote_has_the_commit(self):
        for main, ancestor in ((HEAD, False), (OTHER, True)):
            commands = world(main=main, ancestor=ancestor)
            out, error = go(commands, publish.commit_catalog)
            self.assertIsNone(error)
            self.assertNotIn("git push", out)
            self.assert_nothing_left(commands, out)


class CommandLine(Checks):
    """`--dry-run` from the command line, with the builds left out: what comes after a build runs no command."""

    def dry_run(self, argv, build, built=None):
        commands = world(local="", main=OLDER, tag=None, release=None)
        with mock.patch.object(publish, build, return_value=built) as builder:
            out, error = go(commands, lambda: self.assertEqual(publish.main(argv), 0))
        self.assertIsNone(error)
        self.assertEqual(builder.call_count, 1)
        self.assertEqual(commands.ran, [])

    def test_app_dry_run(self):
        app = {"name": "OpenTools", "repo": "o/OpenTools"}
        built = (app, Path("checkout"), "1.2.3", "v1.2.3", Path("a.zip"), Path("sums.txt"), "notes")
        self.dry_run(["app", "opentools", "--dry-run"], "build_app", built)

    def test_launcher_dry_run(self):
        built = ("1.2.3", Path("out"), [Path("a.msi"), Path("a.zip")], Path("SHA256SUMS.txt"), "notes")
        self.dry_run(["launcher", "--dry-run"], "build_launcher", built)

    def test_catalog_dry_run(self):
        self.dry_run(["catalog", "--dry-run"], "sign_catalog")

    def test_no_command_offers_an_option_that_pushes(self):
        for argv in (["--help"], ["app", "--help"], ["catalog", "--help"], ["launcher", "--help"]):
            out = io.StringIO()
            with contextlib.redirect_stdout(out), self.assertRaises(SystemExit) as stopped:
                publish.main(argv)
            self.assertEqual(stopped.exception.code, 0)
            self.assertIn("usage:", out.getvalue())
            self.assertNotIn("-push", out.getvalue())


if __name__ == "__main__":
    unittest.main()
