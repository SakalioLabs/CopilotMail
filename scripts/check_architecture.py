#!/usr/bin/env python3
"""Fail-closed dependency-direction checks for the Rust workspace."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tomllib
from collections import deque
from collections.abc import Mapping

ROOT = pathlib.Path(__file__).resolve().parents[1]
DOMAIN_MANIFEST = ROOT / "crates" / "copilotmail-domain" / "Cargo.toml"
AUDIT_MANIFEST = ROOT / "crates" / "copilotmail-audit" / "Cargo.toml"
APPLICATION_MANIFEST = ROOT / "crates" / "copilotmail-application" / "Cargo.toml"
MAIL_ENGINE_MANIFEST = ROOT / "crates" / "copilotmail-mail-engine" / "Cargo.toml"
APPLICATION_SOURCE = APPLICATION_MANIFEST.parent / "src"
MAIL_PORTS_SOURCE = APPLICATION_SOURCE / "mail_ports.rs"
MAIL_ENGINE_SOURCE = MAIL_ENGINE_MANIFEST.parent / "src" / "lib.rs"

INTERNAL_PREFIX = "copilotmail-"
LAYER_ALLOWED_INTERNAL = {
    "domain": frozenset(),
    "audit": frozenset({"copilotmail-domain", "copilotmail-crypto"}),
    "application": frozenset({"copilotmail-audit", "copilotmail-domain", "copilotmail-crypto"}),
}
LAYER_PACKAGE_NAMES = {
    "domain": "copilotmail-domain",
    "audit": "copilotmail-audit",
    "application": "copilotmail-application",
}
MAIL_PORT_SYMBOLS = {
    "MailAddress": "struct",
    "MailMessageSummary": "struct",
    "MailMessagePage": "struct",
    "MailAttachmentSummary": "struct",
    "MailMessageDetail": "struct",
    "MailMessageState": "struct",
    "TextDraftInput": "struct",
    "MailDraft": "struct",
    "MailDraftStore": "trait",
    "MailSubmissionReconciliation": "enum",
    "CreatedMailSubmission": "struct",
    "RecipientDeliveryState": "enum",
    "RecipientDeliveryEvidence": "struct",
    "MailDeliverySnapshot": "struct",
    "MailSubmissionAdapter": "trait",
    "MailContentReader": "trait",
    "InboundMessageClassification": "enum",
    "InboundMailEngineEvent": "struct",
    "OutboundDeliveryEventKind": "enum",
    "OutboundMailEngineEvent": "struct",
    "MailEngineWebhookEvent": "enum",
    "MailEngineReadiness": "struct",
    "MailEngine": "trait",
    "MailEngineDomainProjection": "struct",
    "MailDomainProjector": "trait",
    "MailDataPlanePort": "trait",
    "MailEngineError": "enum",
}
FORBIDDEN_APPLICATION_VENDOR_TOKENS = frozenset({"stalwart"})


class ArchitectureError(ValueError):
    """A dependency or source boundary violates the approved architecture."""


def load_manifest(path: pathlib.Path) -> dict[str, object]:
    with path.open("rb") as handle:
        value = tomllib.load(handle)
    if not isinstance(value, dict):
        raise ArchitectureError(f"{path.relative_to(ROOT)} is not a TOML mapping")
    return value


def dependency_package_name(
    manifest_path: pathlib.Path,
    dependency_key: str,
    specification: object,
) -> str:
    if isinstance(specification, str):
        return dependency_key
    if not isinstance(specification, Mapping):
        raise ArchitectureError(
            f"{manifest_path.relative_to(ROOT)} has an invalid dependency "
            f"specification for {dependency_key}"
        )
    package = specification.get("package")
    if isinstance(package, str) and package:
        return package
    dependency_path = specification.get("path")
    if isinstance(dependency_path, str):
        target_manifest = (manifest_path.parent / dependency_path / "Cargo.toml").resolve()
        if target_manifest.is_file():
            target = load_manifest(target_manifest)
            target_package = target.get("package")
            if isinstance(target_package, Mapping):
                name = target_package.get("name")
                if isinstance(name, str) and name:
                    return name
    return dependency_key


def manifest_dependencies(
    manifest_path: pathlib.Path,
    manifest: Mapping[str, object],
) -> set[str]:
    dependencies: set[str] = set()

    def add_sections(table: Mapping[str, object], context: str) -> None:
        for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
            section = table.get(section_name, {})
            if not isinstance(section, Mapping):
                raise ArchitectureError(
                    f"{manifest_path.relative_to(ROOT)} {context}{section_name} "
                    "must be a mapping"
                )
            for key, specification in section.items():
                dependencies.add(
                    dependency_package_name(manifest_path, str(key), specification)
                )

    add_sections(manifest, "")
    targets = manifest.get("target", {})
    if not isinstance(targets, Mapping):
        raise ArchitectureError(
            f"{manifest_path.relative_to(ROOT)} target must be a mapping"
        )
    for target_name, target_table in targets.items():
        if not isinstance(target_table, Mapping):
            raise ArchitectureError(
                f"{manifest_path.relative_to(ROOT)} target.{target_name} "
                "must be a mapping"
            )
        add_sections(target_table, f"target.{target_name}.")
    return dependencies


def direct_dependencies(manifest_path: pathlib.Path) -> set[str]:
    manifest = load_manifest(manifest_path)
    if not isinstance(manifest, Mapping):
        raise ArchitectureError(f"{manifest_path.relative_to(ROOT)} is invalid")
    return manifest_dependencies(manifest_path, manifest)


def validate_layer_dependencies(
    layer: str, dependencies: set[str], allowed_internal: frozenset[str]
) -> None:
    internal = {name for name in dependencies if name.startswith(INTERNAL_PREFIX)}
    forbidden = internal - allowed_internal
    if forbidden:
        raise ArchitectureError(
            f"{layer} layer depends outward on: {', '.join(sorted(forbidden))}"
        )


def shortest_forbidden_path(
    root: str,
    initial_edges: Mapping[str, set[str]],
    continuation_edges: Mapping[str, set[str]],
    forbidden: frozenset[str],
) -> list[str] | None:
    queue: deque[list[str]] = deque(
        [[root, dependency] for dependency in sorted(initial_edges.get(root, set()))]
    )
    visited = {root}
    while queue:
        path = queue.popleft()
        current = path[-1]
        if current in forbidden:
            return path
        if current in visited:
            continue
        visited.add(current)
        for dependency in sorted(continuation_edges.get(current, set())):
            if dependency not in visited:
                queue.append([*path, dependency])
    return None


def resolved_package_graph(
    metadata: Mapping[str, object],
) -> tuple[
    dict[str, str],
    frozenset[str],
    dict[str, set[str]],
    dict[str, set[str]],
]:
    packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    raw_workspace_members = metadata.get("workspace_members")
    if (
        not isinstance(packages, list)
        or not isinstance(resolve, dict)
        or not isinstance(resolve.get("nodes"), list)
        or not isinstance(raw_workspace_members, list)
        or not raw_workspace_members
    ):
        raise ArchitectureError("cargo metadata omitted the resolved workspace graph")
    workspace_members = frozenset(
        member for member in raw_workspace_members if isinstance(member, str)
    )
    if len(workspace_members) != len(raw_workspace_members):
        raise ArchitectureError("cargo metadata workspace member list is invalid")

    package_names: dict[str, str] = {}
    for package in packages:
        if not isinstance(package, dict):
            raise ArchitectureError("cargo metadata package entry is invalid")
        package_id = package.get("id")
        name = package.get("name")
        if isinstance(package_id, str) and isinstance(name, str):
            package_names[package_id] = name
    if not workspace_members.issubset(package_names):
        raise ArchitectureError("cargo metadata workspace package mapping is incomplete")

    # A protected root's first hop includes normal, build, dev, and every target.
    # Past that first hop, only normal/build edges participate in the root build;
    # dependency dev edges are intentionally excluded.

    initial_edges: dict[str, set[str]] = {
        package_id: set() for package_id in package_names
    }
    continuation_edges: dict[str, set[str]] = {
        package_id: set() for package_id in package_names
    }
    resolved_node_ids: set[str] = set()
    for node in resolve["nodes"]:
        if not isinstance(node, dict):
            raise ArchitectureError("cargo metadata dependency node is invalid")
        source_id = node.get("id")
        if not isinstance(source_id, str) or source_id not in package_names:
            raise ArchitectureError("cargo metadata contains an unknown package node")
        if source_id in resolved_node_ids:
            raise ArchitectureError(
                "cargo metadata contains a duplicate package dependency node"
            )
        resolved_node_ids.add(source_id)
        dependencies = node.get("deps")
        if not isinstance(dependencies, list):
            raise ArchitectureError("cargo metadata dependency node is invalid")
        for dependency in dependencies:
            if not isinstance(dependency, dict):
                raise ArchitectureError("cargo metadata dependency edge is invalid")
            target_id = dependency.get("pkg")
            if not isinstance(target_id, str) or target_id not in package_names:
                raise ArchitectureError(
                    "cargo metadata dependency edge targets an unknown package"
                )
            initial_edges[source_id].add(target_id)
            dependency_kinds = dependency.get("dep_kinds", [])
            if not isinstance(dependency_kinds, list) or not dependency_kinds:
                raise ArchitectureError("cargo metadata dependency kind is invalid")
            for dependency_kind in dependency_kinds:
                if (
                    not isinstance(dependency_kind, dict)
                    or "kind" not in dependency_kind
                    or dependency_kind.get("kind") not in (None, "build", "dev")
                ):
                    raise ArchitectureError(
                        "cargo metadata dependency kind is invalid"
                    )
            if any(
                kind["kind"] in (None, "build")
                for kind in dependency_kinds
            ):
                continuation_edges[source_id].add(target_id)
    if resolved_node_ids != set(package_names):
        raise ArchitectureError("cargo metadata dependency graph is incomplete")
    return (
        package_names,
        workspace_members,
        initial_edges,
        continuation_edges,
    )


def cargo_workspace_graph(
) -> tuple[
    dict[str, str],
    frozenset[str],
    dict[str, set[str]],
    dict[str, set[str]],
]:
    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--all-features",
    ]
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            timeout=120,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ArchitectureError("cargo metadata could not be executed") from error
    if result.returncode != 0:
        raise ArchitectureError(
            "cargo metadata failed; refresh Cargo.lock before evaluating architecture"
        )
    try:
        metadata = json.loads(result.stdout)
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise ArchitectureError("cargo metadata returned invalid JSON") from error
    if not isinstance(metadata, dict):
        raise ArchitectureError("cargo metadata root must be a JSON object")
    return resolved_package_graph(metadata)


def validate_resolved_layer_graphs(
    package_names: Mapping[str, str],
    workspace_members: frozenset[str],
    initial_edges: Mapping[str, set[str]],
    continuation_edges: Mapping[str, set[str]],
) -> None:
    canonical_ids: dict[str, str] = {}
    for layer, package_name in LAYER_PACKAGE_NAMES.items():
        candidates = [
            package_id
            for package_id in workspace_members
            if package_names[package_id] == package_name
        ]
        if len(candidates) != 1:
            raise ArchitectureError(
                "cargo metadata must contain exactly one workspace "
                f"{layer} package"
            )
        canonical_ids[package_name] = candidates[0]

    internal_ids = frozenset(
        package_id
        for package_id, name in package_names.items()
        if name.startswith(INTERNAL_PREFIX)
    )
    member_names: dict[str, str] = {}
    for package_id in workspace_members:
        name = package_names[package_id]
        if name in member_names:
            raise ArchitectureError(
                f"cargo metadata must contain exactly one workspace package named {name}"
            )
        member_names[name] = package_id
    for layer, package_name in LAYER_PACKAGE_NAMES.items():
        root = canonical_ids[package_name]
        try:
            allowed_ids = frozenset(
                member_names[allowed_name]
                for allowed_name in LAYER_ALLOWED_INTERNAL[layer]
            )
        except KeyError as error:
            raise ArchitectureError(
                f"{layer} layer allowlist references unknown workspace package: "
                f"{error.args[0]}"
            ) from error
        forbidden_ids = internal_ids - allowed_ids - {root}
        path = shortest_forbidden_path(
            root,
            initial_edges,
            continuation_edges,
            forbidden_ids,
        )
        if path is not None:
            raise ArchitectureError(
                f"{layer} layer reaches a forbidden internal package: "
                + " -> ".join(package_names[package_id] for package_id in path)
            )


def validate_resolved_workspace_layers() -> None:
    validate_resolved_layer_graphs(*cargo_workspace_graph())


def rust_sources(directory: pathlib.Path) -> list[pathlib.Path]:
    return sorted(path for path in directory.rglob("*.rs") if path.is_file())


def mask_rust_non_code(source: str) -> str:
    """Mask comments and literals while preserving offsets and newlines."""

    masked = list(source)

    def blank(start: int, end: int) -> None:
        for index in range(start, end):
            if masked[index] not in ("\r", "\n"):
                masked[index] = " "

    index = 0
    length = len(source)
    while index < length:
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            if end == -1:
                end = length
            blank(index, end)
            index = end
            continue
        if source.startswith("/*", index):
            start = index
            depth = 1
            index += 2
            while index < length and depth:
                if source.startswith("/*", index):
                    depth += 1
                    index += 2
                elif source.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            if depth:
                raise ArchitectureError("Rust source contains an unterminated block comment")
            blank(start, index)
            continue

        raw_match = re.match(r"(?:br|r)(?P<hashes>#{0,255})\"", source[index:])
        if raw_match is not None:
            start = index
            hashes = raw_match.group("hashes")
            index += raw_match.end()
            terminator = '"' + hashes
            end = source.find(terminator, index)
            if end == -1:
                raise ArchitectureError("Rust source contains an unterminated raw string")
            index = end + len(terminator)
            blank(start, index)
            continue

        string_prefix = 1 if source[index] == '"' else 2
        if source[index] == '"' or source.startswith(("b\"", "c\""), index):
            start = index
            index += string_prefix
            escaped = False
            while index < length:
                character = source[index]
                index += 1
                if escaped:
                    escaped = False
                elif character == "\\":
                    escaped = True
                elif character == '"':
                    break
            else:
                raise ArchitectureError("Rust source contains an unterminated string")
            blank(start, index)
            continue

        char_match = re.match(
            r"(?:b)?'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|.)|[^\\'\r\n])'",
            source[index:],
        )
        if char_match is not None:
            end = index + char_match.end()
            blank(index, end)
            index = end
            continue
        index += 1
    return "".join(masked)


def canonical_use_statement(statement: str) -> str:
    value = re.sub(r"\s+", " ", statement).strip()
    value = re.sub(r"\s*::\s*", "::", value)
    value = re.sub(r"\s*([{},;*])\s*", r"\1", value)
    return value


def rust_use_statements(source: str) -> list[tuple[int, str, bool]]:
    """Return (brace depth, canonical statement, is public) tuples."""

    code = mask_rust_non_code(source)
    statements: list[tuple[int, str, bool]] = []
    depth = 0
    index = 0
    while index < len(code):
        character = code[index]
        if character == "{":
            depth += 1
            index += 1
            continue
        if character == "}":
            depth -= 1
            if depth < 0:
                raise ArchitectureError("Rust source contains an unmatched closing brace")
            index += 1
            continue
        if character.isalpha() or character == "_":
            match = re.match(
                r"(?:(pub(?:\s*\([^)\r\n]+\))?)\s+)?use\b",
                code[index:],
            )
            if match is not None:
                previous = code[index - 1] if index else ""
                if previous.isalnum() or previous == "_":
                    index += 1
                    continue
                end = code.find(";", index + match.end())
                if end == -1:
                    raise ArchitectureError("Rust use statement is missing a semicolon")
                statement = canonical_use_statement(code[index : end + 1])
                statements.append((depth, statement, match.group(1) is not None))
                index = end + 1
                continue
            identifier = re.match(r"[A-Za-z_][A-Za-z0-9_]*", code[index:])
            if identifier is not None:
                index += identifier.end()
                continue
        index += 1
    if depth != 0:
        raise ArchitectureError("Rust source contains unmatched braces")
    return statements


def rust_type_declarations(source: str) -> list[tuple[int, str, str, str]]:
    """Return topological depth, visibility, kind, and normalized item name."""

    code = mask_rust_non_code(source)
    depth_by_offset: list[int] = [0] * (len(code) + 1)
    depth = 0
    for index, character in enumerate(code):
        depth_by_offset[index] = depth
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth < 0:
                raise ArchitectureError("Rust source contains an unmatched closing brace")
    depth_by_offset[len(code)] = depth
    if depth != 0:
        raise ArchitectureError("Rust source contains unmatched braces")

    pattern = re.compile(
        r"(?<![A-Za-z0-9_])"
        r"(?:(?P<visibility>pub(?:\s*\([^)]*\))?)\s+)?"
        r"(?P<kind>struct|enum|trait|type|union|mod)\s+"
        r"(?:r#)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b"
    )
    return [
        (
            depth_by_offset[match.start()],
            re.sub(r"\s+", "", match.group("visibility") or ""),
            match.group("kind"),
            match.group("name"),
        )
        for match in pattern.finditer(code)
    ]


def rust_generic_parameter_bindings(source: str) -> set[str]:
    """Return type/const generic bindings declared by Rust items."""

    code = mask_rust_non_code(source)
    item_patterns = (
        re.compile(r"\bimpl\s*<"),
        re.compile(
            r"\b(?:fn|struct|enum|trait|type)\s+"
            r"(?:r#)?[A-Za-z_][A-Za-z0-9_]*\s*<"
        ),
    )
    opening_offsets = sorted(
        {
            match.end() - 1
            for pattern in item_patterns
            for match in pattern.finditer(code)
        }
    )
    bindings: set[str] = set()
    for opening_offset in opening_offsets:
        depth = 1
        index = opening_offset + 1
        parameter_start = index
        parameters: list[str] = []
        while index < len(code) and depth:
            character = code[index]
            if character == "<":
                depth += 1
            elif character == ">" and (index == 0 or code[index - 1] != "-"):
                depth -= 1
                if depth == 0:
                    parameters.append(code[parameter_start:index])
                    break
            elif character == "," and depth == 1:
                parameters.append(code[parameter_start:index])
                parameter_start = index + 1
            index += 1
        if depth:
            raise ArchitectureError("Rust generic parameter list is unterminated")
        for parameter in parameters:
            binding = re.match(
                r"\s*(?:const\s+)?(?:r#)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b",
                parameter,
            )
            if binding is not None:
                bindings.add(binding.group("name"))
    return bindings


def protected_symbol_pattern() -> re.Pattern[str]:
    return re.compile(
        r"(?<![A-Za-z0-9_])(?:r#)?(?:"
        + "|".join(re.escape(symbol) for symbol in MAIL_PORT_SYMBOLS)
        + r")(?![A-Za-z0-9_])"
    )


def validate_mail_port_sources(ports_source: str, adapter_source: str) -> None:
    port_declarations = rust_type_declarations(ports_source)
    invalid_ports: list[str] = []
    for symbol, expected_kind in MAIL_PORT_SYMBOLS.items():
        declarations = [
            (visibility, kind)
            for depth, visibility, kind, name in port_declarations
            if depth == 0 and name == symbol
        ]
        if declarations != [("pub", expected_kind)]:
            invalid_ports.append(symbol)
    if invalid_ports:
        raise ArchitectureError(
            "application mail ports must have one exact public declaration: "
            + ", ".join(sorted(invalid_ports))
        )

    adapter = mask_rust_non_code(adapter_source)
    uses = rust_use_statements(adapter_source)
    expected_import = "use copilotmail_application::mail_ports::*;"
    expected = [
        statement
        for depth, statement, is_public in uses
        if depth == 0 and statement == expected_import and not is_public
    ]
    if len(expected) != 1:
        raise ArchitectureError(
            "mail-engine must privately import the application-owned ports exactly once"
        )

    application_port_paths = re.findall(
        r"(?<![A-Za-z0-9_])copilotmail_application\s*::\s*mail_ports"
        r"(?![A-Za-z0-9_])",
        adapter,
    )
    if len(application_port_paths) != 1:
        raise ArchitectureError(
            "mail-engine must reference the canonical application mail-port path "
            "only in its private import"
        )

    symbol_pattern = protected_symbol_pattern()
    mail_ports_segment = re.compile(
        r"(?<![A-Za-z0-9_])(?:r#)?mail_ports(?![A-Za-z0-9_])"
    )
    application_segment = re.compile(
        r"(?<![A-Za-z0-9_])(?:r#)?copilotmail_application(?![A-Za-z0-9_])"
    )
    glob_pattern = re.compile(r"(?<![A-Za-z0-9_])\*(?![A-Za-z0-9_])")
    for depth, statement, is_public in uses:
        if is_public and (
            glob_pattern.search(statement) is not None
            or mail_ports_segment.search(statement) is not None
            or application_segment.search(statement) is not None
            or symbol_pattern.search(statement) is not None
        ):
            raise ArchitectureError(
                "mail-engine must not re-export application-owned mail ports"
            )
        if statement != expected_import and (
            application_segment.search(statement) is not None
            or mail_ports_segment.search(statement) is not None
            or symbol_pattern.search(statement) is not None
        ):
            raise ArchitectureError(
                "mail-engine imports a second binding for application-owned ports"
            )
        if depth == 0 and statement != expected_import:
            if glob_pattern.search(statement) is not None:
                raise ArchitectureError(
                    "mail-engine has a top-level import that can shadow application ports"
                )

    for _depth, _visibility, _kind, name in rust_type_declarations(adapter_source):
        if name in MAIL_PORT_SYMBOLS:
            raise ArchitectureError(
                f"mail-engine redefines application-owned port {name}"
            )
        if name == "copilotmail_application":
            raise ArchitectureError(
                "mail-engine shadows the canonical application crate name"
            )
    generic_shadows = rust_generic_parameter_bindings(adapter_source).intersection(
        MAIL_PORT_SYMBOLS
    )
    if generic_shadows:
        raise ArchitectureError(
            "mail-engine shadows application-owned ports with generic parameters: "
            + ", ".join(sorted(generic_shadows))
        )
    if re.search(
        r"\bextern\s+crate\s+(?:r#)?copilotmail_application\b",
        adapter,
    ):
        raise ArchitectureError(
            "mail-engine must not create an alias for the application crate"
        )
    for symbol in MAIL_PORT_SYMBOLS:
        if re.search(
            rf"\bextern\s+crate\s+(?:(?:r#)?{re.escape(symbol)}\b|"
            rf"(?:r#)?[A-Za-z_][A-Za-z0-9_]*\s+as\s+"
            rf"(?:r#)?{re.escape(symbol)}\b)",
            adapter,
        ):
            raise ArchitectureError(
                f"mail-engine binds an extern crate as application port {symbol}"
            )


def validate_mail_port_corpus(
    ports_source: str,
    adapter_sources: Mapping[str, str],
) -> None:
    """Validate every adapter Rust source as one ownership boundary."""

    if not isinstance(adapter_sources, Mapping):
        raise ArchitectureError("mail-engine source corpus is invalid")
    if not adapter_sources:
        raise ArchitectureError("mail-engine adapter source corpus is empty")
    if any(
        not isinstance(source_name, str) or not isinstance(source, str)
        for source_name, source in adapter_sources.items()
    ):
        raise ArchitectureError("mail-engine source corpus is invalid")
    ordered_sources: list[str] = []
    for source_name in sorted(adapter_sources):
        source = adapter_sources[source_name]
        # A corpus-wide concatenation enforces one canonical import, while this
        # per-file pass prevents unmatched braces from cancelling across files.
        rust_use_statements(source)
        ordered_sources.append(source)
    validate_mail_port_sources(ports_source, "\n".join(ordered_sources))


def validate_application_vendor_neutrality(
    application_sources: Mapping[str, str],
) -> None:
    """Keep provider identities out of the application layer, including tests."""

    if not isinstance(application_sources, Mapping) or not application_sources:
        raise ArchitectureError("application source corpus is invalid")
    if any(
        not isinstance(source_name, str) or not isinstance(source, str)
        for source_name, source in application_sources.items()
    ):
        raise ArchitectureError("application source corpus is invalid")
    for source_name in sorted(application_sources):
        source = application_sources[source_name]
        lowered = source.casefold()
        for token in sorted(FORBIDDEN_APPLICATION_VENDOR_TOKENS):
            offset = lowered.find(token.casefold())
            if offset != -1:
                line = source.count("\n", 0, offset) + 1
                raise ArchitectureError(
                    "application source contains forbidden vendor token "
                    f"{token!r}: {source_name}:{line}"
                )


def validate_mail_port_ownership() -> None:
    if not MAIL_PORTS_SOURCE.is_file():
        raise ArchitectureError("application-owned mail_ports.rs is missing")
    if not MAIL_ENGINE_SOURCE.is_file():
        raise ArchitectureError("mail-engine adapter source is missing")

    application_sources = {
        str(source.relative_to(APPLICATION_MANIFEST.parent)): source.read_text(
            encoding="utf-8"
        )
        for source in rust_sources(APPLICATION_SOURCE)
    }
    validate_application_vendor_neutrality(application_sources)
    for source_name, text in application_sources.items():
        if "copilotmail_mail_engine" in text:
            raise ArchitectureError(
                f"{APPLICATION_MANIFEST.parent.relative_to(ROOT) / source_name} "
                "imports the outward mail-engine adapter"
            )

    adapter_sources = {
        str(source.relative_to(MAIL_ENGINE_MANIFEST.parent)): source.read_text(
            encoding="utf-8"
        )
        for source in rust_sources(MAIL_ENGINE_MANIFEST.parent / "src")
    }
    validate_mail_port_corpus(
        MAIL_PORTS_SOURCE.read_text(encoding="utf-8"),
        adapter_sources,
    )


def expect_architecture_error(callable_value: object, message: str) -> None:
    if not callable(callable_value):
        raise ArchitectureError("self-test received a non-callable case")
    try:
        callable_value()
    except ArchitectureError:
        return
    raise ArchitectureError(message)


def self_test() -> None:
    validate_layer_dependencies(
        "application",
        {"serde", "copilotmail-domain", "copilotmail-audit"},
        LAYER_ALLOWED_INTERNAL["application"],
    )
    expect_architecture_error(
        lambda: validate_layer_dependencies(
            "application",
            {"copilotmail-domain", "copilotmail-mail-engine"},
            LAYER_ALLOWED_INTERNAL["application"],
        ),
        "self-test accepted an application-to-adapter dependency",
    )
    target_dependencies = manifest_dependencies(
        APPLICATION_MANIFEST,
        {
            "target": {
                "cfg(windows)": {
                    "dev-dependencies": {
                        "renamed-adapter": {
                            "package": "copilotmail-mail-engine",
                            "version": "1",
                        }
                    }
                }
            }
        },
    )
    if target_dependencies != {"copilotmail-mail-engine"}:
        raise ArchitectureError(
            "self-test missed a target-specific renamed dependency"
        )
    direct_path = shortest_forbidden_path(
        "application",
        {"application": {"mail-engine"}},
        {"application": {"mail-engine"}, "mail-engine": set()},
        frozenset({"mail-engine"}),
    )
    if direct_path != ["application", "mail-engine"]:
        raise ArchitectureError("self-test missed a direct adapter path")
    indirect_path = shortest_forbidden_path(
        "application",
        {"application": {"allowed-wrapper"}},
        {
            "application": {"allowed-wrapper"},
            "allowed-wrapper": {"mail-engine"},
            "mail-engine": set(),
        },
        frozenset({"mail-engine"}),
    )
    if indirect_path != ["application", "allowed-wrapper", "mail-engine"]:
        raise ArchitectureError("self-test missed an indirect adapter path")
    reverse_path = shortest_forbidden_path(
        "application",
        {"application": set(), "mail-engine": {"application"}},
        {"application": set(), "mail-engine": {"application"}},
        frozenset({"mail-engine"}),
    )
    if reverse_path is not None:
        raise ArchitectureError("self-test rejected the legal adapter-to-port direction")

    synthetic_package_names = {
        "domain-id": "copilotmail-domain",
        "crypto-id": "copilotmail-crypto",
        "audit-id": "copilotmail-audit",
        "application-id": "copilotmail-application",
        "external-id": "external-facade",
        "adapter-id": "copilotmail-mail-engine",
    }
    synthetic_workspace_members = [
        "domain-id",
        "crypto-id",
        "audit-id",
        "application-id",
        "adapter-id",
    ]

    def synthetic_metadata(
        edges: list[tuple[str, str, str | None, str | None, str | None]],
    ) -> dict[str, object]:
        dependencies_by_source: dict[str, list[dict[str, object]]] = {
            package_id: [] for package_id in synthetic_package_names
        }
        for source_id, target_id, kind, target_cfg, dependency_alias in edges:
            dependencies_by_source[source_id].append(
                {
                    "name": dependency_alias or synthetic_package_names[target_id],
                    "pkg": target_id,
                    "dep_kinds": [{"kind": kind, "target": target_cfg}],
                }
            )
        return {
            "packages": [
                {"id": package_id, "name": package_name}
                for package_id, package_name in synthetic_package_names.items()
            ],
            "workspace_members": synthetic_workspace_members,
            "resolve": {
                "nodes": [
                    {
                        "id": package_id,
                        "deps": dependencies_by_source[package_id],
                    }
                    for package_id in synthetic_package_names
                ]
            },
        }

    legal_layer_edges = [
        ("audit-id", "domain-id", None, None, None),
        ("application-id", "audit-id", None, None, None),
        ("adapter-id", "application-id", None, None, None),
    ]
    validate_resolved_layer_graphs(
        *resolved_package_graph(synthetic_metadata(legal_layer_edges))
    )

    expect_architecture_error(
        lambda: validate_resolved_layer_graphs(
            *resolved_package_graph(
                synthetic_metadata(
                    legal_layer_edges
                    + [
                        ("application-id", "external-id", None, None, None),
                        ("external-id", "adapter-id", None, None, None),
                    ]
                )
            )
        ),
        "self-test missed an adapter behind a non-workspace dependency",
    )
    expect_architecture_error(
        lambda: validate_resolved_layer_graphs(
            *resolved_package_graph(
                synthetic_metadata(
                    legal_layer_edges
                    + [
                        (
                            "domain-id",
                            "adapter-id",
                            "dev",
                            "cfg(windows)",
                            "db",
                        )
                    ]
                )
            )
        ),
        "self-test accepted a target-specific workspace dependency alias",
    )
    expect_architecture_error(
        lambda: validate_resolved_layer_graphs(
            *resolved_package_graph(
                synthetic_metadata(
                    legal_layer_edges
                    + [("audit-id", "application-id", None, None, None)]
                )
            )
        ),
        "self-test missed an outward dependency from the audit layer",
    )
    expect_architecture_error(
        lambda: validate_resolved_layer_graphs(
            *resolved_package_graph(
                synthetic_metadata(
                    legal_layer_edges
                    + [
                        ("application-id", "external-id", "dev", None, None),
                        ("external-id", "adapter-id", None, None, None),
                    ]
                )
            )
        ),
        "self-test failed to traverse a root dev dependency",
    )
    expect_architecture_error(
        lambda: validate_resolved_layer_graphs(
            *resolved_package_graph(
                synthetic_metadata(
                    legal_layer_edges
                    + [
                        ("application-id", "external-id", None, None, None),
                        ("external-id", "adapter-id", "build", "cfg(unix)", None),
                    ]
                )
            )
        ),
        "self-test failed to traverse a target-specific build dependency",
    )
    validate_resolved_layer_graphs(
        *resolved_package_graph(
            synthetic_metadata(
                legal_layer_edges
                + [
                    ("application-id", "external-id", None, None, None),
                    ("external-id", "adapter-id", "dev", None, None),
                ]
            )
        )
    )

    validate_application_vendor_neutrality(
        {"src/lib.rs": "pub const PROVIDER_ID: &str = \"opaque-provider\";\n"}
    )
    for source_name, bypass in (
        ("src/literal.rs", 'const SOURCE: &str = "stalwart";\n'),
        ("src/identifier.rs", "const STALWART_SOURCE: &str = \"opaque\";\n"),
        ("src/comment.rs", "// StAlWaRt must not leak through comments.\n"),
        ("src/raw.rs", 'const SOURCE: &str = r#"STALWART"#;\n'),
        (
            "src/tests.rs",
            '#[cfg(test)] mod tests { const SOURCE: &str = "stalwart"; }\n',
        ),
    ):
        expect_architecture_error(
            lambda source_name=source_name, bypass=bypass: (
                validate_application_vendor_neutrality({source_name: bypass})
            ),
            f"self-test accepted application vendor bypass: {bypass.strip()}",
        )
    expect_architecture_error(
        lambda: validate_application_vendor_neutrality({}),
        "self-test accepted an empty application source corpus",
    )

    valid_ports_parts: list[str] = []
    for symbol, kind in MAIL_PORT_SYMBOLS.items():
        if kind == "struct":
            valid_ports_parts.append(f"pub struct {symbol};")
        elif kind == "enum":
            valid_ports_parts.append(f"pub enum {symbol} {{ Value }}")
        else:
            valid_ports_parts.append(f"pub trait {symbol} {{}}")
    valid_ports = "\n".join(valid_ports_parts)
    valid_adapter = "use copilotmail_application::mail_ports::*;\n"
    validate_mail_port_sources(valid_ports, valid_adapter)
    validate_mail_port_sources(
        valid_ports,
        valid_adapter
        + "struct Adapter; impl MailDataPlanePort for Adapter {}\n",
    )

    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports.replace(
                "pub trait MailDataPlanePort {}",
                "pub struct MailDataPlanePort;",
            ),
            valid_adapter,
        ),
        "self-test accepted MailDataPlanePort with the wrong item kind",
    )
    for bypass in (
        "pub use copilotmail_application::mail_ports::MailDataPlanePort;\n",
        "trait MailDataPlanePort {}\n",
        "use another_crate::Port as MailDataPlanePort;\n",
        "fn wrap<MailDataPlanePort>() {}\n",
        "struct Wrapper<T: Iterator<Item = Vec<u8>>, MailDataPlanePort>(T);\n",
        "impl<const N: usize, MailDataPlanePort> Adapter {}\n",
    ):
        expect_architecture_error(
            lambda bypass=bypass: validate_mail_port_sources(
                valid_ports,
                valid_adapter + bypass,
            ),
            f"self-test accepted MailDataPlanePort bypass: {bypass.strip()}",
        )

    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports,
            "// use copilotmail_application::mail_ports::*;\n",
        ),
        "self-test accepted a commented-out port import",
    )
    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports,
            "pub(crate) use copilotmail_application::mail_ports::*;\n",
        ),
        "self-test accepted a visible port import",
    )
    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports,
            valid_adapter
            + "pub use copilotmail_application::mail_ports::MailAddress;\n",
        ),
        "self-test accepted a mail port re-export",
    )
    for replacement in (
        "struct MailAddress;",
        "pub(crate) struct MailAddress;",
        "pub type MailAddress = String;",
        "pub enum MailAddress { Value }",
    ):
        expect_architecture_error(
            lambda replacement=replacement: validate_mail_port_sources(
                valid_ports,
                valid_adapter + replacement,
            ),
            f"self-test accepted adapter shadowing via {replacement}",
        )
    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports.replace("pub struct MailAddress;", "pub enum MailAddress { Value }"),
            valid_adapter,
        ),
        "self-test accepted an application port with the wrong item kind",
    )
    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports
            + "\n#[cfg(windows)]\npub enum MailAddress { ConditionalValue }\n",
            valid_adapter,
        ),
        "self-test accepted a conditional duplicate application port",
    )
    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports,
            valid_adapter + "use another_crate::MailAddress;\n",
        ),
        "self-test accepted an import that shadows a mail port",
    )
    expect_architecture_error(
        lambda: validate_mail_port_sources(
            valid_ports,
            valid_adapter + "use another_crate::{nested::*};\n",
        ),
        "self-test accepted a second top-level glob import",
    )
    for bypass in (
        "use another_crate::Other as MailAddress;\n",
        "use another_crate::{*};\n",
        "use copilotmail_application::mail_ports as mp;\n",
        "pub use mp::{*};\n",
        "pub use mp::MailAddress as Exported;\n",
        "struct r#MailAddress;\n",
        "pub type r#MailAddress = String;\n",
        "extern crate another_crate as MailAddress;\n",
        (
            "extern crate copilotmail_application as app;\n"
            "pub type ExportedAddress = app::mail_ports::MailAddress;\n"
        ),
    ):
        expect_architecture_error(
            lambda bypass=bypass: validate_mail_port_sources(
                valid_ports,
                valid_adapter + bypass,
            ),
            f"self-test accepted mail-port bypass: {bypass.strip()}",
        )
    validate_mail_port_sources(
        valid_ports,
        valid_adapter + "pub use crate::not_mail_ports_helpers::Thing;\n",
    )
    validate_mail_port_corpus(
        valid_ports,
        {
            "src/lib.rs": valid_adapter,
            "src/helper.rs": "pub struct AdapterHelper;\n",
        },
    )
    expect_architecture_error(
        lambda: validate_mail_port_corpus(
            valid_ports,
            {
                "src/lib.rs": valid_adapter + "pub mod compatibility;\n",
                "src/compatibility.rs": (
                    "pub use copilotmail_application::mail_ports::*;\n"
                ),
            },
        ),
        "self-test accepted a mail-port re-export from a secondary source",
    )
    expect_architecture_error(
        lambda: validate_mail_port_corpus(
            valid_ports,
            {
                "src/lib.rs": valid_adapter,
                "src/duplicate.rs": valid_adapter,
            },
        ),
        "self-test accepted duplicate canonical imports across the source corpus",
    )
    for source_name, bypass in (
        ("src/raw_shadow.rs", "struct r#MailAddress;\n"),
        (
            "src/extern_shadow.rs",
            "extern crate another_crate as MailAddress;\n",
        ),
        (
            "src/crate_shadow.rs",
            "mod r#copilotmail_application {}\n",
        ),
    ):
        expect_architecture_error(
            lambda source_name=source_name, bypass=bypass: validate_mail_port_corpus(
                valid_ports,
                {"src/lib.rs": valid_adapter, source_name: bypass},
            ),
            f"self-test accepted a secondary-source bypass: {bypass.strip()}",
        )
    expect_architecture_error(
        lambda: validate_mail_port_corpus(
            valid_ports,
            {"src/lib.rs": valid_adapter + "mod unclosed {\n", "src/end.rs": "}\n"},
        ),
        "self-test allowed unmatched braces to cancel across source files",
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Check Rust dependency direction and application-owned ports"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="exercise rejection logic before checking repository files",
    )
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        manifests = {
            "domain": DOMAIN_MANIFEST,
            "audit": AUDIT_MANIFEST,
            "application": APPLICATION_MANIFEST,
        }
        for layer, manifest in manifests.items():
            validate_layer_dependencies(
                layer,
                direct_dependencies(manifest),
                LAYER_ALLOWED_INTERNAL[layer],
            )
        mail_engine_dependencies = direct_dependencies(MAIL_ENGINE_MANIFEST)
        if "copilotmail-application" not in mail_engine_dependencies:
            raise ArchitectureError(
                "mail-engine adapter must depend inward on the application ports"
            )
        validate_resolved_workspace_layers()
        validate_mail_port_ownership()
    except (OSError, tomllib.TOMLDecodeError, ArchitectureError) as error:
        print(f"architecture gate failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    print(
        "architecture gate passed: domain/audit/application dependency direction "
        "and application-owned mail ports enforced"
    )


if __name__ == "__main__":
    main()
