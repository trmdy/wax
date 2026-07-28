from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import sys
import tarfile
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("corpus_tool.py")
SPEC = importlib.util.spec_from_file_location("corpus_tool", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
corpus_tool = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = corpus_tool
SPEC.loader.exec_module(corpus_tool)


class ManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        payload = self.repo / "corpus" / "files" / "fixture" / "book.xlsx"
        payload.parent.mkdir(parents=True)
        payload.write_bytes(b"PK fixture")
        self.payload = payload
        self.entry = {
            "id": "fixture/book.xlsx",
            "path": "corpus/files/fixture/book.xlsx",
            "sha256": hashlib.sha256(b"PK fixture").hexdigest(),
            "bytes": len(b"PK fixture"),
            "ext": "xlsx",
            "collection": "fixture",
            "source": "https://example.invalid/book.xlsx",
            "licence": "Apache-2.0",
            "fetchedAt": "2026-07-27T20:00:00Z",
            "private": False,
        }

    def test_valid_entry_and_summary(self) -> None:
        self.assertEqual(
            corpus_tool.validate_manifest([self.entry], self.repo), []
        )
        summary = corpus_tool.build_summary([self.entry])
        self.assertEqual(summary["totalCount"], 1)
        self.assertEqual(summary["publicCount"], 1)
        self.assertEqual(summary["uniquePublicSha256Count"], 1)
        self.assertEqual(summary["collections"]["fixture"]["bytes"], 10)
        self.assertEqual(summary["extensions"], {"xlsx": 1})

    def test_rejects_unsorted_duplicate_and_extra_keys(self) -> None:
        second = dict(self.entry)
        second["id"] = "aaa/book.xlsx"
        second["path"] = "corpus/files/aaa/book.xlsx"
        second["unexpected"] = True
        errors = corpus_tool.validate_manifest(
            [self.entry, second, dict(second)], self.repo
        )
        joined = "\n".join(errors)
        self.assertIn("not sorted", joined)
        self.assertIn("duplicate ids", joined)
        self.assertIn("extra keys", joined)

    def test_malformed_types_report_errors_without_throwing(self) -> None:
        malformed = dict(self.entry)
        malformed.update(
            {
                "id": None,
                "path": [],
                "sha256": None,
                "bytes": "ten",
                "ext": [],
                "source": None,
                "fetchedAt": None,
                "private": "no",
            }
        )
        errors = corpus_tool.validate_manifest([malformed], self.repo)
        self.assertGreaterEqual(len(errors), 8)

    def test_verify_malformed_manifest_returns_failure(self) -> None:
        manifest = self.repo / "corpus" / "manifest.jsonl"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        malformed = dict(self.entry)
        malformed["private"] = "no"
        manifest.write_text(json.dumps(malformed) + "\n", encoding="utf-8")
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            self.assertEqual(corpus_tool.verify(self.repo), 1)

    def test_private_path_must_be_absolute_and_external(self) -> None:
        private = dict(self.entry)
        private.update(
            {
                "id": "spike-local/001/book.xlsx",
                "path": "relative/book.xlsx",
                "collection": "spike-local",
                "source": "https://example.invalid/private.xlsx",
                "licence": "Private-local-only",
                "private": True,
            }
        )
        errors = corpus_tool.validate_entry(private, self.repo)
        self.assertTrue(any("must be absolute" in error for error in errors))
        self.assertTrue(any("must be a file URL" in error for error in errors))

    def test_public_path_traversal_is_rejected(self) -> None:
        traversal = dict(self.entry)
        traversal.update(
            {
                "id": "../book.xlsx",
                "path": "corpus/files/../../private/book.xlsx",
            }
        )
        errors = corpus_tool.validate_entry(traversal, self.repo)
        self.assertTrue(any("safe relative path" in error for error in errors))
        self.assertTrue(
            any("repo-relative under corpus/files/" in error for error in errors)
        )

    def test_write_outputs_is_stable_and_verify_detects_tamper(self) -> None:
        manifest = self.repo / "corpus" / "manifest.jsonl"
        summary = self.repo / "corpus" / "manifest-summary.json"
        corpus_tool.write_outputs(
            [self.entry], manifest, summary, self.repo
        )
        first_manifest = manifest.read_bytes()
        first_summary = summary.read_bytes()
        corpus_tool.write_outputs(
            [self.entry], manifest, summary, self.repo
        )
        self.assertEqual(manifest.read_bytes(), first_manifest)
        self.assertEqual(summary.read_bytes(), first_summary)
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            self.assertEqual(corpus_tool.verify(self.repo), 0)
        self.payload.write_bytes(b"tampered")
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            self.assertEqual(corpus_tool.verify(self.repo), 1)

    def test_summary_file_shape_is_json(self) -> None:
        summary = corpus_tool.build_summary([self.entry])
        encoded = json.dumps(summary)
        self.assertEqual(json.loads(encoded), summary)

    def test_add_file_accepts_a_precomputed_hash(self) -> None:
        fetcher = corpus_tool.Fetcher(self.repo, min_public=0, sec_target=0)
        original = corpus_tool.sha256_file
        corpus_tool.sha256_file = lambda _path: self.fail("unexpected rehash")
        self.addCleanup(setattr, corpus_tool, "sha256_file", original)
        added = fetcher.add_file(
            entry_id=self.entry["id"],
            manifest_path=self.entry["path"],
            file_path=self.payload,
            collection=self.entry["collection"],
            source=self.entry["source"],
            licence=self.entry["licence"],
            precomputed_sha256=self.entry["sha256"],
        )
        self.assertTrue(added)
        self.assertEqual(
            fetcher.entries[self.entry["id"]]["sha256"], self.entry["sha256"]
        )

    def test_add_file_preserves_fetch_time_for_unchanged_payload(self) -> None:
        manifest = self.repo / "corpus" / "manifest.jsonl"
        summary = self.repo / "corpus" / "manifest-summary.json"
        corpus_tool.write_outputs([self.entry], manifest, summary, self.repo)
        fetcher = corpus_tool.Fetcher(self.repo, min_public=0, sec_target=0)
        fetcher.started_at = "2026-07-28T03:00:00Z"
        fetcher.add_file(
            entry_id=self.entry["id"],
            manifest_path=self.entry["path"],
            file_path=self.payload,
            collection=self.entry["collection"],
            source=self.entry["source"],
            licence=self.entry["licence"],
            precomputed_sha256=self.entry["sha256"],
        )
        self.assertEqual(
            fetcher.entries[self.entry["id"]]["fetchedAt"],
            self.entry["fetchedAt"],
        )

    def test_sec_candidate_discovery_is_filtered_and_deterministic(self) -> None:
        fetcher = corpus_tool.Fetcher(self.repo, min_public=0, sec_target=0)
        index = self.repo / "master.idx"
        index.write_text(
            "CIK|Company Name|Form Type|Date Filed|Filename\n"
            "100|First|10-Q|2025-04-01|edgar/data/100/0000000100-25-000001.txt\n"
            "200|Ignored|8-K|2025-04-02|edgar/data/200/0000000200-25-000002.txt\n"
            "300|Third|10-K|2025-04-03|edgar/data/300/0000000300-25-000003.txt\n",
            encoding="latin-1",
        )
        self.assertEqual(
            fetcher.sec_candidates(index),
            [
                "https://www.sec.gov/Archives/edgar/data/100/"
                "000000010025000001/Financial_Report.xlsx",
                "https://www.sec.gov/Archives/edgar/data/300/"
                "000000030025000003/Financial_Report.xlsx",
            ],
        )

    def test_archive_traversal_is_rejected(self) -> None:
        fetcher = corpus_tool.Fetcher(self.repo, min_public=0, sec_target=0)
        archive = self.repo / "malicious.tar.gz"
        source = self.repo / "payload"
        source.write_bytes(b"escape")
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(source, arcname="../escape")
        with self.assertRaises(corpus_tool.CorpusError):
            fetcher.safe_extract_tar(archive, self.repo / "extract")
        self.assertFalse((self.repo.parent / "escape").exists())

    def test_collection_copy_ignores_symlinked_payloads(self) -> None:
        fetcher = corpus_tool.Fetcher(self.repo, min_public=0, sec_target=0)
        source = self.repo / "source"
        source.mkdir()
        (source / "real.xlsx").write_bytes(b"PK real")
        outside = self.repo / "outside.xlsx"
        outside.write_bytes(b"private")
        (source / "linked.xlsx").symlink_to(outside)
        fetcher.copy_collection_tree(
            source_root=source,
            destination_root=self.repo / "corpus" / "files" / "copy",
            id_root=corpus_tool.PurePosixPath("copy"),
            path_root=corpus_tool.PurePosixPath("corpus/files/copy"),
            collection="copy",
            source_root_url="https://example.invalid",
            licence="Apache-2.0",
        )
        self.assertEqual(list(fetcher.entries), ["copy/real.xlsx"])
        self.assertFalse(
            (self.repo / "corpus" / "files" / "copy" / "linked.xlsx").exists()
        )


if __name__ == "__main__":
    unittest.main()
