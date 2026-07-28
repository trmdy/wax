#!/usr/bin/env python3
"""Fetch, build, and verify the wax spreadsheet corpus.

Only the Python standard library and common command-line tools (git and curl)
are required. Payloads and source caches live below corpus/files/, which is
gitignored. The manifest is deterministic across no-op reruns: fetchedAt is
retained for an unchanged id+sha256 pair and the summary timestamp is derived
from the manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any, Iterable


USER_AGENT = "wax-corpus-fetch/1.0 (tormod.haugland@gmail.com)"
ALLOWED_EXTENSIONS = frozenset(
    {"xlsx", "xlsm", "xlsb", "xls", "ods", "csv", "tsv"}
)
MAX_FILE_BYTES = 30 * 1024 * 1024
MAX_CORPUS_BYTES = 4 * 1024 * 1024 * 1024
MIN_PUBLIC_FILES = 1_000
SEC_TARGET_FILES = 24

MANIFEST_KEYS = frozenset(
    {
        "id",
        "path",
        "sha256",
        "bytes",
        "ext",
        "collection",
        "source",
        "licence",
        "fetchedAt",
        "private",
    }
)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
FETCHED_AT_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")

SHEETJS_SITE_REPO = "https://github.com/SheetJS/SheetJS.github.io.git"
SHEETJS_SITE_BRANCH = "master"
SHEETJS_SITE_COMMIT = "7d4614945c6a652421b66aa536fd0140a3ff3e4f"
SHEETJS_SITE_RAW = (
    "https://raw.githubusercontent.com/SheetJS/SheetJS.github.io/"
    f"{SHEETJS_SITE_COMMIT}/test_files"
)

POI_REPO = "https://github.com/apache/poi.git"
POI_BRANCH = "trunk"
POI_COMMIT = "0c5d8675e124cdfb4c147963135c9ba35fcfb009"
POI_RAW = (
    f"https://raw.githubusercontent.com/apache/poi/{POI_COMMIT}"
    "/test-data/spreadsheet"
)

OPENPYXL_VERSION = "3.1.5"
OPENPYXL_ARCHIVE_URL = (
    "https://foss.heptapod.net/openpyxl/openpyxl/-/archive/"
    f"{OPENPYXL_VERSION}/openpyxl-{OPENPYXL_VERSION}.tar.gz"
)
OPENPYXL_ARCHIVE_SHA256 = (
    "64a599aeed98b74925dcc09a18c7b3e19dafb3754eb8bad2b6887b63a91f7a37"
)

SEC_INDEX_URL = (
    "https://www.sec.gov/Archives/edgar/full-index/2025/QTR2/master.idx"
)
SEC_ARCHIVE_ROOT = "https://www.sec.gov/Archives"

SPIKE_SCRIPT = Path(os.environ.get("WAX_SPIKE_SCRIPT", ""))  # local-only overlay; unset in public checkouts


class CorpusError(RuntimeError):
    """An actionable corpus fetch or validation failure."""


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def extension(path: Path | PurePosixPath) -> str:
    return path.suffix.removeprefix(".").lower()


def quote_path(path: str) -> str:
    return urllib.parse.quote(path, safe="/")


def run(
    args: list[str],
    *,
    cwd: Path | None = None,
    capture: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        check=check,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )


def load_manifest(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    entries: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as stream:
        for line_number, line in enumerate(stream, 1):
            if not line.strip():
                raise CorpusError(f"{path}:{line_number}: blank lines are not allowed")
            try:
                value = json.loads(line)
            except json.JSONDecodeError as error:
                raise CorpusError(f"{path}:{line_number}: {error}") from error
            if not isinstance(value, dict):
                raise CorpusError(
                    f"{path}:{line_number}: manifest line must be an object"
                )
            entries.append(value)
    return entries


def validate_entry(entry: dict[str, Any], repo_root: Path) -> list[str]:
    errors: list[str] = []
    entry_id = entry.get("id", "<missing>")
    missing = MANIFEST_KEYS - entry.keys()
    extra = entry.keys() - MANIFEST_KEYS
    if missing:
        errors.append(f"{entry_id}: missing keys: {', '.join(sorted(missing))}")
    if extra:
        errors.append(f"{entry_id}: extra keys: {', '.join(sorted(extra))}")
    if missing:
        return errors

    string_fields = (
        "id",
        "path",
        "sha256",
        "ext",
        "collection",
        "source",
        "licence",
        "fetchedAt",
    )
    for field in string_fields:
        if not isinstance(entry[field], str) or not entry[field]:
            errors.append(f"{entry_id}: {field} must be a non-empty string")

    byte_count = entry["bytes"]
    if not isinstance(byte_count, int) or isinstance(byte_count, bool):
        errors.append(f"{entry_id}: bytes must be an integer")
    elif byte_count < 0 or byte_count > MAX_FILE_BYTES:
        errors.append(
            f"{entry_id}: bytes must be between 0 and {MAX_FILE_BYTES}"
        )
    is_private = entry["private"]
    if not isinstance(is_private, bool):
        errors.append(f"{entry_id}: private must be a boolean")
    digest = entry["sha256"]
    if not isinstance(digest, str) or not SHA256_RE.fullmatch(digest):
        errors.append(f"{entry_id}: sha256 must be 64 lowercase hex characters")
    ext = entry["ext"]
    if not isinstance(ext, str) or ext not in ALLOWED_EXTENSIONS:
        errors.append(f"{entry_id}: unsupported extension {ext!r}")
    id_value = entry["id"]
    if isinstance(id_value, str) and isinstance(ext, str):
        id_path = PurePosixPath(id_value)
        if id_path.is_absolute() or ".." in id_path.parts:
            errors.append(f"{entry_id}: id must be a safe relative path")
        if extension(id_path) != ext:
            errors.append(f"{entry_id}: id extension does not match ext")
    path_value = entry["path"]
    if isinstance(path_value, str) and isinstance(ext, str):
        if extension(Path(path_value)) != ext:
            errors.append(f"{entry_id}: path extension does not match ext")
    fetched_at = entry["fetchedAt"]
    if not isinstance(fetched_at, str) or not FETCHED_AT_RE.fullmatch(fetched_at):
        errors.append(f"{entry_id}: fetchedAt must be UTC RFC3339 seconds")

    if not isinstance(path_value, str) or not isinstance(is_private, bool):
        return errors
    path = Path(path_value)
    source = entry["source"]
    if is_private:
        if not path.is_absolute():
            errors.append(f"{entry_id}: private path must be absolute")
        try:
            path.resolve(strict=False).relative_to(
                (repo_root / "corpus" / "files").resolve()
            )
            errors.append(f"{entry_id}: private path cannot be under corpus/files")
        except ValueError:
            pass
        if not isinstance(source, str) or not source.startswith("file://"):
            errors.append(f"{entry_id}: private source must be a file URL")
    else:
        expected_prefix = "corpus/files/"
        posix_path = PurePosixPath(path_value)
        if (
            path.is_absolute()
            or ".." in posix_path.parts
            or not path_value.startswith(expected_prefix)
        ):
            errors.append(
                f"{entry_id}: public path must be repo-relative under {expected_prefix}"
            )
        if not isinstance(source, str) or not source.startswith(
            ("https://", "http://")
        ):
            errors.append(f"{entry_id}: public source must be an HTTP(S) URL")
    return errors


def validate_manifest(
    entries: list[dict[str, Any]], repo_root: Path
) -> list[str]:
    errors: list[str] = []
    ids = [entry.get("id") for entry in entries]
    if all(isinstance(entry_id, str) for entry_id in ids) and ids != sorted(ids):
        errors.append("manifest entries are not sorted by id")
    duplicate_ids = sorted(
        entry_id
        for entry_id, count in Counter(
            entry_id for entry_id in ids if isinstance(entry_id, str)
        ).items()
        if count > 1
    )
    if duplicate_ids:
        errors.append(f"duplicate ids: {', '.join(map(str, duplicate_ids))}")
    for entry in entries:
        errors.extend(validate_entry(entry, repo_root))
    public_bytes = sum(
        byte_count
        for entry in entries
        if entry.get("private") is False
        and isinstance((byte_count := entry.get("bytes")), int)
        and not isinstance(byte_count, bool)
    )
    if public_bytes > MAX_CORPUS_BYTES:
        errors.append(
            f"public corpus is {public_bytes} bytes, above {MAX_CORPUS_BYTES}"
        )
    return errors


def build_summary(entries: list[dict[str, Any]]) -> dict[str, Any]:
    collections: dict[str, dict[str, int]] = defaultdict(
        lambda: {"count": 0, "bytes": 0}
    )
    extensions: Counter[str] = Counter()
    for entry in entries:
        bucket = collections[entry["collection"]]
        bucket["count"] += 1
        bucket["bytes"] += entry["bytes"]
        extensions[entry["ext"]] += 1
    public_entries = [entry for entry in entries if not entry["private"]]
    generated_at = max(
        (entry["fetchedAt"] for entry in entries), default="1970-01-01T00:00:00Z"
    )
    return {
        "generatedAt": generated_at,
        "totalCount": len(entries),
        "publicCount": len(public_entries),
        "privateCount": len(entries) - len(public_entries),
        "uniquePublicSha256Count": len(
            {entry["sha256"] for entry in public_entries}
        ),
        "totalBytes": sum(entry["bytes"] for entry in entries),
        "publicBytes": sum(entry["bytes"] for entry in public_entries),
        "collections": dict(sorted(collections.items())),
        "extensions": dict(sorted(extensions.items())),
    }


def write_text_if_changed(path: Path, text: str) -> None:
    if path.exists() and path.read_text(encoding="utf-8") == text:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(text, encoding="utf-8")
    os.replace(temporary, path)


def write_outputs(
    entries: Iterable[dict[str, Any]], manifest: Path, summary: Path, repo_root: Path
) -> None:
    ordered = sorted(entries, key=lambda entry: entry["id"])
    errors = validate_manifest(ordered, repo_root)
    if errors:
        raise CorpusError("\n".join(errors))
    manifest_text = "".join(
        json.dumps(entry, ensure_ascii=False, separators=(",", ":")) + "\n"
        for entry in ordered
    )
    summary_text = json.dumps(
        build_summary(ordered), ensure_ascii=False, indent=2, sort_keys=False
    )
    write_text_if_changed(manifest, manifest_text)
    write_text_if_changed(summary, summary_text + "\n")


@dataclass
class CollectionResult:
    count: int = 0
    bytes: int = 0
    skipped_large: int = 0
    skipped_duplicate_id: int = 0


class Fetcher:
    def __init__(self, repo_root: Path, *, min_public: int, sec_target: int):
        self.repo_root = repo_root
        self.corpus_dir = repo_root / "corpus"
        self.files_dir = self.corpus_dir / "files"
        self.sources_dir = self.files_dir / ".sources"
        self.manifest_path = self.corpus_dir / "manifest.jsonl"
        self.summary_path = self.corpus_dir / "manifest-summary.json"
        self.log_path = self.corpus_dir / "fetch.log"
        self.min_public = min_public
        self.sec_target = sec_target
        old_entries = load_manifest(self.manifest_path)
        old_errors = validate_manifest(old_entries, repo_root)
        if old_errors:
            raise CorpusError("existing manifest is invalid:\n" + "\n".join(old_errors))
        self.old_by_id = {entry["id"]: entry for entry in old_entries}
        self.entries: dict[str, dict[str, Any]] = {}
        self.results: dict[str, CollectionResult] = defaultdict(CollectionResult)
        self.started_at = utc_now()
        self.last_http_request: dict[str, float] = {}

    def log(self, message: str) -> None:
        line = f"{utc_now()} {message}"
        print(line, file=sys.stderr)
        self.corpus_dir.mkdir(parents=True, exist_ok=True)
        with self.log_path.open("a", encoding="utf-8") as stream:
            stream.write(line + "\n")

    def rate_limit(self, url: str) -> None:
        host = urllib.parse.urlparse(url).hostname
        if not host:
            raise CorpusError(f"URL has no host: {url}")
        elapsed = time.monotonic() - self.last_http_request.get(host, 0.0)
        if elapsed < 0.5:
            time.sleep(0.5 - elapsed)
        self.last_http_request[host] = time.monotonic()

    def download(
        self,
        url: str,
        destination: Path,
        *,
        expected_sha256: str | None = None,
        required: bool = True,
    ) -> Path | None:
        if destination.exists():
            if expected_sha256 is None or sha256_file(destination) == expected_sha256:
                self.log(f"reuse download sha256-ok path={destination} source={url}")
                return destination
            self.log(f"replace checksum-mismatch path={destination} source={url}")

        destination.parent.mkdir(parents=True, exist_ok=True)
        part = destination.with_name(destination.name + ".part")
        self.rate_limit(url)
        self.log(f"download begin source={url} destination={destination}")
        retry_count = "5" if required else "1"
        retry_max_time = "300" if required else "20"
        result = run(
            [
                "curl",
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--continue-at",
                "-",
                "--retry",
                retry_count,
                "--retry-all-errors",
                "--retry-max-time",
                retry_max_time,
                "--connect-timeout",
                "20",
                "--max-time",
                "600",
                "--user-agent",
                USER_AGENT,
                "--output",
                str(part),
                url,
            ],
            capture=True,
            check=False,
        )
        if result.returncode != 0:
            self.log(
                f"download failed exit={result.returncode} source={url} "
                f"error={result.stderr.strip()!r}"
            )
            if required:
                raise CorpusError(f"download failed: {url}: {result.stderr.strip()}")
            return None
        actual_sha256 = sha256_file(part)
        if expected_sha256 and actual_sha256 != expected_sha256:
            raise CorpusError(
                f"checksum mismatch for {url}: expected {expected_sha256}, "
                f"got {actual_sha256}"
            )
        os.replace(part, destination)
        self.log(
            f"download complete bytes={destination.stat().st_size} "
            f"sha256={actual_sha256} source={url}"
        )
        return destination

    def git_checkout(
        self,
        *,
        name: str,
        repo: str,
        branch: str,
        commit: str,
        sparse_path: str,
    ) -> Path:
        checkout = self.sources_dir / "git" / name
        if not (checkout / ".git").is_dir():
            checkout.parent.mkdir(parents=True, exist_ok=True)
            staging = checkout.with_name(checkout.name + ".cloning")
            if staging.exists():
                staging = checkout.with_name(
                    f"{checkout.name}.cloning-{int(time.time())}"
                )
            self.log(
                f"git shallow-clone begin repo={repo} branch={branch} "
                f"sparse={sparse_path}"
            )
            self.rate_limit(repo)
            result = run(
                [
                    "git",
                    "-c",
                    f"http.userAgent={USER_AGENT}",
                    "clone",
                    "--depth",
                    "1",
                    "--filter=blob:none",
                    "--sparse",
                    "--no-checkout",
                    "--branch",
                    branch,
                    repo,
                    str(staging),
                ],
                capture=True,
                check=False,
            )
            if result.returncode != 0:
                raise CorpusError(
                    f"shallow clone failed for {repo}: {result.stderr.strip()}"
                )
            os.replace(staging, checkout)
            self.log(f"git shallow-clone complete repo={repo} path={checkout}")

        exists = run(
            ["git", "-C", str(checkout), "cat-file", "-e", f"{commit}^{{commit}}"],
            capture=True,
            check=False,
        )
        if exists.returncode != 0:
            self.log(f"git fetch pinned commit={commit} repo={repo}")
            self.rate_limit(repo)
            run(
                [
                    "git",
                    "-c",
                    f"http.userAgent={USER_AGENT}",
                    "-C",
                    str(checkout),
                    "fetch",
                    "--depth",
                    "1",
                    "origin",
                    commit,
                ]
            )
        run(
            [
                "git",
                "-C",
                str(checkout),
                "sparse-checkout",
                "set",
                sparse_path,
            ]
        )
        run(["git", "-C", str(checkout), "checkout", "--detach", commit])
        actual = run(
            ["git", "-C", str(checkout), "rev-parse", "HEAD"], capture=True
        ).stdout.strip()
        if actual != commit:
            raise CorpusError(
                f"{name} checkout mismatch: expected {commit}, got {actual}"
            )
        self.log(f"git checkout ready repo={repo} commit={commit}")
        return checkout

    def add_file(
        self,
        *,
        entry_id: str,
        manifest_path: str,
        file_path: Path,
        collection: str,
        source: str,
        licence: str,
        private: bool = False,
        precomputed_sha256: str | None = None,
    ) -> bool:
        file_ext = extension(file_path)
        if file_ext not in ALLOWED_EXTENSIONS:
            return False
        size = file_path.stat().st_size
        result = self.results[collection]
        if size > MAX_FILE_BYTES:
            result.skipped_large += 1
            self.log(
                f"skip too-large bytes={size} limit={MAX_FILE_BYTES} "
                f"source={source}"
            )
            return False
        if entry_id in self.entries:
            result.skipped_duplicate_id += 1
            self.log(f"skip duplicate-id id={entry_id} source={source}")
            return False
        digest = precomputed_sha256 or sha256_file(file_path)
        old = self.old_by_id.get(entry_id)
        fetched_at = (
            old["fetchedAt"]
            if old is not None and old.get("sha256") == digest
            else self.started_at
        )
        entry = {
            "id": entry_id,
            "path": manifest_path,
            "sha256": digest,
            "bytes": size,
            "ext": file_ext,
            "collection": collection,
            "source": source,
            "licence": licence,
            "fetchedAt": fetched_at,
            "private": private,
        }
        errors = validate_entry(entry, self.repo_root)
        if errors:
            raise CorpusError("\n".join(errors))
        self.entries[entry_id] = entry
        result.count += 1
        result.bytes += size
        return True

    def copy_collection_tree(
        self,
        *,
        source_root: Path,
        destination_root: Path,
        id_root: PurePosixPath,
        path_root: PurePosixPath,
        collection: str,
        source_root_url: str,
        licence: str,
    ) -> None:
        for source_file in sorted(
            (
                path
                for path in source_root.rglob("*")
                if path.is_file() and not path.is_symlink()
            ),
            key=lambda path: path.as_posix(),
        ):
            relative = source_file.relative_to(source_root)
            if extension(relative) not in ALLOWED_EXTENSIONS:
                continue
            if source_file.stat().st_size > MAX_FILE_BYTES:
                self.results[collection].skipped_large += 1
                self.log(
                    f"skip too-large bytes={source_file.stat().st_size} "
                    f"source={source_root_url}/{quote_path(relative.as_posix())}"
                )
                continue
            destination = destination_root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            source_sha = sha256_file(source_file)
            if not destination.exists() or sha256_file(destination) != source_sha:
                with tempfile.NamedTemporaryFile(
                    dir=destination.parent,
                    prefix=f".{destination.name}.",
                    delete=False,
                ) as temporary:
                    temporary_path = Path(temporary.name)
                shutil.copyfile(source_file, temporary_path)
                os.replace(temporary_path, destination)
            self.add_file(
                entry_id=(id_root / relative.as_posix()).as_posix(),
                manifest_path=(path_root / relative.as_posix()).as_posix(),
                file_path=destination,
                collection=collection,
                source=f"{source_root_url}/{quote_path(relative.as_posix())}",
                licence=licence,
            )

    def fetch_sheetjs(self) -> None:
        checkout = self.git_checkout(
            name="sheetjs-site",
            repo=SHEETJS_SITE_REPO,
            branch=SHEETJS_SITE_BRANCH,
            commit=SHEETJS_SITE_COMMIT,
            sparse_path="test_files",
        )
        source_root = checkout / "test_files"
        if not source_root.is_dir():
            raise CorpusError(f"SheetJS test_files missing from {checkout}")
        self.copy_collection_tree(
            source_root=source_root,
            destination_root=self.files_dir / "sheetjs" / "test_files",
            id_root=PurePosixPath("sheetjs/test_files"),
            path_root=PurePosixPath("corpus/files/sheetjs/test_files"),
            collection="sheetjs-test-files",
            source_root_url=SHEETJS_SITE_RAW,
            licence="Apache-2.0",
        )

    def fetch_poi(self) -> None:
        checkout = self.git_checkout(
            name="apache-poi",
            repo=POI_REPO,
            branch=POI_BRANCH,
            commit=POI_COMMIT,
            sparse_path="test-data/spreadsheet",
        )
        source_root = checkout / "test-data" / "spreadsheet"
        if not source_root.is_dir():
            raise CorpusError(f"POI spreadsheet test data missing from {checkout}")
        self.copy_collection_tree(
            source_root=source_root,
            destination_root=self.files_dir / "poi" / "test-data" / "spreadsheet",
            id_root=PurePosixPath("poi/test-data/spreadsheet"),
            path_root=PurePosixPath(
                "corpus/files/poi/test-data/spreadsheet"
            ),
            collection="poi-test-data",
            source_root_url=POI_RAW,
            licence="Apache-2.0",
        )

    def safe_extract_tar(self, archive: Path, destination: Path) -> None:
        marker = destination / ".wax-extracted-sha256"
        archive_sha = sha256_file(archive)
        if marker.exists() and marker.read_text(encoding="utf-8").strip() == archive_sha:
            self.log(f"reuse extracted archive={archive} destination={destination}")
            return
        destination.mkdir(parents=True, exist_ok=True)
        root = destination.resolve()
        with tarfile.open(archive, "r:gz") as bundle:
            for member in bundle.getmembers():
                member_path = (destination / member.name).resolve()
                try:
                    member_path.relative_to(root)
                except ValueError as error:
                    raise CorpusError(
                        f"unsafe archive member {member.name!r} in {archive}"
                    ) from error
            bundle.extractall(destination, filter="data")
        marker.write_text(archive_sha + "\n", encoding="utf-8")
        self.log(f"archive extracted source={archive} destination={destination}")

    def fetch_openpyxl(self) -> None:
        archive = self.sources_dir / "archives" / f"openpyxl-{OPENPYXL_VERSION}.tar.gz"
        downloaded = self.download(
            OPENPYXL_ARCHIVE_URL,
            archive,
            expected_sha256=OPENPYXL_ARCHIVE_SHA256,
        )
        assert downloaded is not None
        extracted = self.sources_dir / "extracted" / f"openpyxl-{OPENPYXL_VERSION}"
        self.safe_extract_tar(downloaded, extracted)
        source_root = extracted / f"openpyxl-{OPENPYXL_VERSION}"
        if not source_root.is_dir():
            raise CorpusError(f"OpenPyXL archive root missing from {extracted}")
        self.copy_collection_tree(
            source_root=source_root,
            destination_root=self.files_dir / "openpyxl" / OPENPYXL_VERSION,
            id_root=PurePosixPath("openpyxl", OPENPYXL_VERSION),
            path_root=PurePosixPath(
                "corpus/files/openpyxl", OPENPYXL_VERSION
            ),
            collection="openpyxl-test-data",
            source_root_url=f"{OPENPYXL_ARCHIVE_URL}#path=openpyxl-{OPENPYXL_VERSION}",
            licence="MIT",
        )

    def sec_entry_from_file(self, source: str, file_path: Path) -> None:
        parsed = urllib.parse.urlparse(source)
        parts = PurePosixPath(parsed.path).parts
        try:
            data_index = parts.index("data")
            cik = parts[data_index + 1]
            accession = parts[data_index + 2]
        except (ValueError, IndexError) as error:
            raise CorpusError(f"unexpected SEC URL: {source}") from error
        destination = self.files_dir / "sec-edgar" / cik / accession / file_path.name
        if file_path != destination:
            destination.parent.mkdir(parents=True, exist_ok=True)
            if not destination.exists() or sha256_file(destination) != sha256_file(
                file_path
            ):
                shutil.copyfile(file_path, destination)
        self.add_file(
            entry_id=f"sec-edgar/{cik}/{accession}/{destination.name}",
            manifest_path=destination.relative_to(self.repo_root).as_posix(),
            file_path=destination,
            collection="sec-edgar",
            source=source,
            licence="US-PD",
        )

    def fetch_sec_existing(self) -> int:
        existing = sorted(
            (
                entry
                for entry in self.old_by_id.values()
                if entry["collection"] == "sec-edgar" and not entry["private"]
            ),
            key=lambda entry: entry["id"],
        )
        fetched = 0
        for entry in existing:
            destination = self.repo_root / entry["path"]
            downloaded = self.download(
                entry["source"],
                destination,
                expected_sha256=entry["sha256"],
                required=True,
            )
            assert downloaded is not None
            self.sec_entry_from_file(entry["source"], downloaded)
            fetched += 1
        return fetched

    def sec_candidates(self, index: Path) -> list[str]:
        candidates: list[str] = []
        for line in index.read_text(encoding="latin-1").splitlines():
            parts = line.split("|")
            if len(parts) != 5 or parts[2] not in {"10-K", "10-Q"}:
                continue
            cik = parts[0]
            filing_path = PurePosixPath(parts[4])
            accession = filing_path.stem.replace("-", "")
            candidates.append(
                f"{SEC_ARCHIVE_ROOT}/edgar/data/{cik}/{accession}/"
                "Financial_Report.xlsx"
            )
        return candidates

    def fetch_sec(self) -> None:
        if self.sec_target <= 0:
            return
        fetched = self.fetch_sec_existing()
        if fetched >= self.sec_target:
            return
        index_path = self.sources_dir / "sec-edgar" / "2025-QTR2-master.idx"
        downloaded_index = self.download(SEC_INDEX_URL, index_path)
        assert downloaded_index is not None
        existing_sources = {
            entry["source"]
            for entry in self.entries.values()
            if entry["collection"] == "sec-edgar"
        }
        for source in self.sec_candidates(downloaded_index):
            if fetched >= self.sec_target:
                break
            if source in existing_sources:
                continue
            parsed = urllib.parse.urlparse(source)
            parts = PurePosixPath(parsed.path).parts
            cik = parts[parts.index("data") + 1]
            accession = parts[parts.index("data") + 2]
            destination = (
                self.files_dir
                / "sec-edgar"
                / cik
                / accession
                / "Financial_Report.xlsx"
            )
            downloaded = self.download(
                source, destination, expected_sha256=None, required=False
            )
            if downloaded is None:
                continue
            if downloaded.read_bytes()[:2] != b"PK":
                raise CorpusError(f"SEC source did not return an XLSX zip: {source}")
            self.sec_entry_from_file(source, downloaded)
            existing_sources.add(source)
            fetched += 1
        if fetched < self.sec_target:
            raise CorpusError(
                f"SEC discovery found only {fetched}/{self.sec_target} files"
            )

    def spike_paths(self) -> list[Path]:
        if not SPIKE_SCRIPT.is_file():
            self.log(f"private spike list absent; skip path={SPIKE_SCRIPT}")
            return []
        pattern = re.compile(
            r'^\s*"([^"]+\.(?:xlsx|xlsm|xlsb|xls|ods|csv|tsv))"\s*$',
            re.IGNORECASE | re.MULTILINE,
        )
        paths: list[Path] = []
        for match in pattern.finditer(SPIKE_SCRIPT.read_text(encoding="utf-8")):
            candidate = Path(match.group(1))
            if not candidate.is_absolute():
                candidate = SPIKE_SCRIPT.parent / candidate
            if candidate.is_file():
                paths.append(candidate.resolve())
            else:
                self.log(f"private spike file absent; skip path={candidate}")
        return paths

    def add_spike_local(self) -> None:
        for index, path in enumerate(self.spike_paths(), 1):
            size = path.stat().st_size
            if size > MAX_FILE_BYTES:
                self.results["spike-local"].skipped_large += 1
                self.log(
                    f"skip private spike too-large bytes={size} "
                    f"limit={MAX_FILE_BYTES} path={path}"
                )
                continue
            self.log(f"hash private spike begin path={path}")
            try:
                result = subprocess.run(
                    ["shasum", "-a", "256", str(path)],
                    check=False,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=5,
                )
            except subprocess.TimeoutExpired:
                self.log(
                    f"skip private spike unreadable-timeout seconds=5 path={path}"
                )
                continue
            if result.returncode != 0:
                self.log(
                    f"skip private spike hash-failed exit={result.returncode} "
                    f"path={path} error={result.stderr.strip()!r}"
                )
                continue
            digest = result.stdout.split(maxsplit=1)[0].lower()
            if not SHA256_RE.fullmatch(digest):
                self.log(
                    f"skip private spike invalid-hash-output path={path} "
                    f"output={result.stdout.strip()!r}"
                )
                continue
            quoted = urllib.parse.quote(path.as_posix(), safe="/")
            self.add_file(
                entry_id=f"spike-local/{index:03d}/{path.name}",
                manifest_path=path.as_posix(),
                file_path=path,
                collection="spike-local",
                source=f"file://{quoted}",
                licence="Private-local-only",
                private=True,
                precomputed_sha256=digest,
            )

    def fetch(self) -> None:
        self.files_dir.mkdir(parents=True, exist_ok=True)
        self.log(
            f"fetch start ua={USER_AGENT!r} max_file_bytes={MAX_FILE_BYTES} "
            "per_host_rate_limit=2req/s"
        )
        self.fetch_sheetjs()
        self.fetch_poi()
        self.fetch_openpyxl()
        self.fetch_sec()
        self.add_spike_local()

        entries = sorted(self.entries.values(), key=lambda entry: entry["id"])
        public = [entry for entry in entries if not entry["private"]]
        unique_public = {entry["sha256"] for entry in public}
        if len(public) < self.min_public:
            raise CorpusError(
                f"public manifest has {len(public)} entries; need {self.min_public}"
            )
        if len(unique_public) < self.min_public:
            raise CorpusError(
                f"public corpus has {len(unique_public)} distinct sha256 payloads; "
                f"need {self.min_public}"
            )
        write_outputs(
            entries, self.manifest_path, self.summary_path, self.repo_root
        )
        summary = build_summary(entries)
        self.log(
            f"fetch complete public={summary['publicCount']} "
            f"unique_public={summary['uniquePublicSha256Count']} "
            f"private={summary['privateCount']} bytes={summary['publicBytes']}"
        )
        for collection, result in sorted(self.results.items()):
            self.log(
                f"collection={collection} count={result.count} bytes={result.bytes} "
                f"skipped_large={result.skipped_large} "
                f"skipped_duplicate_id={result.skipped_duplicate_id}"
            )


def verify(repo_root: Path, *, allow_missing: bool = False) -> int:
    manifest_path = repo_root / "corpus" / "manifest.jsonl"
    entries = load_manifest(manifest_path)
    errors = validate_manifest(entries, repo_root)
    if errors:
        for error in errors:
            print(f"ERROR {error}", file=sys.stderr)
        print(
            f"verify failed entries={len(entries)} manifest_invalid=true",
            file=sys.stderr,
        )
        return 1
    missing_public = 0
    mismatched = 0
    checked = 0
    skipped_private_missing = 0
    for entry in entries:
        path = Path(entry["path"])
        if not path.is_absolute():
            path = repo_root / path
        if not path.is_file():
            if entry["private"]:
                skipped_private_missing += 1
                continue
            missing_public += 1
            if not allow_missing:
                errors.append(f"{entry['id']}: missing public payload {path}")
            continue
        if not entry["private"] and path.is_symlink():
            errors.append(f"{entry['id']}: public payload cannot be a symlink")
            mismatched += 1
            continue
        checked += 1
        actual_size = path.stat().st_size
        if actual_size != entry["bytes"]:
            errors.append(
                f"{entry['id']}: size mismatch: manifest={entry['bytes']} "
                f"actual={actual_size}"
            )
            mismatched += 1
            continue
        actual_sha = sha256_file(path)
        if actual_sha != entry["sha256"]:
            errors.append(
                f"{entry['id']}: sha256 mismatch: manifest={entry['sha256']} "
                f"actual={actual_sha}"
            )
            mismatched += 1
    expected_summary = build_summary(entries)
    summary_path = repo_root / "corpus" / "manifest-summary.json"
    if not summary_path.is_file():
        errors.append(f"missing summary: {summary_path}")
    else:
        try:
            actual_summary = json.loads(summary_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as error:
            errors.append(f"invalid summary JSON: {error}")
        else:
            if actual_summary != expected_summary:
                errors.append("manifest-summary.json does not match manifest")
    if errors:
        for error in errors:
            print(f"ERROR {error}", file=sys.stderr)
        print(
            f"verify failed entries={len(entries)} checked={checked} "
            f"missing_public={missing_public} mismatched={mismatched} "
            f"private_missing_skipped={skipped_private_missing}",
            file=sys.stderr,
        )
        return 1
    print(
        f"verify ok entries={len(entries)} checked={checked} "
        f"missing_public={missing_public} mismatched={mismatched} "
        f"private_missing_skipped={skipped_private_missing}"
    )
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    fetch_parser = subparsers.add_parser("fetch", help="fetch and manifest corpus")
    fetch_parser.add_argument("--min-public", type=int, default=MIN_PUBLIC_FILES)
    fetch_parser.add_argument("--sec-target", type=int, default=SEC_TARGET_FILES)
    verify_parser = subparsers.add_parser("verify", help="validate and hash corpus")
    verify_parser.add_argument(
        "--allow-missing",
        action="store_true",
        help="validate a committed manifest without requiring public payloads",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    repo_root = Path(__file__).resolve().parent.parent
    try:
        if args.command == "fetch":
            if args.min_public < 0 or args.sec_target < 0:
                raise CorpusError("counts must be non-negative")
            Fetcher(
                repo_root,
                min_public=args.min_public,
                sec_target=args.sec_target,
            ).fetch()
            return 0
        return verify(repo_root, allow_missing=args.allow_missing)
    except (CorpusError, OSError, subprocess.CalledProcessError) as error:
        print(f"ERROR {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
