#!/usr/bin/env python3
"""Inventory explicit Rust functions and summarize measured coverage honestly.

The source inventory is lexical (no macro expansion or conditional compilation
evaluation). Compiler coverage and per-capability assertions remain separate.
Generated reports contain identifiers/counts/hashes, never source bodies.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import platform

from rust_source import functions, masked, production_source

ROOT = Path(__file__).resolve().parents[1]


def rust_inventory() -> dict:
    paths = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT).decode().split("\0")
    rows = []
    for name in sorted(set(paths) - {""}):
        path = ROOT / name
        if not name.endswith(".rs") or not name.startswith(("crates/", "apps/", "src/")):
            continue
        if "/tests/" in name or path.name == "tests.rs" or path.stem.endswith("_tests") or not path.is_file():
            continue
        source = path.read_text(encoding="utf-8")
        code = masked(production_source(source))
        entries = []
        for function in functions(code):
            line = source.count("\n", 0, function.start) + 1
            end = source.count("\n", 0, function.end) + 1
            entries.append({"name": function.name, "line": line, "end_line": end,
                            "source_lines": end - line + 1})
        rows.append({"path": name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                     "source_lines": len(source.splitlines()), "functions": entries})
    return {"schema_version": 1, "kind": "explicit_rust_source_inventory_not_test_coverage",
            "limitations": ["Does not expand macros or enumerate anonymous closures", "Includes platform-specific source even if not built on this host", "Function length is a review signal, not a completeness proof"],
            "file_count": len(rows), "function_count": sum(len(row["functions"]) for row in rows), "files": rows}


def coverage_summary(path: Path) -> dict:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if raw.get("type") != "llvm.coverage.json.export" or len(raw.get("data", [])) != 1:
        raise ValueError("expected one LLVM workspace export")
    data = raw["data"][0]
    files = []
    for record in data["files"]:
        source = Path(record["filename"]).resolve()
        if not source.is_relative_to(ROOT):
            raise ValueError("coverage contains a source outside the workspace")
        files.append({"path": source.relative_to(ROOT).as_posix(),
                      "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                      "summary": record["summary"]})
    return {"schema_version": 1, "kind": "measured_llvm_coverage",
            "input_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "scope": f"{platform.system()} workspace tests; ignored integration scenarios not run",
            "limitations": ["Includes inline test harness and compiler-generated functions", "A hit is not proof of adequate assertions or every failure path", "Source can change after measurement; bind a final candidate before release"],
            "branch_coverage": "not_measured" if data["totals"]["branches"]["count"] == 0 else "measured",
            "totals": data["totals"], "files": files}


def write_report(relative: str, value: dict) -> None:
    target = (ROOT / relative).resolve()
    if not target.is_relative_to(ROOT / "quality" / "reports"):
        raise ValueError("quality reports must stay in quality/reports")
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--llvm-baseline", action="store_true")
    parser.add_argument("--llvm-report", action="store_true", help="Summarize this run without overwriting the pre-review baseline")
    args = parser.parse_args()
    inventory = rust_inventory()
    write_report("quality/reports/rust-function-inventory.json", inventory)
    if args.llvm_baseline:
        write_report("quality/reports/rust-coverage-before-quality-pass.json",
                     coverage_summary(ROOT / "quality/coverage/rust-workspace.json"))
    if args.llvm_report:
        write_report("quality/reports/rust-coverage.json",
                     coverage_summary(ROOT / "quality/coverage/rust-workspace.json"))
    print(f"inventoried {inventory['file_count']} Rust files / {inventory['function_count']} explicit functions; no review completion inferred")


if __name__ == "__main__":
    main()
