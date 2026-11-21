import contextlib
from contextlib import chdir
import hashlib
import io
import json
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools.ci import nightly

PREVIOUS = "26.3-0.1.10"
OLD, HEAD = "a" * 40, "b" * 40
DECLARATION = {"id": "behaviour:example", "hash": "declaration", "version": 1}
SURFACE = {"schema": 3, "hash": "surface", "behaviours": [DECLARATION]}
ADDITIVE = {**SURFACE, "behaviours": [DECLARATION, {**DECLARATION, "id": "behaviour:new"}]}
CONDITIONAL = {**SURFACE, "behaviours": [{**DECLARATION, "version": 2}]}
BREAKING = {**SURFACE, "behaviours": []}


def git(*args):
    """Git for building fixture repositories in the current folder; `nightly.git` always runs in its checkout."""
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout.strip()


def release(tag, *, draft=False, prerelease=False, body=""):
    return {"tag_name": tag, "draft": draft, "prerelease": prerelease, "published_at": "2026-11-20T10:00:00Z",
            "body": body}


def function(signature="signature", source="source", span=(5, 9)):
    return {"module": "module", "file": "module.rs", "span": span, "signature_hash": signature, "sha256": source}


def sdk_assets(sha, surface, content_api, functions, *, function_header=None):
    header = {"schema": 1, "build": {"commit": sha}}
    index_body = {"functions": functions, "crates": {}, "refused": {}}
    build_data = io.BytesIO()
    with tarfile.open(fileobj=build_data, mode="w:xz") as tar:
        for name, fields in (("functions", index_body | (function_header or {})), ("internals", {"internals": []}),
                             ("loops", {"loops": []})):
            data = json.dumps(header | fields).encode()
            member = tarfile.TarInfo(f"./{name}.json")
            member.size = len(data)
            tar.addfile(member, io.BytesIO(data))
    assets = {nightly.SDK_SURFACE: json.dumps(surface).encode(), nightly.SDK_BUILD_DATA: build_data.getvalue()}
    index = {"schema": 1, "build": {"commit": sha}, "content_api": content_api,
             "surface": {"previous": "c" * 40},  # A Nightly SDK is not a stable baseline.
             "assets": {name: {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
                        for name, data in assets.items()}}
    assets[nightly.SDK_INDEX] = json.dumps(index).encode()
    return assets


TOOL = "/judge/sunrise-stage"


def signed_release_file(tag, *names):
    """A published stable release's signed sunrise-release.json, naming the commit its tag names now."""
    return {nightly.RELEASE_FILE: json.dumps({"commit": nightly.train.origin_tags()[tag]}).encode()}


class StableModApiTests(unittest.TestCase):
    def setUp(self):
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.notes = Path(directory) / "nightly-notes.md"
        self.notes.write_text("Existing notes.\n")
        self.release_file = self.enterContext(mock.patch.object(nightly, "release_assets",
                                                                side_effect=signed_release_file))
        self.stage = self.enterContext(mock.patch.object(nightly, "parent_stage", return_value=TOOL))
        self.unpacked = []
        self.enterContext(mock.patch.object(nightly, "unpack",
                                            side_effect=lambda sha, into: self.unpacked.append((sha, into.name))))

    def judge(self, parent_tree, candidate_tree, verdict="identical", calls=None):
        """subprocess.run as the parent's sunrise-stage answers: it exports each tree's surface, and diffs."""
        def run(args, *, cwd=None, env=None, check=False, capture_output=False, text=False):
            self.assertEqual(args[0], TOOL, "only the parent's own tool runs")
            self.assertFalse({"GH_TOKEN", "RUSTC_WRAPPER"} & env.keys())
            if calls is not None:
                calls.append(args)
            if args[1] == "surface":
                surface = parent_tree if Path(args[2]).name == "parent-tree" else candidate_tree
                Path(args[3]).write_text(json.dumps(surface))
            else:
                Path(args[-1]).write_text(json.dumps({"verdict": verdict}))
            return subprocess.CompletedProcess(args, 0, "", "")
        return run

    def test_same_line_accepts_only_changes_allowed_by_the_version_step(self):
        cases = (
            ("26.3-0.1.11", SURFACE, "identical", "1", None),
            ("26.3-0.1.11", ADDITIVE, "additive", "1", "patch release.*additive"),
            ("26.3-0.2.0", ADDITIVE, "additive", "1", None),
            ("26.3-0.2.0", CONDITIONAL, "conditional", "1", "minor release.*conditional"),
            ("26.3-0.2.0", BREAKING, "breaking", "1", "minor release.*breaking"),
            ("26.3-0.1.11", SURFACE, "identical", "2", "patch release.*content_api changed"),
            ("26.3-0.2.0", ADDITIVE, "additive", "2", "minor release.*content_api changed"),
        )
        for tag, surface, verdict, content_api, error in cases:
            with self.subTest(tag=tag, verdict=verdict, content_api=content_api):
                old_functions = {"body_change": function(), "signature_change": function(),
                                 "span_move": function(), "removed": function()}
                new_functions = {"body_change": function(source="new body"),
                                 "signature_change": function(signature="new signature"),
                                 "span_move": function(span=(7, 11)), "added": function()}
                assets = {OLD: sdk_assets(OLD, SURFACE, "1", old_functions),
                          HEAD: sdk_assets(HEAD, surface, content_api, new_functions)}

                def verified(sha, *names):
                    return {name: assets[sha][name] for name in names}

                calls = []
                compare = self.judge(SURFACE, surface, verdict, calls)

                pages = [[release("26.3-0.1.9"), release("26.2-2.0.1"), release(tag),
                          release("26.3-0.2.0", draft=True), release("26.3-0.1.20", prerelease=True),
                          release(f"sdk-{HEAD}")], [release(PREVIOUS)]]
                tags = {tag: HEAD, PREVIOUS: OLD}
                original = "Existing notes.\n"
                self.notes.write_text(original)
                with mock.patch.object(nightly.train, "gh_json", return_value=pages), \
                        mock.patch.object(nightly.train, "origin_tags", return_value=tags), \
                        mock.patch.object(nightly, "git"), \
                        mock.patch.object(nightly, "require_stable_lineage") as lineage, \
                        mock.patch.object(nightly, "require_unchanged_content_schema"), \
                        mock.patch.object(nightly, "released_assets", side_effect=verified), \
                        mock.patch.dict(nightly.os.environ, {"GH_TOKEN": "secret", "RUSTC_WRAPPER": "sccache"}), \
                        mock.patch.object(nightly.subprocess, "run", side_effect=compare):
                    if error:
                        with self.assertRaisesRegex(SystemExit, error):
                            nightly.stable_mod_api(HEAD, tag, self.notes)
                        self.assertEqual(self.notes.read_text(), original)
                    else:
                        nightly.stable_mod_api(HEAD, tag, self.notes)
                        self.assertEqual(self.notes.read_text(), original + f"\n### Changed mixin targets\n\n"
                                         f"Compared with `{PREVIOUS}`:\n\n- Added: `added`\n"
                                         "- Changed: `body_change`\n- Removed: `removed`\n"
                                         "- Changed: `signature_change`\n")
                    lineage.assert_called_once_with(HEAD, tag, OLD, PREVIOUS)
                    # The parent's tool exported both trees, then diffed its own exports.
                    self.assertEqual([args[1] for args in calls], ["surface", "surface", "surface-diff"])
                    self.assertEqual([Path(name).name for name in calls[-1][2:4]], ["parent.json", "candidate.json"])
                self.stage.assert_called_with(OLD, mock.ANY)

    def test_a_break_glass_release_keeps_its_parents_surface_judged_by_the_parents_own_stage(self):
        # The parent's own sunrise-stage exports both trees itself: a candidate whose own exporter hides a change
        # (its published surface.json unchanged) is still refused.
        tag = "26.3-0.1.11"
        other = {**SURFACE, "hash": "other"}
        for published, parent_tree, candidate_tree, error in (
                (SURFACE, SURFACE, SURFACE, None),
                (other, SURFACE, SURFACE, "changes surface.json from 26.3-0.1.10"),
                (SURFACE, SURFACE, other, "exports a different surface from 26.3-0.1.11's tree"),
                (SURFACE, other, other, "does not export its published surface.json")):
            assets = {OLD: sdk_assets(OLD, SURFACE, "1", {}), HEAD: sdk_assets(HEAD, published, "1", {})}
            calls = []
            self.unpacked.clear()
            with self.subTest(error=error), mock.patch.dict(nightly.os.environ, {"BREAK_GLASS": "true"}), \
                    mock.patch.object(nightly.train, "gh_json", return_value=[[release(PREVIOUS)]]), \
                    mock.patch.object(nightly.train, "origin_tags", return_value={tag: HEAD, PREVIOUS: OLD}), \
                    mock.patch.object(nightly, "git"), mock.patch.object(nightly, "require_stable_lineage"), \
                    mock.patch.object(nightly, "require_unchanged_content_schema"), \
                    mock.patch.object(nightly, "released_assets", side_effect=lambda sha, *names: assets[sha]), \
                    mock.patch.object(nightly.subprocess, "run", side_effect=self.judge(parent_tree, candidate_tree,
                                                                                         calls=calls)):
                if error:
                    with self.assertRaisesRegex(SystemExit, error):
                        nightly.stable_mod_api(HEAD, tag, self.notes)
                    self.assertNotIn("surface-diff", [args[1] for args in calls])
                    continue
                nightly.stable_mod_api(HEAD, tag, self.notes)
            self.assertEqual(self.unpacked, [(OLD, "parent-tree"), (HEAD, "candidate-tree")])
            self.assertEqual([args[1] for args in calls], ["surface", "surface", "surface-diff"])

    def test_an_ordinary_release_is_judged_by_the_parents_tool_too(self):
        # Not only break-glass: a candidate whose own exporter would hide a break is diffed from the parent tool's
        # export of its tree, and a tree that tool cannot read is refused, never judged by the candidate's tool.
        tag = "26.3-0.1.11"
        assets = {OLD: sdk_assets(OLD, SURFACE, "1", {}), HEAD: sdk_assets(HEAD, SURFACE, "1", {})}
        common = (mock.patch.object(nightly.train, "gh_json", return_value=[[release(PREVIOUS)]]),
                  mock.patch.object(nightly.train, "origin_tags", return_value={tag: HEAD, PREVIOUS: OLD}),
                  mock.patch.object(nightly, "git"), mock.patch.object(nightly, "require_stable_lineage"),
                  mock.patch.object(nightly, "require_unchanged_content_schema"),
                  mock.patch.object(nightly, "released_assets", side_effect=lambda sha, *names: assets[sha]))
        with contextlib.ExitStack() as stack:
            for patch in common:
                stack.enter_context(patch)
            stack.enter_context(mock.patch.object(nightly.subprocess, "run",
                                                  side_effect=self.judge(SURFACE, BREAKING, "breaking")))
            with self.assertRaisesRegex(SystemExit, "patch release after 26.3-0.1.10: mod API is breaking"):
                nightly.stable_mod_api(HEAD, tag, self.notes)

        def unreadable(args, **kwargs):
            if Path(args[2]).name == "candidate-tree":
                return subprocess.CompletedProcess(args, 1, "", "unknown key")
            Path(args[3]).write_text(json.dumps(SURFACE))
            return subprocess.CompletedProcess(args, 0, "", "")
        with contextlib.ExitStack() as stack:
            for patch in common:
                stack.enter_context(patch)
            stack.enter_context(mock.patch.object(nightly.subprocess, "run", side_effect=unreadable))
            with self.assertRaisesRegex(SystemExit, f"sunrise-stage cannot export candidate {HEAD}: unknown key"):
                nightly.stable_mod_api(HEAD, tag, self.notes)

    def test_a_moved_parent_tag_or_a_version_that_skips_a_step_is_refused(self):
        tags = {PREVIOUS: OLD}
        with mock.patch.object(nightly.train, "gh_json", return_value=[[release(PREVIOUS)]]), \
                mock.patch.object(nightly.train, "origin_tags", return_value=tags), mock.patch.object(nightly, "git"), \
                mock.patch.object(nightly, "released_assets") as inputs:
            # The tag now names another commit than the release's own signed file.
            self.release_file.side_effect = lambda tag, *names: {nightly.RELEASE_FILE: json.dumps({"commit": "9" * 40}).encode()}
            with self.assertRaisesRegex(SystemExit, f"the tag {PREVIOUS} names {OLD}, but its signed"):
                nightly.stable_mod_api(HEAD, "26.3-0.1.11", self.notes)
            self.release_file.side_effect = signed_release_file
            for tag in ("26.3-0.2.1", "26.3-1.0.1", "26.3-1.1.0"):
                with self.subTest(tag=tag), self.assertRaisesRegex(SystemExit, f"{tag} does not follow {PREVIOUS}"):
                    nightly.stable_mod_api(HEAD, tag, self.notes)
        inputs.assert_not_called()
        with mock.patch.object(nightly.train, "gh_json", return_value=[[]]), \
                mock.patch.object(nightly.train, "origin_tags", return_value={}), \
                self.assertRaisesRegex(SystemExit, "the first stable release on 26.3 is 26.3-0.1.0, not 26.3-0.2.0"):
            nightly.stable_mod_api(HEAD, "26.3-0.2.0", self.notes)
        for parent, tag in ((None, "26.3-0.1.0"), (PREVIOUS, "26.3-0.1.11"), (PREVIOUS, "26.3-0.1.15"),
                            (PREVIOUS, "26.3-0.2.0"), (PREVIOUS, "26.3-1.0.0")):
            with self.subTest(parent=parent, tag=tag):
                found = nightly.train.Release(parent, OLD, "") if parent else None
                self.assertEqual(nightly.stable_step(tag, found), "")

    def test_a_break_glass_release_needs_a_published_parent_on_its_line(self):
        for pages, tags, tag, error in (
                ([[]], {}, "26.3-0.1.0", "needs a published stable release to compare with"),
                ([[release(PREVIOUS)]], {PREVIOUS: OLD}, "26.3-1.0.0", "is a patch of 26.3-0.1.10, not 26.3-1.0.0"),
                ([[release(PREVIOUS)]], {PREVIOUS: OLD}, "26.3-0.2.0", "is a patch of 26.3-0.1.10, not 26.3-0.2.0")):
            with self.subTest(tag=tag), mock.patch.dict(nightly.os.environ, {"BREAK_GLASS": "true"}), \
                    mock.patch.object(nightly.train, "gh_json", return_value=pages), \
                    mock.patch.object(nightly.train, "origin_tags", return_value=tags), \
                    mock.patch.object(nightly, "git"), \
                    mock.patch.object(nightly, "released_assets") as inputs, \
                    self.assertRaisesRegex(SystemExit, error):
                nightly.stable_mod_api(HEAD, tag, self.notes)
            inputs.assert_not_called()

    def test_first_release_does_not_need_an_earlier_sdk(self):
        with mock.patch.object(nightly.train, "gh_json", return_value=[[release("26.2-0.1.3")]]), \
                mock.patch.object(nightly.train, "origin_tags", return_value={"26.2-0.1.3": OLD}), \
                mock.patch.object(nightly, "released_assets") as inputs:
            nightly.stable_mod_api(HEAD, "26.3-0.1.0", self.notes)
        inputs.assert_not_called()
        self.assertIn("No earlier stable release to compare.", self.notes.read_text())

    def test_new_major_only_compares_mixin_targets(self):
        old = sdk_assets(OLD, SURFACE, "1", {"target": function()})
        new = sdk_assets(HEAD, BREAKING, "2", {"target": function(source="new body")})
        with mock.patch.object(nightly.train, "gh_json", return_value=[[release(PREVIOUS)]]), \
                mock.patch.object(nightly.train, "origin_tags", return_value={PREVIOUS: OLD}), \
                mock.patch.object(nightly, "git"), \
                mock.patch.object(nightly, "require_stable_lineage") as lineage, \
                mock.patch.object(nightly, "released_assets", side_effect=[old, new]) as inputs, \
                mock.patch.object(nightly.subprocess, "run") as diff:
            nightly.stable_mod_api(HEAD, "26.3-1.0.0", self.notes)
        diff.assert_not_called()
        lineage.assert_not_called()
        self.assertEqual(inputs.call_args_list, [mock.call(sha, nightly.SDK_INDEX, nightly.SDK_BUILD_DATA)
                                                for sha in (OLD, HEAD)])
        self.assertIn(f"Compared with `{PREVIOUS}`:\n\n- Changed: `target`", self.notes.read_text())

    def test_previous_stable_missing_a_tag_stops_the_comparison(self):
        with mock.patch.object(nightly.train, "gh_json", return_value=[[release(PREVIOUS)]]), \
                mock.patch.object(nightly.train, "origin_tags", return_value={}):
            with self.assertRaisesRegex(SystemExit, f"the published release {PREVIOUS} has no tag"):
                nightly.stable_mod_api(HEAD, "26.3-0.1.11", self.notes)

    def test_the_parent_is_the_newest_published_release_and_an_orphan_tag_above_it_refuses(self):
        pages = [[release("26.3-0.1.9"), release(PREVIOUS, body=f"x\n{nightly.BREAK_GLASS_SECTION}\ny"),
                  release("26.3-0.2.0", draft=True), release("26.4-0.1.0"), release("26.3-0.1.11")]]
        tags = {"26.3-0.1.9": "9" * 40, PREVIOUS: OLD, "26.3-0.1.11": HEAD, "26.4-0.1.0": "4" * 40}
        with mock.patch.object(nightly.train, "gh_json", return_value=pages), mock.patch.object(nightly, "git") as git:
            with mock.patch.object(nightly.train, "origin_tags", return_value=tags):
                parent = nightly.stable_parent("26.3-0.1.11")
                # Its notes say nothing to the checks: the parent is its tag, commit and publication time.
                self.assertEqual(parent, nightly.train.Release(PREVIOUS, OLD, "2026-11-20T10:00:00Z"))
                git.assert_called_once_with("fetch", "--quiet", "--no-tags", "origin", OLD)
                self.assertEqual(nightly.stable_parent().tag, "26.3-0.1.11", "no bound: the newest")
                self.assertEqual(nightly.stable_parent("26.3-0.1.10").tag, "26.3-0.1.9")
            # An aborted run's tag, or one pushed by hand, above the newest published release: no release may be
            # judged against the wrong parent. The draft has no tag and doesn't count.
            for orphan in ("26.3-0.1.12", "26.3-0.2.0", "26.3-1.0.0"):
                with self.subTest(orphan=orphan), \
                        mock.patch.object(nightly.train, "origin_tags", return_value={**tags, orphan: "e" * 40}), \
                        self.assertRaisesRegex(SystemExit, f"{orphan} is a tag with no published release above 26.3-0.1.11"):
                    nightly.stable_parent("26.3-1.1.0")
            with mock.patch.object(nightly.train, "origin_tags", return_value={"26.3-0.1.0": "e" * 40}), \
                    mock.patch.object(nightly.train, "gh_json", return_value=[[release("26.4-0.1.0")]]), \
                    self.assertRaisesRegex(SystemExit, "26.3-0.1.0 is a tag with no published release above every"):
                nightly.stable_parent("26.3-0.1.1")
            with mock.patch.object(nightly.train, "origin_tags", return_value={"26.3-0.1.0": "e" * 40}), \
                    mock.patch.object(nightly.train, "gh_json", return_value=[[]]):
                self.assertIsNone(nightly.stable_parent("26.3-0.1.0"), "the first release's own tag is not an orphan")

    def test_an_sdk_asset_must_match_its_index(self):
        assets = sdk_assets(OLD, SURFACE, "1", {})
        assets[nightly.SDK_SURFACE] += b"\n"
        with mock.patch.object(nightly, "released_assets", return_value=assets):
            with self.assertRaisesRegex(SystemExit, "surface.json does not match sdk.json"):
                nightly.stable_sdk_inputs(OLD, self.notes.with_name("surface.json"))

    def test_function_index_must_name_its_build_and_have_a_supported_schema(self):
        cases = (({"build": {"commit": HEAD}}, f"functions.json describes game build {HEAD}, not {OLD}"),
                 ({"schema": 2}, "functions.json has build-data schema 2; expected 1"))
        for header, error in cases:
            with self.subTest(header=header), \
                    mock.patch.object(nightly, "released_assets",
                                      return_value=sdk_assets(OLD, SURFACE, "1", {}, function_header=header)), \
                    self.assertRaisesRegex(SystemExit, error):
                nightly.stable_sdk_inputs(OLD, None)

    def test_same_line_accepts_pr_merges_of_backports_but_not_new_main_history(self):
        repo, checkout = (self.notes.with_name(name) for name in ("upstream", "checkout"))
        repo.mkdir()
        with chdir(repo):
            git("init", "--initial-branch=main")
            for key, default in (("user.name", "Release Test"), ("user.email", "release-test@example.invalid")):
                git("config", key, git("config", "--default", default, "--get", key))
            # The base is shared; the upstream-only change makes the cherry-pick a different commit.
            git("commit", "--allow-empty", "-m", "initial")
            unrelated = git("rev-parse", "HEAD")
            git("commit", "--allow-empty", "-m", "previous stable")
            previous = git("rev-parse", "HEAD")
            git("branch", "release")
            git("commit", "--allow-empty", "-m", "main-only change")
            Path("fix").touch()
            git("add", "fix")
            git("commit", "-m", "fix")
            main = git("rev-parse", "HEAD")
            git("switch", "-c", "backport", previous)
            git("cherry-pick", main)
            git("switch", "release")
            git("merge", "--no-ff", "backport", "-m", "backport PR")
            candidate = git("rev-parse", "HEAD")
            git("switch", "-c", "imported")
            git("merge", "--no-ff", "main", "-m", "bring in main")
            imported = git("rev-parse", "HEAD")
        git("clone", "--depth", "1", "--branch", "release", repo.as_uri(), str(checkout))
        with chdir(checkout), mock.patch.object(nightly, "REPOSITORY", checkout):
            tag = "26.3-0.1.11"
            nightly.require_stable_lineage(candidate, tag, previous, PREVIOUS)
            self.assertEqual(git("rev-parse", "--is-shallow-repository"), "false")
            with self.assertRaisesRegex(SystemExit, "brings in main history"):
                nightly.require_stable_lineage(imported, tag, previous, PREVIOUS)
            with self.assertRaisesRegex(SystemExit, "does not descend from"):
                nightly.require_stable_lineage(unrelated, tag, previous, PREVIOUS)

    def test_same_line_accepts_backport_already_merged_back_into_main(self):
        repo, checkout = (self.notes.with_name(name) for name in ("upstream", "checkout"))
        repo.mkdir()
        with chdir(repo):
            git("init", "--initial-branch=main")
            for key, default in (("user.name", "Release Test"), ("user.email", "release-test@example.invalid")):
                git("config", key, git("config", "--default", default, "--get", key))
            git("commit", "--allow-empty", "-m", "previous stable")
            previous = git("rev-parse", "HEAD")
            git("switch", "-c", "release")
            git("commit", "--allow-empty", "-m", "backport")
            git("switch", "main")
            git("commit", "--allow-empty", "-m", "main-only change")
            git("merge", "--no-ff", "release", "-m", "back-merge release")
            git("switch", "release")
            git("commit", "--allow-empty", "-m", "another release fix")
            candidate = git("rev-parse", "HEAD")
        git("clone", "--depth", "1", "--branch", "release", repo.as_uri(), str(checkout))
        with chdir(checkout), mock.patch.object(nightly, "REPOSITORY", checkout):
            nightly.require_stable_lineage(candidate, "26.3-0.1.11", previous, PREVIOUS)

    def test_only_a_new_major_can_change_content_declaration_files(self):
        repo = self.notes.with_name("repo")
        repo.mkdir()
        reader, helper = nightly.CONTENT_READER + "\n", nightly.CONTENT_HELPER + "\n"
        manifest = "crates/sunrise-mod-package/src/manifest.rs"
        # A reader outside the folders is held by its line alone.
        elsewhere = "crates/sunrise-client-render/src/particles/declarations.rs"
        dropped = "crates/sunrise-client/src/mod_content_structures.rs"
        tests = "crates/sunrise-mod-package/src/content/entity_sounds_tests.rs"
        runtime = "crates/sunrise-client/src/mod_content/root_lifetime.rs"
        unmarked = "crates/sunrise-client-render/src/particles.rs"
        unclassified = "crates/sunrise-mod-package/src/content/new_reader.rs"

        def commit(name, text):
            Path(name).parent.mkdir(parents=True, exist_ok=True)
            Path(name).write_text(text)
            git("add", name)
            git("commit", "-m", name)
            return git("rev-parse", "HEAD")

        with chdir(repo):
            git("init", "--initial-branch=main")
            git("config", "user.name", "Release Test")
            git("config", "user.email", "release-test@example.invalid")
            for name in (manifest, elsewhere, dropped):
                commit(name, reader)
            commit(runtime, helper)
            commit(unmarked, "")
            previous = commit(tests, "")
            for name in (tests, runtime, unmarked):
                unrelated_edit = commit(name, Path(name).read_text() + "// fixed\n")
            # One reader changes, one is turned into a helper, one appears.
            commit(manifest, reader + "// changed\n")
            commit(dropped, helper)
            schema_edit = commit("crates/sunrise-world/src/loot/new_reader.rs", reader)
            unjudged = commit(unclassified, "")
            decoding = commit(unclassified, helper + "use serde::Deserialize;\n")
            # Keep the checkout elsewhere to prove the guard reads the passed commits.
            git("switch", "--detach", previous)
        declarations = (dropped, "crates/sunrise-world/src/loot/new_reader.rs")

        parent = nightly.train.Release(PREVIOUS, previous, "")
        run = subprocess.run
        judge = self.judge(SURFACE, SURFACE)

        def judge_or_git(args, **kwargs):
            return judge(args, **kwargs) if args[0] == TOOL else run(args, **kwargs)

        with mock.patch.object(nightly, "REPOSITORY", repo), \
                mock.patch.object(nightly, "stable_parent", return_value=parent), \
                mock.patch.object(nightly, "require_stable_lineage"), \
                mock.patch.dict(nightly.os.environ, {"BREAK_GLASS": "false"}), \
                mock.patch.object(nightly, "released_assets",
                                  side_effect=lambda sha, *names: sdk_assets(sha, SURFACE, "1", {})), \
                mock.patch.object(nightly.subprocess, "run", side_effect=judge_or_git):
            for tag in ("26.3-0.1.11", "26.3-0.2.0"):
                with self.subTest(tag=tag):
                    nightly.stable_mod_api(unrelated_edit, tag, self.notes)
                    with self.assertRaisesRegex(SystemExit, "changes content declaration files") as refused:
                        nightly.stable_mod_api(schema_edit, tag, self.notes)
                    self.assertIn(manifest, str(refused.exception))
                    for declaration in declarations:
                        self.assertIn(declaration, str(refused.exception))
                    for allowed in (tests, runtime, unmarked, elsewhere):
                        self.assertNotIn(allowed, str(refused.exception))
                    with self.assertRaisesRegex(SystemExit, "do not say whether") as refused:
                        nightly.stable_mod_api(unjudged, tag, self.notes)
                    self.assertIn(unclassified, str(refused.exception))
                    self.assertNotIn(tests, str(refused.exception))
                    with self.assertRaisesRegex(SystemExit, "decodes data in files marked") as refused:
                        nightly.stable_mod_api(decoding, tag, self.notes)
                    self.assertIn(unclassified, str(refused.exception))
                    self.assertNotIn(runtime, str(refused.exception))
            nightly.stable_mod_api(schema_edit, "26.3-1.0.0", self.notes)

    def test_this_tree_classifies_its_content_files(self):
        # What a release would refuse is refused on the pull request that adds it.
        self.assertTrue(nightly.content_readers("HEAD", "this tree"))

    def test_stable_mod_api_command_requires_a_full_commit_sha(self):
        for sha in ("HEAD", HEAD[:12], HEAD.upper()):
            with self.subTest(sha=sha), \
                    mock.patch("sys.argv", ["nightly.py", "stable-mod-api", sha, PREVIOUS, str(self.notes)]), \
                    mock.patch("sys.stderr", new_callable=io.StringIO) as stderr, \
                    mock.patch.object(nightly, "stable_mod_api") as compare:
                with self.assertRaises(SystemExit):
                    nightly.main()
                self.assertIn("sha must be a full lowercase commit SHA", stderr.getvalue())
                compare.assert_not_called()

    def test_stable_sdk_rejects_non_integer_index_schema(self):
        assets = sdk_assets(OLD, SURFACE, "1", {})
        index = json.loads(assets[nightly.SDK_INDEX])
        for schema in (True, 1.0, "1"):
            with self.subTest(schema=schema):
                assets[nightly.SDK_INDEX] = json.dumps(index | {"schema": schema}).encode()
                with mock.patch.object(nightly, "released_assets", return_value=assets), \
                        self.assertRaisesRegex(SystemExit, "sdk.json has schema"):
                    nightly.stable_sdk_inputs(OLD, None)

    def test_large_notes_keep_every_target_in_a_linked_file(self):
        target = "module::" + "x" * 125_000
        nightly.append_mixin_notes(self.notes, "26.3-0.1.11", PREVIOUS, {}, {target: ("signature", "source")})
        listing = self.notes.with_name("changed-mixin-targets.md")
        self.assertIn(f"- Added: `{target}`", listing.read_text())
        self.assertIn(f"/releases/download/26.3-0.1.11/{listing.name}", self.notes.read_text())


class JudgeBuildTests(unittest.TestCase):
    def test_the_parents_stage_is_built_from_its_tree_alone(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            with mock.patch.dict(nightly.os.environ, {"CARGO_TARGET_DIR": "/elsewhere", "RUSTC_WRAPPER": "sccache",
                                                      "GH_TOKEN": "secret", "GITHUB_ENV": "/env", "GITHUB_PATH": "/path"}), \
                    mock.patch.object(nightly, "unpack") as unpack, \
                    mock.patch.object(nightly.subprocess, "run") as run:
                tool = nightly.parent_stage(OLD, work)
            unpack.assert_called_once_with(OLD, work / "judge")
            args, kwargs = run.call_args
            self.assertEqual(kwargs["cwd"], work / "judge", "no .cargo/config.toml of the candidate's checkout")
            self.assertEqual(args[0][args[0].index("--target-dir") + 1], str(work / "judge-target"))
            self.assertFalse({"CARGO_TARGET_DIR", "RUSTC_WRAPPER", "GH_TOKEN", "GITHUB_ENV", "GITHUB_PATH"}
                             & kwargs["env"].keys(), "nothing a later step reads can be written")
            self.assertEqual(tool, str(work / "judge-target" / "debug" / "sunrise-stage"))
            # The tree is a plain extraction: the checkout gains no worktree.
            repo = work / "repo"
            repo.mkdir()
            with chdir(repo), mock.patch.object(nightly, "REPOSITORY", repo):
                git("init", "-q")
                Path("a.txt").write_text("a\n")
                git("add", "a.txt")
                git("-c", "user.name=T", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "a")
                nightly.unpack("HEAD", work / "tree")
                self.assertEqual((work / "tree" / "a.txt").read_text(), "a\n")
                self.assertEqual(len(git("worktree", "list").splitlines()), 1)

    def test_download_only_returns_assets_verified_under_their_tag(self):
        files = {nightly.SDK_INDEX: b"index", nightly.SDK_SURFACE: b"surface"}

        def download(*args):
            folder = Path(args[args.index("--dir") + 1])
            for name, data in files.items():
                (folder / name).write_bytes(data)

        with mock.patch.object(nightly, "gh", side_effect=download) as fetched, \
                mock.patch.object(nightly, "verify_sums") as verify:
            self.assertEqual(nightly.released_assets(HEAD, *files), files)
            self.assertEqual(fetched.call_args.args[2], f"sdk-{HEAD}")
            self.assertEqual((verify.call_args.args[1:]), (f"sdk-{HEAD}", tuple(files)))
            nightly.release_assets("26.3-0.1.0", nightly.SDK_INDEX)
            self.assertEqual(verify.call_args.args[1], "26.3-0.1.0", "a stable release's files, under its own tag")
            verify.side_effect = SystemExit("not signed")
            with self.assertRaisesRegex(SystemExit, "not signed"):
                nightly.released_assets(HEAD, *files)
