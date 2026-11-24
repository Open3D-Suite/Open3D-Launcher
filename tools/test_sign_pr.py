# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of sign_pr.py: only an approved commit is signed, what a pull request holds is data, and the key stays out of
what is published. They use a key made here and a Git repository of their own, in a temporary folder."""

import contextlib
import hashlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).parent))
import sign_pr  # noqa: E402 - tools/ is put on the path just above

publish = sign_pr.publish
REPO = publish.LAUNCHER_REPO
HEAD, OLDER = "a" * 40, "b" * 40


def review(login, state="APPROVED", commit=HEAD):
    return {"user": {"login": login}, "state": state, "commit_id": commit}


class Approval(unittest.TestCase):
    def ask(self, reviews, permission="admin", **changes):
        head = {"sha": HEAD, "ref": "add-app", "repo": {"full_name": REPO}} | changes.pop("head", {})
        pull = {"state": "open", "base": {"ref": "main"}, "head": head, "user": {"login": "author"}} | changes

        def answer(path):
            if path.endswith("/permission"):
                return {"permission": permission}
            return reviews if "/reviews" in path else pull

        return sign_pr.check_approved(REPO, 7, answer)

    def test_an_approval_of_the_last_commit_by_somebody_else_is_enough(self):
        self.assertEqual(self.ask([review("other")]), (HEAD, "add-app"))

    def test_nothing_else_is(self):
        for reviews in (
            [],
            [review("author")],
            [review("other", commit=OLDER)],
            [review("other", "COMMENTED")],
            [review("other"), review("other", "CHANGES_REQUESTED")],
            [review("other"), review("other", "DISMISSED")],
        ):
            with self.subTest(reviews=reviews), self.assertRaises(sign_pr.Refused):
                self.ask(reviews)

    def test_an_approval_by_somebody_who_cannot_write_does_not_count(self):
        with self.assertRaises(sign_pr.Refused):
            self.ask([review("visitor")], permission="read")

    def test_a_comment_after_the_approval_does_not_take_it_back(self):
        self.ask([review("other"), review("other", "COMMENTED")])

    def test_only_an_open_pull_request_of_this_repository_to_main(self):
        for changes in (
            {"state": "closed"},
            {"base": {"ref": "other"}},
            {"head": {"repo": {"full_name": "someone/Open3D-Launcher"}}},
            {"head": {"repo": None}},
            {"head": {"ref": "main"}},
            {"head": {"ref": "--force"}},
            {"head": {"ref": "a b"}},
        ):
            with self.subTest(changes=changes), self.assertRaises(sign_pr.Refused):
                self.ask([review("other")], **changes)


class Signing(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        folder = tempfile.TemporaryDirectory()
        cls.addClassCleanup(folder.cleanup)
        root = Path(folder.name)
        cls.public = root / "public.pem"
        private = root / "private.pem"
        for words in (["genpkey", "-algorithm", "ed25519", "-out"], ["pkey", "-in", str(private), "-pubout", "-out"]):
            target = private if words[0] == "genpkey" else cls.public
            subprocess.run([publish.openssl(), *words, str(target)], check=True, capture_output=True)  # noqa: S603
        cls.key = private.read_text(encoding="ascii")

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.repo = Path(folder.name)
        names = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com"}
        names |= {"GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com"}
        keep = {n: getattr(publish, n) for n in ("SIGNING_KEY", "CATALOG_SOURCE", "CATALOG", "APP_ICONS", "ICONS")}
        for patch in (
            mock.patch.dict(os.environ, names),
            mock.patch.multiple(publish, LAUNCHER=self.repo, PUBLIC_KEY=self.public, **keep),
        ):
            patch.start()
            self.addCleanup(patch.stop)
        sign_pr.git("init", "-q", "-b", "main")

    def catalog(self, icon=b"an icon", **app):
        app = {"id": "opentest", "repo": "Open3D-Suite/OpenTest"} | app
        app.setdefault("icon_sha256", hashlib.sha256(icon).hexdigest())
        return ("[[apps]]\n" + "".join('%s = "%s"\n' % x for x in app.items())).encode()

    def commit(self, files, links=()):
        index = ["100644,%s,%s" % (self.store(data), name) for name, data in files.items()]
        index += ["120000,%s,%s" % (self.store(target), name) for name, target in links]
        sign_pr.git("read-tree", "--empty")
        for entry in index:
            sign_pr.git("update-index", "--add", "--cacheinfo", entry)
        return sign_pr.git("commit-tree", sign_pr.git("write-tree").decode().strip(), "-m", "x").decode().strip()

    def store(self, data):
        return sign_pr.git("hash-object", "-w", "--stdin", data=data).decode().strip()

    def usual(self, **changes):
        files = {sign_pr.SOURCE: self.catalog(), "assets/app-icons/opentest.png": b"an icon", "README.md": b"x"}
        return files | changes

    def sign(self, commit):
        with contextlib.redirect_stdout(io.StringIO()) as said:
            made = sign_pr.sign(commit, self.key)
        self.assertNotIn("PRIVATE KEY", said.getvalue())
        return made

    def show(self, commit, name):
        return sign_pr.git("cat-file", "blob", "%s:%s" % (commit, name))

    def test_the_signed_files_are_added_on_top_and_the_signature_is_the_keys(self):
        parent = self.commit(self.usual())
        made = self.sign(parent)
        self.assertEqual(sign_pr.git("rev-parse", made + "^").decode().strip(), parent)
        changed = sign_pr.git("diff", "--name-only", parent, made).decode().split()
        icon = "launcher-app-icons/opentest-%s.png" % hashlib.sha256(b"an icon").hexdigest()
        fixture = sign_pr.FIXTURE
        self.assertEqual(
            changed, [fixture, fixture + ".sig", icon, "launcher-catalog.toml", "launcher-catalog.toml.sig"]
        )
        self.assertEqual(self.show(made, "launcher-catalog.toml"), self.catalog())
        self.assertEqual(self.show(made, fixture + ".sig"), self.show(made, "launcher-catalog.toml.sig"))
        for name in changed:
            self.assertNotIn(b"PRIVATE KEY", self.show(made, name))

    def test_signing_again_changes_nothing(self):
        made = self.sign(self.commit(self.usual()))
        self.assertEqual(self.sign(made), made)

    def test_a_link_is_never_read(self):
        files = self.usual()
        icon = files.pop("assets/app-icons/opentest.png")
        self.assertTrue(icon)
        for links in ([("assets/app-icons/opentest.png", b"../../key.pem")], [(sign_pr.SOURCE, b"/etc/passwd")]):
            kept = {n: d for n, d in files.items() if n != links[0][0]}
            with self.subTest(links=links), self.assertRaises(sign_pr.Refused):
                self.sign(self.commit(kept, links))

    def test_a_catalog_that_is_not_one_is_refused(self):
        for catalog in (
            b"not = [toml",
            b"apps = 5\n",
            self.catalog().replace(b"\n", b"\r\n"),
            self.catalog(repo="someone/OpenTest"),
            self.catalog(id="../key"),
            self.catalog() + self.catalog(),
            b"#" * (sign_pr.MAX_CATALOG + 1),
        ):
            with self.subTest(catalog=catalog[:40]), self.assertRaises(sign_pr.Refused):
                self.sign(self.commit(self.usual(**{sign_pr.SOURCE: catalog})))

    def test_an_icon_that_is_not_the_catalogs_is_refused(self):
        with self.assertRaises(publish.PublishError):
            self.sign(self.commit(self.usual(**{"assets/app-icons/opentest.png": b"another"})))

    def test_odd_or_big_icons_are_refused(self):
        big = b"x" * (sign_pr.MAX_ICON + 1)
        for name, data in (
            ("assets/app-icons/a b.png", b"x"),
            ("assets/app-icons/x.exe", b"x"),
            ("assets/app-icons/x.png", big),
        ):
            with self.subTest(name=name), self.assertRaises(sign_pr.Refused):
                self.sign(self.commit(self.usual(**{name: data})))

    def test_without_the_key_or_with_another_one_nothing_is_signed(self):
        commit = self.commit(self.usual())
        with self.assertRaises(sign_pr.Refused):
            sign_pr.sign(commit, "")
        with self.assertRaises(publish.PublishError) as caught:
            sign_pr.sign(commit, "-----BEGIN PRIVATE KEY-----\nbm90IGEga2V5\n-----END PRIVATE KEY-----")
        self.assertNotIn("bm90IGEga2V5", str(caught.exception))

    def test_a_commit_is_a_commit(self):
        for commit in ("main", "--help", "a" * 39, HEAD + "x"):
            with self.subTest(commit=commit), self.assertRaises(sign_pr.Refused):
                sign_pr.sign(commit, self.key)


if __name__ == "__main__":
    unittest.main()
