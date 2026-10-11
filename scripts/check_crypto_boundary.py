#!/usr/bin/env python3
"""Guard the chosen JWT provider and known RSA private-operation boundaries.

This targeted source/dependency check is not a cryptographic implementation
audit. It complements the unsuppressed Cargo audit by enforcing that JWT
signing uses AWS-LC and production code does not reintroduce RustCrypto RSA
private-operation types.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest

from rust_source import masked, production_source

ROOT = Path(__file__).resolve().parents[1]
PRIVATE_RSA_TYPES = re.compile(r"\b(?:SigningKey|BlindedSigningKey|DecryptingKey|Pkcs1v15Sign|Pkcs1v15Encrypt|Oaep|Pss)\b")


def safe_jwt_features(features: set[str]) -> bool:
    return "aws_lc_rs" in features and "rust_crypto" not in features


def contains_private_rsa_operation(source: str) -> bool:
    code = masked(production_source(source))
    return bool(re.search(r"\brsa\s*::", code) and PRIVATE_RSA_TYPES.search(code))


class CryptoBoundaryTests(unittest.TestCase):
    def test_provider_requires_one_safe_backend(self):
        self.assertTrue(safe_jwt_features({"aws_lc_rs", "use_pem"}))
        for features in [set(), {"rust_crypto"}, {"aws_lc_rs", "rust_crypto"}]:
            self.assertFalse(safe_jwt_features(features))

    def test_private_operations_and_alias_imports_are_rejected(self):
        for source in ["use rsa::pkcs1v15::SigningKey;", "use rsa::Pkcs1v15Sign as Padding;", "use rsa::oaep::DecryptingKey;"]:
            self.assertTrue(contains_private_rsa_operation(source))

    def test_public_parsing_and_ring_signing_are_allowed(self):
        self.assertFalse(contains_private_rsa_operation("use rsa::RsaPublicKey; fn sign() { let key: ring::signature::RsaKeyPair; }"))

    def test_test_only_compatibility_code_does_not_restore_production_rsa_signing(self):
        source = '#[cfg(test)] mod tests { use rsa::pkcs1v15::SigningKey; }\nfn production() {}'
        self.assertFalse(contains_private_rsa_operation(source))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(CryptoBoundaryTests))
        if not result.wasSuccessful():
            return 1
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT))
    packages = {package["id"] for package in metadata["packages"] if package["name"] == "jsonwebtoken" and package["version"].startswith("10.")}
    nodes = [node for node in metadata["resolve"]["nodes"] if node["id"] in packages]
    errors = []
    if len(nodes) != 1 or not safe_jwt_features(set(nodes[0]["features"])):
        errors.append("V2 JWT must resolve aws_lc_rs only, never RustCrypto RSA signing")
    for folder in ("crates", "apps", "src"):
        for path in (ROOT / folder).rglob("*.rs"):
            if "tests" in path.parts or path.stem == "tests" or path.stem.endswith("_tests"):
                continue
            if contains_private_rsa_operation(path.read_text(encoding="utf-8")):
                errors.append(f"{path.relative_to(ROOT).as_posix()}: private RSA operation outside the hardened backend")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("crypto boundary: V2 JWT uses AWS-LC; no known RustCrypto private-operation types in production source")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
