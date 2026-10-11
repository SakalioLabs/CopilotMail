#!/usr/bin/env python3
"""Validate function/test traceability; never infer execution from test existence.

The matrix uses the JSON subset of YAML so CI needs only Python's standard
library. Default mode checks references and required scenarios while allowing
honestly incomplete work. --release rejects incomplete inventory or evidence.
"""

from __future__ import annotations

import argparse
import ast
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile
import unittest

from rust_source import functions, masked

ROOT = Path(__file__).resolve().parents[1]
REQUIRED = {"capability_id", "module", "entrypoints", "risk", "invariants", "cases",
            "test_refs", "environment", "evidence_ref", "candidate_revision", "owner",
            "review_status", "mutates", "concurrent"}
CASE_KINDS = {"normal", "boundary", "failure", "authorization", "transaction", "concurrency"}


def safe_file(root: Path, relative: object) -> Path:
    if not isinstance(relative, str) or not relative or "\\" in relative:
        raise ValueError("reference must be a nonempty repository-relative POSIX path")
    value = Path(relative)
    path = (root / value).resolve()
    if value.is_absolute() or not path.is_relative_to(root.resolve()) or not path.is_file():
        raise ValueError(f"missing or escaping reference: {relative}")
    return path


def require_strings(value: object, label: str) -> list[str]:
    if (not isinstance(value, list) or not value
            or any(not isinstance(item, str) or not item.strip() for item in value)):
        raise ValueError(f"{label} must contain nonempty strings")
    return value


def has_function(source: str, symbol: str) -> bool:
    # Mask multiline/raw literals too. Existence still is not coverage proof.
    if any(item.name == symbol for item in functions(masked(source))):
        return True
    try:
        return any(isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == symbol
                   for node in ast.walk(ast.parse(source)))
    except SyntaxError:
        return False


def contains_symbol(root: Path, relative: str, symbol: str, *, test: bool = False) -> bool:
    file = safe_file(root, relative)
    if file.suffix == ".py":
        tree = ast.parse(file.read_text(encoding="utf-8"))
        return any(isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == symbol
                   for node in ast.walk(tree))
    if file.suffix not in {".ts", ".tsx"}:
        return has_function(file.read_text(encoding="utf-8"), symbol)
    kind = "test" if test else "function"
    report = json.loads(safe_file(root, f"quality/reports/typescript-{kind}-inventory.json").read_text(encoding="utf-8"))
    rows = [row for row in report["files"] if row["path"] == relative]
    if (report.get("source_hash_encoding") != "UTF-8 normalized LF" or len(rows) != 1
            or rows[0]["sha256"] != hashlib.sha256(file.read_text(encoding="utf-8").encode("utf-8")).hexdigest()):
        raise ValueError("TypeScript AST inventory is stale; run node web/scripts/build-quality-inventory.mjs")
    return any(item["name"] == symbol for item in rows[0]["tests" if test else "functions"])


def validate_identity(entry: dict, root: Path, seen: set[str]) -> str:
    if set(entry) != REQUIRED:
        raise ValueError("capability fields do not match schema")
    label = entry["capability_id"]
    if not isinstance(label, str) or not re.fullmatch(r"[a-z][a-z0-9_.-]+", label):
        raise ValueError("invalid capability ID")
    if label in seen:
        raise ValueError("duplicate capability ID")
    seen.add(label)
    for field in ("owner", "candidate_revision", "environment"):
        if not isinstance(entry[field], str) or not entry[field].strip():
            raise ValueError(f"{field} is required")
    if entry["risk"] not in {"low", "medium", "high", "critical"}:
        raise ValueError("unknown risk")
    if type(entry["mutates"]) is not bool or type(entry["concurrent"]) is not bool:
        raise ValueError("mutation/concurrency flags must be booleans")
    for symbol in require_strings(entry["entrypoints"], "entrypoints"):
        if not contains_symbol(root, entry["module"], symbol):
            raise ValueError(f"entrypoint not found: {symbol}")
    require_strings(entry["invariants"], "invariants")
    if entry["review_status"] not in {"baseline_unreviewed", "in_progress", "verified"}:
        raise ValueError("unknown review status")
    return label


def validate_references(references: object, root: Path, release: bool) -> dict:
    if not isinstance(references, dict) or not references:
        raise ValueError("test_refs must not be empty")
    for test_id, reference in references.items():
        if not isinstance(test_id, str) or not isinstance(reference, dict):
            raise ValueError("invalid test reference")
        if set(reference) != {"path", "symbol", "execution"}:
            raise ValueError("test reference fields do not match schema")
        if not isinstance(reference["symbol"], str) or not contains_symbol(root, reference["path"], reference["symbol"], test=True):
            raise ValueError(f"test symbol not found: {test_id}")
        if reference["execution"] not in {"not_run", "passed", "failed", "skipped"}:
            raise ValueError("invalid test execution state")
        if release and reference["execution"] != "passed":
            raise ValueError(f"release test not passed: {test_id}")
    return references


def validate_cases(entry: dict, references: dict) -> None:
    cases = entry["cases"]
    if not isinstance(cases, dict) or not set(cases).issubset(CASE_KINDS):
        raise ValueError("unknown test case category")
    required = {"normal", "boundary", "failure"}
    if entry["risk"] in {"high", "critical"}:
        required.add("authorization")
    if entry["mutates"]:
        required.add("transaction")
    if entry["concurrent"]:
        required.add("concurrency")
    if not required.issubset(cases):
        raise ValueError(f"missing categories: {sorted(required - set(cases))}")
    for category, test_ids in cases.items():
        if not set(require_strings(test_ids, category)).issubset(references):
            raise ValueError(f"unknown test ID in {category}")


def validate_evidence(entry: dict, root: Path, release: bool) -> None:
    if entry["evidence_ref"] is not None:
        safe_file(root, entry["evidence_ref"])
    if not release:
        return
    if entry["review_status"] != "verified" or entry["evidence_ref"] is None:
        raise ValueError("release requires reviewed execution evidence")
    evidence = json.loads(safe_file(root, entry["evidence_ref"]).read_text(encoding="utf-8"))
    if (evidence.get("candidate_revision") != entry["candidate_revision"]
            or evidence.get("result") != "passed"
            or evidence.get("capability_id") != entry["capability_id"]
            or set(evidence.get("passed_test_ids", [])) != set(entry["test_refs"])):
        raise ValueError("evidence does not prove these tests for this candidate")
    sources = {entry["module"], *(reference["path"] for reference in entry["test_refs"].values())}
    hashes = {path: hashlib.sha256(safe_file(root, path).read_bytes()).hexdigest() for path in sources}
    if evidence.get("source_sha256") != hashes:
        raise ValueError("evidence source/test hashes do not match this candidate")


def validate(matrix: object, root: Path, release: bool = False) -> list[str]:
    errors: list[str] = []
    if not isinstance(matrix, dict) or matrix.get("schema_version") != 1:
        return ["unsupported matrix schema"]
    if matrix.get("inventory_scope") not in {"initial", "complete"}:
        errors.append("inventory_scope must explicitly be initial or complete")
    if release and matrix.get("inventory_scope") != "complete":
        errors.append("release requires a reviewed complete function inventory")
    capabilities = matrix.get("capabilities")
    if not isinstance(capabilities, list) or not capabilities:
        return errors + ["capabilities must not be empty"]
    seen: set[str] = set()
    for entry in capabilities:
        label = entry.get("capability_id", "unknown") if isinstance(entry, dict) else "unknown"
        try:
            if not isinstance(entry, dict):
                raise ValueError("capability must be an object")
            validate_identity(entry, root, seen)
            references = validate_references(entry["test_refs"], root, release)
            validate_cases(entry, references)
            validate_evidence(entry, root, release)
        except (ValueError, OSError, TypeError) as error:
            errors.append(f"{label}: {error}")
    return errors


class MatrixTests(unittest.TestCase):
    def test_comment_or_quoted_names_are_not_declarations(self):
        self.assertFalse(has_function('// fn fake() {}', 'fake'))
        self.assertFalse(has_function('let text = "fn fake() {}";', 'fake'))
        self.assertTrue(has_function('    pub(super) async fn real<T>() {}', 'real'))
        self.assertFalse(has_function('/*\nfn fake() {}\n*/', 'fake'))
        self.assertFalse(has_function('let text = r#"\nfn fake() {}\n"#;', 'fake'))
        self.assertFalse(has_function('"""\ndef fake(): pass\n"""', 'fake'))
        self.assertTrue(has_function('async def real():\n    pass', 'real'))

    def test_fail_closed_fixtures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "code.rs").write_text("pub fn run() {}\n#[test]\nfn verifies() {}", encoding="utf-8")
            entry = {"capability_id": "sample.run", "module": "code.rs", "entrypoints": ["run"],
                     "risk": "critical", "invariants": ["atomic"], "mutates": True,
                     "concurrent": True, "cases": {key: ["test"] for key in CASE_KINDS},
                     "test_refs": {"test": {"path": "code.rs", "symbol": "verifies", "execution": "not_run"}},
                     "environment": "unit", "candidate_revision": "candidate", "owner": "maintainer",
                     "review_status": "baseline_unreviewed", "evidence_ref": None}
            good = {"schema_version": 1, "inventory_scope": "initial", "capabilities": [entry]}
            self.assertEqual(validate(good, root), [])
            self.assertTrue(validate(good, root, release=True))
            for field, value in [("owner", ""), ("module", "../code.rs"),
                                 ("entrypoints", ["missing"]), ("risk", "unknown"),
                                 ("test_refs", {}), ("evidence_ref", "absent.json"),
                                 ("mutates", "false"), ("review_status", "done")]:
                changed = deepcopy(good)
                changed["capabilities"][0][field] = value
                with self.subTest(field=field):
                    self.assertTrue(validate(changed, root))
            for category in CASE_KINDS:
                changed = deepcopy(good)
                del changed["capabilities"][0]["cases"][category]
                self.assertTrue(validate(changed, root))
            changed = deepcopy(good)
            changed["capabilities"].append(deepcopy(entry))
            self.assertTrue(validate(changed, root))
            changed = deepcopy(good)
            changed["capabilities"][0]["test_refs"]["test"]["symbol"] = "absent"
            self.assertTrue(validate(changed, root))
            changed = deepcopy(good)
            changed["inventory_scope"] = "complete"
            checked = changed["capabilities"][0]
            checked.update(review_status="verified", evidence_ref="evidence.json")
            checked["test_refs"]["test"]["execution"] = "passed"
            evidence = {"candidate_revision": "candidate", "capability_id": "sample.run",
                        "result": "passed", "passed_test_ids": ["test"],
                        "source_sha256": {"code.rs": hashlib.sha256((root / "code.rs").read_bytes()).hexdigest()}}
            (root / "evidence.json").write_text(json.dumps(evidence), encoding="utf-8")
            self.assertEqual(validate(changed, root, release=True), [])
            (root / "code.rs").write_text("pub fn run() { panic!(); }\n#[test]\nfn verifies() {}", encoding="utf-8")
            self.assertTrue(validate(changed, root, release=True))
            evidence["candidate_revision"] = "stale"
            (root / "evidence.json").write_text(json.dumps(evidence), encoding="utf-8")
            self.assertTrue(validate(changed, root, release=True))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", default="quality/function-test-matrix.yaml")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(MatrixTests))
        if not result.wasSuccessful():
            return 1
    try:
        matrix = json.loads(safe_file(ROOT, args.matrix).read_text(encoding="utf-8"))
        errors = validate(matrix, ROOT, args.release)
    except (ValueError, OSError) as error:
        errors = [str(error)]
    if errors:
        for error in errors:
            print(f"function-test-matrix: {error}", file=sys.stderr)
        return 1
    print(f"function test references valid: {len(matrix['capabilities'])} capabilities; "
          f"inventory={matrix['inventory_scope']}; release_approved={args.release}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
