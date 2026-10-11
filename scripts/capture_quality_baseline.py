#!/usr/bin/env python3
"""Capture an immutable source fingerprint without staging or discarding work.

This is an inventory, not a backup or a passing test report. Ignored files and
credential files are never included. Generated quality evidence is excluded so
that a report cannot hash itself. Existing reports are never overwritten.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
SECRET_SUFFIXES = {".pem", ".key", ".pfx", ".p12", ".env"}


def git(*args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(ROOT), *args])


def collect() -> dict:
    entries = []
    paths = sorted(set(git("ls-files", "-z", "--cached", "--others", "--exclude-standard")
                       .decode("utf-8").split("\0")) - {""})
    for relative in paths:
        path = ROOT / relative
        if (relative.startswith("quality/baselines/")
                or path.suffix.lower() in SECRET_SUFFIXES
                or path.name == ".env" or path.name.startswith(".env.")
                or path.is_symlink()):
            continue
        if not path.resolve().is_relative_to(ROOT):
            raise ValueError("source path escapes workspace")
        if not path.exists():
            entries.append({"path": relative, "state": "deleted"})
            continue
        data = path.read_bytes()
        entries.append({"path": relative, "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest()})
    canonical = json.dumps(entries, sort_keys=True, separators=(",", ":")).encode()
    return {
        "schema_version": 1,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "head": git("rev-parse", "HEAD").decode().strip(),
        "working_tree_sha256": hashlib.sha256(canonical).hexdigest(),
        "evidence_kind": "source_inventory_not_test_result_or_backup",
        "files": entries,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    target = (ROOT / args.output).resolve()
    if not target.is_relative_to(ROOT / "quality" / "baselines"):
        parser.error("output must be inside quality/baselines")
    report = collect()
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    print(f"source inventory: {len(report['files'])} files; "
          f"sha256={report['working_tree_sha256']}; tests not asserted")


if __name__ == "__main__":
    main()
