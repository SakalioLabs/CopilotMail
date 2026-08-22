#!/usr/bin/env python3
"""Fail closed on deployment defaults that can expose a development stack.

The check intentionally uses only the Python standard library so it can run
before Docker, Compose, or third-party YAML tooling is installed.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import urllib.parse


ROOT = pathlib.Path(__file__).resolve().parents[1]
COMPOSE_GLOB = "compose*.yaml"
BASE_COMPOSE = ROOT / "deploy" / "compose.dev.yaml"
POC_COMPOSE = ROOT / "deploy" / "compose.mail-poc.yaml"
MAIL_BOOTSTRAP_COMPOSE = ROOT / "deploy" / "compose.mail-bootstrap.yaml"
HUMAN_SESSION_COMPOSE = ROOT / "deploy" / "compose.human-session.yaml"
PILOT_COMPOSE = ROOT / "deploy" / "compose.pilot.yaml"
ENV_EXAMPLE = ROOT / "deploy" / ".env.v2.example"
DOCKERIGNORE = ROOT / ".dockerignore"
GITIGNORE = ROOT / ".gitignore"
HUMAN_SESSION_KEY = ROOT / "deploy" / "secrets" / "human-session.key"
HUMAN_SESSION_KEY_CONTAINER_PATH = (
    "/run/secrets/copilotmail/human-session.key"
)
HUMAN_LOGIN_RATE_EXPRESSION = (
    "${COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE:-600}"
)
HUMAN_LOGIN_RATE_DEFAULT = "600"
NGINX_CONFIGS = (
    ROOT / "deploy" / "nginx" / "admin-app.conf",
    ROOT / "deploy" / "nginx" / "mail-app.conf",
)
WEB_FRONTEND_DOCKERFILES = {
    "admin": ROOT / "deploy" / "docker" / "Dockerfile.admin-app",
    "mail": ROOT / "deploy" / "docker" / "Dockerfile.mail-app",
}
WEB_FRONTEND_TMPFS = (
    "/tmp:uid=101,gid=101,mode=1770",
    "/var/cache/nginx:uid=101,gid=101,mode=0750",
    "/var/run:uid=101,gid=101,mode=0750",
)
PILOT_SERVICE_SCALARS = {
    "postgres": {
        "mem_limit": "2g",
        "cpus": "2.0",
        "pids_limit": "256",
        "stop_grace_period": "1m",
        "restart": "unless-stopped",
    },
    "minio": {
        "mem_limit": "2g",
        "cpus": "2.0",
        "pids_limit": "256",
        "stop_grace_period": "1m",
        "restart": "unless-stopped",
    },
    "minio-init": {
        "mem_limit": "256m",
        "cpus": "0.5",
        "pids_limit": "64",
        "stop_grace_period": "15s",
    },
    "migrate": {
        "mem_limit": "1g",
        "cpus": "1.0",
        "pids_limit": "128",
        "stop_grace_period": "30s",
    },
    "api": {
        "mem_limit": "1g",
        "cpus": "2.0",
        "pids_limit": "256",
        "stop_signal": "SIGTERM",
        "stop_grace_period": "30s",
        "restart": "unless-stopped",
    },
    "admin": {
        "mem_limit": "256m",
        "cpus": "0.5",
        "pids_limit": "128",
        "stop_grace_period": "15s",
        "restart": "unless-stopped",
    },
    "mail": {
        "mem_limit": "256m",
        "cpus": "0.5",
        "pids_limit": "128",
        "stop_grace_period": "15s",
        "restart": "unless-stopped",
    },
    "worker": {
        "mem_limit": "1g",
        "cpus": "2.0",
        "pids_limit": "256",
        "stop_signal": "SIGTERM",
        "stop_grace_period": "1m",
        "restart": "unless-stopped",
    },
    "audit-maintenance": {
        "mem_limit": "1g",
        "cpus": "1.0",
        "pids_limit": "256",
        "stop_signal": "SIGTERM",
        "stop_grace_period": "2m",
        "restart": "unless-stopped",
    },
}
STALWART_RUNTIME_SCALARS = {
    "mem_limit": "2g",
    "cpus": "2.0",
    "pids_limit": "512",
    "stop_signal": "SIGTERM",
    "stop_grace_period": "1m",
    "restart": "unless-stopped",
}
STALWART_HEALTHCHECK_URL = "http://127.0.0.1:8080/healthz/ready"
STALWART_BOOTSTRAP_ADMIN_EXPRESSION = (
    "${COPILOTMAIL_DEV_STALWART_BOOTSTRAP_ADMIN:"
    "?COPILOTMAIL_DEV_STALWART_BOOTSTRAP_ADMIN must be set}"
)
REQUIRED_MAIL_ENV = {
    "api": {
        "COPILOTMAIL_REQUIRE_MAIL_ENGINE": "true",
        "COPILOTMAIL_REQUIRE_MAIL_CONTENT": "true",
        "COPILOTMAIL_REQUIRE_MAIL_COMPOSE": "true",
        "COPILOTMAIL_REQUIRE_MAIL_EVENT_INGRESS": "true",
    },
    "worker": {
        "COPILOTMAIL_REQUIRE_MAIL_ENGINE": "true",
        "COPILOTMAIL_REQUIRE_MAIL_SUBMISSION": "true",
    },
}

PUBLIC_URL_EXPRESSION = (
    "${COPILOTMAIL_STALWART_PUBLIC_URL:"
    "?COPILOTMAIL_STALWART_PUBLIC_URL must be set}"
)
STALWART_ORIGIN_KEYS = {
    "STALWART_PUBLIC_URL",
    "COPILOTMAIL_MAIL_ENGINE_BASE_URL",
    "COPILOTMAIL_MAIL_CONTENT_BASE_URL",
    "COPILOTMAIL_MAIL_COMPOSE_BASE_URL",
    "COPILOTMAIL_MAIL_SUBMISSION_BASE_URL",
}
STALWART_ORIGIN_COUNTS = {
    "STALWART_PUBLIC_URL": 1,
    # API readiness and Worker domain projection share the same immutable
    # management-plane origin while retaining separate process boundaries.
    "COPILOTMAIL_MAIL_ENGINE_BASE_URL": 2,
    "COPILOTMAIL_MAIL_CONTENT_BASE_URL": 1,
    "COPILOTMAIL_MAIL_COMPOSE_BASE_URL": 1,
    "COPILOTMAIL_MAIL_SUBMISSION_BASE_URL": 1,
}
REQUIRED_DOCKERIGNORE_RULES = {
    ".env*",
    "deploy/.env.v2*",
    "deploy/secrets/*",
    "!deploy/secrets/README.md",
    ".local",
}
REQUIRED_GITIGNORE_RULES = {
    "/deploy/secrets/*",
    "!/deploy/secrets/README.md",
}
BASE_API_HUMAN_SESSION_ENV = {
    "COPILOTMAIL_REQUIRE_HUMAN_SESSION": (
        "${COPILOTMAIL_REQUIRE_HUMAN_SESSION:-false}"
    ),
    "COPILOTMAIL_ADMIN_OIDC_CLIENT_ID": (
        "${COPILOTMAIL_ADMIN_OIDC_CLIENT_ID:-}"
    ),
    "COPILOTMAIL_ADMIN_OIDC_SCOPE": (
        "${COPILOTMAIL_ADMIN_OIDC_SCOPE:-openid profile email}"
    ),
    "COPILOTMAIL_ADMIN_PUBLIC_ORIGIN": (
        "${COPILOTMAIL_ADMIN_PUBLIC_ORIGIN:-}"
    ),
    "COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID": (
        "${COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID:-}"
    ),
    "COPILOTMAIL_MAIL_APP_OIDC_SCOPE": (
        "${COPILOTMAIL_MAIL_APP_OIDC_SCOPE:-openid profile email}"
    ),
    "COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN": (
        "${COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN:-}"
    ),
    "COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS": (
        "${COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS:-3600}"
    ),
    "COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE": (
        HUMAN_LOGIN_RATE_EXPRESSION
    ),
    "COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE": (
        "${COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE:-false}"
    ),
    "COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE": (
        "${COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE:-}"
    ),
}
OVERLAY_API_HUMAN_SESSION_ENV = {
    "COPILOTMAIL_REQUIRE_HUMAN_AUTH": "true",
    "COPILOTMAIL_REQUIRE_HUMAN_SESSION": "true",
    "COPILOTMAIL_OIDC_ISSUER": (
        "${COPILOTMAIL_OIDC_ISSUER:"
        "?COPILOTMAIL_OIDC_ISSUER must be set}"
    ),
    "COPILOTMAIL_OIDC_AUDIENCE": (
        "${COPILOTMAIL_OIDC_AUDIENCE:"
        "?COPILOTMAIL_OIDC_AUDIENCE must be set}"
    ),
    "COPILOTMAIL_ADMIN_OIDC_CLIENT_ID": (
        "${COPILOTMAIL_ADMIN_OIDC_CLIENT_ID:"
        "?COPILOTMAIL_ADMIN_OIDC_CLIENT_ID must be set}"
    ),
    "COPILOTMAIL_ADMIN_OIDC_SCOPE": (
        "${COPILOTMAIL_ADMIN_OIDC_SCOPE:-openid profile email}"
    ),
    "COPILOTMAIL_ADMIN_PUBLIC_ORIGIN": (
        "${COPILOTMAIL_ADMIN_PUBLIC_ORIGIN:"
        "?COPILOTMAIL_ADMIN_PUBLIC_ORIGIN must be set}"
    ),
    "COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID": (
        "${COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID:"
        "?COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID must be set}"
    ),
    "COPILOTMAIL_MAIL_APP_OIDC_SCOPE": (
        "${COPILOTMAIL_MAIL_APP_OIDC_SCOPE:-openid profile email}"
    ),
    "COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN": (
        "${COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN:"
        "?COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN must be set}"
    ),
    "COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS": (
        "${COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS:-3600}"
    ),
    "COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE": (
        "${COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE:-false}"
    ),
    "COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE": (
        HUMAN_SESSION_KEY_CONTAINER_PATH
    ),
}
ENV_EXPRESSION = re.compile(
    r"\$\{(?P<name>[A-Z_][A-Z0-9_]*)(?::-(?P<default>[^}]*))?\}"
)
LOOPBACK_PORT = re.compile(
    r"^127\.0\.0\.1:[0-9]{1,5}:[0-9]{1,5}(?:/(?:tcp|udp))?$"
)


class SecurityGateError(RuntimeError):
    """Raised when a deployment file weakens a checked security invariant."""


def unquote(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {'"', "'"}:
        return value[1:-1]
    return value


def expand_compose_defaults(value: str) -> str:
    """Render only `${NAME:-default}` expressions used by published ports."""

    position = 0
    parts: list[str] = []
    for match in ENV_EXPRESSION.finditer(value):
        parts.append(value[position : match.start()])
        default = match.group("default")
        if default is None:
            raise SecurityGateError(
                f"published port variable {match.group('name')} has no safe default"
            )
        parts.append(default)
        position = match.end()
    parts.append(value[position:])
    rendered = "".join(parts)
    if "${" in rendered:
        raise SecurityGateError("published port contains an unsupported expression")
    return rendered


def port_mappings(path: pathlib.Path) -> list[tuple[int, str]]:
    mappings: list[tuple[int, str]] = []
    ports_indent: int | None = None
    for line_number, raw_line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(raw_line) - len(raw_line.lstrip())
        if ports_indent is not None:
            if indent <= ports_indent:
                ports_indent = None
            elif stripped.startswith("-"):
                mappings.append((line_number, unquote(stripped[1:].strip())))
                continue
        if stripped == "ports:":
            ports_indent = indent
        elif stripped.startswith("ports:"):
            raise SecurityGateError(
                f"{path.relative_to(ROOT)}:{line_number}: inline ports are not "
                "accepted by the security gate"
            )
    return mappings


def validate_published_ports() -> int:
    checked = 0
    compose_files = sorted((ROOT / "deploy").glob(COMPOSE_GLOB))
    if not compose_files:
        raise SecurityGateError("no deploy/compose*.yaml files found")
    for path in compose_files:
        for line_number, mapping in port_mappings(path):
            try:
                rendered = expand_compose_defaults(mapping)
            except SecurityGateError as error:
                raise SecurityGateError(
                    f"{path.relative_to(ROOT)}:{line_number}: {error}"
                ) from error
            if not LOOPBACK_PORT.fullmatch(rendered):
                raise SecurityGateError(
                    f"{path.relative_to(ROOT)}:{line_number}: published port "
                    f"default must bind 127.0.0.1, got {rendered!r}"
                )
            host_port = int(rendered.split(":", 2)[1])
            container_port = int(rendered.split(":", 2)[2].split("/", 1)[0])
            if not (1 <= host_port <= 65535 and 1 <= container_port <= 65535):
                raise SecurityGateError(
                    f"{path.relative_to(ROOT)}:{line_number}: port is out of range"
                )
            checked += 1
    return checked


def parse_simple_assignments(path: pathlib.Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if "=" not in stripped:
            raise SecurityGateError(
                f"{path.relative_to(ROOT)}:{line_number}: invalid environment entry"
            )
        name, value = stripped.split("=", 1)
        if name in values:
            raise SecurityGateError(
                f"{path.relative_to(ROOT)}:{line_number}: duplicate {name}"
            )
        values[name] = value
    return values


def service_block(path: pathlib.Path, service: str) -> str:
    """Return one top-level Compose service block without parsing arbitrary YAML."""

    lines = path.read_text(encoding="utf-8").splitlines()
    marker = f"  {service}:"
    try:
        start = next(
            index for index, line in enumerate(lines) if line.rstrip() == marker
        )
    except StopIteration as error:
        raise SecurityGateError(
            f"{path.relative_to(ROOT)} is missing service {service!r}"
        ) from error
    end = len(lines)
    for index in range(start + 1, len(lines)):
        line = lines[index]
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            indent = len(line) - len(line.lstrip())
            if indent <= 2:
                end = index
                break
    return "\n".join(lines[start:end])


def validate_service_scalars(
    block: str, expected: dict[str, str], context: str
) -> None:
    """Require exact service-level scalar values while ignoring nested maps."""

    found: dict[str, str] = {}
    assignment = re.compile(r"^    ([a-z][a-z0-9_-]*):\s*(.*?)\s*$")
    for line_number, line in enumerate(block.splitlines(), start=1):
        match = assignment.match(line)
        if not match or match.group(1) not in expected:
            continue
        key, value = match.groups()
        if key in found:
            raise SecurityGateError(
                f"{context}:{line_number}: duplicate service scalar {key}"
            )
        found[key] = unquote(value)
    missing = expected.keys() - found.keys()
    if missing:
        raise SecurityGateError(
            f"{context} is missing runtime limits: " + ", ".join(sorted(missing))
        )
    drifted = {
        key: value
        for key, value in found.items()
        if value != expected[key]
    }
    if drifted:
        raise SecurityGateError(
            f"{context} runtime limits drifted: " + ", ".join(sorted(drifted))
        )


def validate_pilot_runtime() -> None:
    text = PILOT_COMPOSE.read_text(encoding="utf-8")
    if text.count("name: copilotmail-v2-pilot") != 1:
        raise SecurityGateError(
            "deploy/compose.pilot.yaml must use the isolated Pilot project name"
        )
    service_names = set(
        re.findall(r"(?m)^  ([a-z][a-z0-9-]*):\s*$", text)
    )
    expected_names = set(PILOT_SERVICE_SCALARS)
    if service_names != expected_names:
        raise SecurityGateError(
            "Pilot runtime policy service set drifted: "
            + ", ".join(sorted(service_names ^ expected_names))
        )
    if re.search(r"(?m)^    ports\s*:", text):
        raise SecurityGateError(
            "Pilot runtime policy must not add or widen published ports"
        )
    for service, expected in PILOT_SERVICE_SCALARS.items():
        validate_service_scalars(
            service_block(PILOT_COMPOSE, service),
            expected,
            f"deploy/compose.pilot.yaml {service}",
        )

    worker = service_block(PILOT_COMPOSE, "worker")
    for invariant in (
        'test: ["CMD", "copilotmail-control", "check"]',
        "interval: 30s",
        "timeout: 10s",
        "retries: 3",
        "start_period: 20s",
    ):
        if worker.count(invariant) != 1:
            raise SecurityGateError(
                "Pilot Worker healthcheck must contain exactly one "
                f"{invariant!r}"
            )

    validate_service_scalars(
        service_block(POC_COMPOSE, "stalwart"),
        STALWART_RUNTIME_SCALARS,
        "deploy/compose.mail-poc.yaml stalwart",
    )


def validate_mail_fail_closed() -> None:
    """Keep normal mail startup strict and isolate one-time recovery access."""

    poc_text = POC_COMPOSE.read_text(encoding="utf-8")
    if "STALWART_RECOVERY_ADMIN" in poc_text:
        raise SecurityGateError(
            "normal Stalwart startup must not receive a recovery administrator"
        )

    stalwart = service_block(POC_COMPOSE, "stalwart")
    health_values = selected_mapping_values(
        stalwart,
        {"STALWART_HEALTHCHECK_URL": STALWART_HEALTHCHECK_URL},
        "deploy/compose.mail-poc.yaml stalwart environment",
    )
    if health_values["STALWART_HEALTHCHECK_URL"] != STALWART_HEALTHCHECK_URL:
        raise SecurityGateError(
            "normal Stalwart startup must use the local readiness endpoint"
        )

    for service, expected in REQUIRED_MAIL_ENV.items():
        block = service_block(POC_COMPOSE, service)
        values = selected_mapping_values(
            block,
            expected,
            f"deploy/compose.mail-poc.yaml {service} environment",
        )
        drifted = {
            key: value
            for key, value in values.items()
            if value != expected[key]
        }
        if drifted:
            raise SecurityGateError(
                f"deploy/compose.mail-poc.yaml {service} must fail closed: "
                + ", ".join(sorted(drifted))
            )
        healthy_dependency = (
            "    depends_on:\n"
            "      stalwart:\n"
            "        condition: service_healthy"
        )
        if block.count(healthy_dependency) != 1:
            raise SecurityGateError(
                f"deploy/compose.mail-poc.yaml {service} must wait for "
                "Stalwart health"
            )

    bootstrap_text = MAIL_BOOTSTRAP_COMPOSE.read_text(encoding="utf-8")
    service_names = set(
        re.findall(r"(?m)^  ([a-z][a-z0-9-]*):\s*$", bootstrap_text)
    )
    if service_names != {"stalwart"}:
        raise SecurityGateError(
            "Stalwart bootstrap overlay must target only the stalwart service"
        )
    bootstrap = service_block(MAIL_BOOTSTRAP_COMPOSE, "stalwart")
    service_keys = set(
        re.findall(r"(?m)^    ([a-z][a-z0-9_-]*):", bootstrap)
    )
    if service_keys != {"environment"}:
        raise SecurityGateError(
            "Stalwart bootstrap overlay may only add environment configuration"
        )
    bootstrap_assignments = set(
        re.findall(r"(?m)^\s+([A-Z][A-Z0-9_]*):", bootstrap)
    )
    if bootstrap_assignments != {"STALWART_RECOVERY_ADMIN"}:
        raise SecurityGateError(
            "Stalwart bootstrap overlay may only inject "
            "STALWART_RECOVERY_ADMIN"
        )
    bootstrap_values = selected_mapping_values(
        bootstrap,
        {"STALWART_RECOVERY_ADMIN": STALWART_BOOTSTRAP_ADMIN_EXPRESSION},
        "deploy/compose.mail-bootstrap.yaml stalwart environment",
    )
    if (
        bootstrap_values["STALWART_RECOVERY_ADMIN"]
        != STALWART_BOOTSTRAP_ADMIN_EXPRESSION
    ):
        raise SecurityGateError(
            "Stalwart bootstrap administrator must be explicitly supplied"
        )

    for compose_path in sorted((ROOT / "deploy").glob(COMPOSE_GLOB)):
        if compose_path == MAIL_BOOTSTRAP_COMPOSE:
            continue
        if "STALWART_RECOVERY_ADMIN" in compose_path.read_text(encoding="utf-8"):
            raise SecurityGateError(
                f"{compose_path.relative_to(ROOT)} must not receive the "
                "Stalwart recovery administrator"
            )


def validate_compose_hazard_text(text: str, context: str) -> None:
    """Reject Compose capabilities that bypass the intended container boundary."""

    hazardous_keys = {"cap_add", "devices"}
    host_namespace_keys = {"network_mode", "pid", "ipc"}
    assignment = re.compile(
        r"^\s*(privileged|network_mode|pid|ipc|cap_add|devices)\s*:\s*(.*?)\s*$"
    )
    for line_number, raw_line in enumerate(text.splitlines(), start=1):
        stripped = raw_line.lstrip()
        if not stripped or stripped.startswith("#"):
            continue
        line = raw_line.split("#", 1)[0].rstrip()
        if "docker.sock" in line:
            raise SecurityGateError(
                f"{context}:{line_number}: Docker socket mounts are forbidden"
            )
        match = assignment.match(line)
        if not match:
            continue
        key, raw_value = match.groups()
        value = unquote(raw_value).casefold()
        if key in hazardous_keys:
            raise SecurityGateError(
                f"{context}:{line_number}: {key} is forbidden"
            )
        if key in host_namespace_keys and (value == "host" or "${" in value):
            raise SecurityGateError(
                f"{context}:{line_number}: {key}: host is forbidden"
            )
        if key == "privileged" and value != "false":
            raise SecurityGateError(
                f"{context}:{line_number}: privileged containers are forbidden"
            )


def validate_bind_source(source: str, context: str, line_number: int) -> None:
    allowed_prefixes = ("./secrets/", "./observability/")
    if (
        not source.startswith(allowed_prefixes)
        or ".." in source.split("/")
        or "\\" in source
    ):
        raise SecurityGateError(
            f"{context}:{line_number}: host bind source is outside the "
            "read-only deployment allowlist"
        )


def validate_compose_bind_mounts(text: str, context: str) -> None:
    """Allow only repository-scoped, read-only secret/config bind mounts."""

    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = re.match(r"^(    )volumes:\s*$", line)
        if not match:
            continue
        volumes_indent = len(match.group(1))
        cursor = index + 1
        while cursor < len(lines):
            raw_entry = lines[cursor]
            stripped = raw_entry.strip()
            indent = len(raw_entry) - len(raw_entry.lstrip())
            if stripped and indent <= volumes_indent:
                break
            if not stripped or stripped.startswith("#"):
                cursor += 1
                continue
            if indent != volumes_indent + 2 or not stripped.startswith("- "):
                cursor += 1
                continue

            entry = stripped[2:].strip()
            type_match = re.fullmatch(r"type:\s*(.*?)", entry)
            if type_match and unquote(type_match.group(1)).casefold() == "bind":
                fields: dict[str, tuple[str, int]] = {}
                nested = cursor + 1
                while nested < len(lines):
                    raw_field = lines[nested]
                    field_stripped = raw_field.strip()
                    field_indent = len(raw_field) - len(raw_field.lstrip())
                    if field_stripped and field_indent <= volumes_indent + 2:
                        break
                    if not field_stripped or field_stripped.startswith("#"):
                        nested += 1
                        continue
                    field_match = re.match(
                        r"^\s+(source|target|read_only):\s*(.*?)\s*$",
                        raw_field,
                    )
                    if field_indent != volumes_indent + 4 or not field_match:
                        raise SecurityGateError(
                            f"{context}:{nested + 1}: bind mount uses an "
                            "unsupported nested option"
                        )
                    key, value = field_match.groups()
                    if key in fields:
                        raise SecurityGateError(
                            f"{context}:{nested + 1}: duplicate bind mount {key}"
                        )
                    fields[key] = (unquote(value), nested + 1)
                    nested += 1
                source, source_line = fields.get("source", ("", cursor + 1))
                target, _ = fields.get("target", ("", cursor + 1))
                read_only, _ = fields.get("read_only", ("", cursor + 1))
                validate_bind_source(source, context, source_line)
                if (
                    not target.startswith("/")
                    or target == "/"
                    or read_only.casefold() != "true"
                ):
                    raise SecurityGateError(
                        f"{context}:{cursor + 1}: bind mount must have an "
                        "absolute non-root target and read_only: true"
                    )
                cursor = nested
                continue
            if type_match:
                raise SecurityGateError(
                    f"{context}:{cursor + 1}: unsupported long-form volume type"
                )
            if entry.startswith("{"):
                raise SecurityGateError(
                    f"{context}:{cursor + 1}: inline volume mappings are "
                    "not accepted by the deployment security gate"
                )

            value = unquote(entry)
            source = value.split(":", 1)[0]
            looks_like_bind = (
                "/" in source
                or "\\" in source
                or source.startswith((".", "~", "$"))
                or bool(re.match(r"^[A-Za-z]:", value))
            )
            if looks_like_bind:
                validate_bind_source(source, context, cursor + 1)
                parts = value.rsplit(":", 2)
                if len(parts) != 3 or parts[2] != "ro" or not parts[1].startswith("/"):
                    raise SecurityGateError(
                        f"{context}:{cursor + 1}: short bind mount must use "
                        "an absolute target and explicit :ro mode"
                    )
            cursor += 1


def validate_compose_hazards() -> None:
    for path in sorted((ROOT / "deploy").glob(COMPOSE_GLOB)):
        text = path.read_text(encoding="utf-8")
        context = str(path.relative_to(ROOT))
        validate_compose_hazard_text(text, context)
        validate_compose_bind_mounts(text, context)


def selected_mapping_values(
    text: str, expected: dict[str, str], context: str
) -> dict[str, str]:
    found: dict[str, str] = {}
    assignment = re.compile(r"^\s*([A-Z][A-Z0-9_]*):\s*(.*?)\s*$")
    for line_number, line in enumerate(text.splitlines(), start=1):
        match = assignment.match(line)
        if not match or match.group(1) not in expected:
            continue
        key, value = match.groups()
        if key in found:
            raise SecurityGateError(
                f"{context}:{line_number}: duplicate {key}"
            )
        found[key] = unquote(value)
    missing = expected.keys() - found.keys()
    if missing:
        raise SecurityGateError(
            f"{context} is missing: " + ", ".join(sorted(missing))
        )
    return found


def url_origin(value: str, name: str) -> tuple[str, str, int]:
    parsed = urllib.parse.urlsplit(value)
    if (
        parsed.scheme not in {"http", "https"}
        or not parsed.hostname
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
    ):
        raise SecurityGateError(f"{name} must be an absolute credential-free URL")
    try:
        port = parsed.port or (443 if parsed.scheme == "https" else 80)
    except ValueError as error:
        raise SecurityGateError(f"{name} has an invalid port") from error
    return parsed.scheme, parsed.hostname.casefold(), port


def compose_origin_values(path: pathlib.Path) -> dict[str, list[str]]:
    found: dict[str, list[str]] = {}
    assignment = re.compile(r"^\s*([A-Z][A-Z0-9_]*):\s*(.*?)\s*$")
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        match = assignment.match(line)
        if not match or match.group(1) not in STALWART_ORIGIN_KEYS:
            continue
        key, value = match.groups()
        found.setdefault(key, []).append(unquote(value))
    return found


def validate_stalwart_origin() -> None:
    values = compose_origin_values(POC_COMPOSE)
    missing = STALWART_ORIGIN_KEYS - values.keys()
    if missing:
        raise SecurityGateError(
            "Stalwart origin settings missing from PoC Compose: "
            + ", ".join(sorted(missing))
        )
    drifted = {
        key
        for key, entries in values.items()
        if any(value != PUBLIC_URL_EXPRESSION for value in entries)
    }
    if drifted:
        raise SecurityGateError(
            "Stalwart and adapter origins must all use the required "
            "COPILOTMAIL_STALWART_PUBLIC_URL expression: "
            + ", ".join(sorted(drifted))
        )
    wrong_counts = {
        key: len(values.get(key, []))
        for key, expected in STALWART_ORIGIN_COUNTS.items()
        if len(values.get(key, [])) != expected
    }
    if wrong_counts:
        raise SecurityGateError(
            "Stalwart adapter origin ownership drifted: "
            + ", ".join(
                f"{key}={count}" for key, count in sorted(wrong_counts.items())
            )
        )

    compose_text = POC_COMPOSE.read_text(encoding="utf-8")
    required_data_plane_id = (
        "${COPILOTMAIL_MAIL_DATA_PLANE_ID:-"
        "018f305d-3d0f-7b2a-9d68-7f4a3f6e2101}"
    )
    if compose_text.count(
        "COPILOTMAIL_MAIL_DATA_PLANE_ID: " + required_data_plane_id
    ) != 3:
        raise SecurityGateError(
            "PoC Migrate, API, and Worker must share the governed mail data-plane ID"
        )
    if compose_text.count(
        "COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN: " + PUBLIC_URL_EXPRESSION
    ) != 3:
        raise SecurityGateError(
            "PoC Migrate, API, and Worker must share the exact Stalwart data-plane origin"
        )

    environment = parse_simple_assignments(ENV_EXAMPLE)
    if environment.get("COPILOTMAIL_DEV_BIND_HOST") != "127.0.0.1":
        raise SecurityGateError(
            "COPILOTMAIL_DEV_BIND_HOST must default to 127.0.0.1"
        )
    public_url = environment.get("COPILOTMAIL_STALWART_PUBLIC_URL", "")
    smoke_url = environment.get("COPILOTMAIL_STALWART_SMOKE_JMAP_URL", "")
    if url_origin(public_url, "COPILOTMAIL_STALWART_PUBLIC_URL") != url_origin(
        smoke_url, "COPILOTMAIL_STALWART_SMOKE_JMAP_URL"
    ):
        raise SecurityGateError(
            "Stalwart public URL and smoke JMAP URL have different origins"
        )
    smoke_path = urllib.parse.urlsplit(smoke_url).path
    if smoke_path != "/.well-known/jmap":
        raise SecurityGateError(
            "COPILOTMAIL_STALWART_SMOKE_JMAP_URL must use /.well-known/jmap"
        )
    if urllib.parse.urlsplit(public_url).path not in {"", "/"}:
        raise SecurityGateError(
            "development COPILOTMAIL_STALWART_PUBLIC_URL must not add a path prefix"
        )
    try:
        published_port = int(environment["COPILOTMAIL_DEV_STALWART_HTTP_PORT"])
    except (KeyError, ValueError) as error:
        raise SecurityGateError(
            "COPILOTMAIL_DEV_STALWART_HTTP_PORT must be an integer"
        ) from error
    if url_origin(public_url, "COPILOTMAIL_STALWART_PUBLIC_URL")[2] != published_port:
        raise SecurityGateError(
            "Stalwart public URL port differs from the published HTTP port"
        )


def validate_human_session_deployment() -> None:
    base_api = service_block(BASE_COMPOSE, "api")
    base_values = selected_mapping_values(
        base_api,
        BASE_API_HUMAN_SESSION_ENV,
        "deploy/compose.dev.yaml api environment",
    )
    drifted_base = {
        key: value
        for key, value in base_values.items()
        if value != BASE_API_HUMAN_SESSION_ENV[key]
    }
    if drifted_base:
        raise SecurityGateError(
            "base API Human Session settings must remain disabled or empty: "
            + ", ".join(sorted(drifted_base))
        )

    stale_oidc = re.compile(r"COPILOTMAIL_[A-Z0-9_]*OIDC[A-Z0-9_]*")
    for service in ("admin", "mail"):
        stale = sorted(set(stale_oidc.findall(service_block(BASE_COMPOSE, service))))
        if stale:
            raise SecurityGateError(
                f"deploy/compose.dev.yaml {service} still receives browser OIDC "
                "configuration: "
                + ", ".join(stale)
            )

    overlay_api = service_block(HUMAN_SESSION_COMPOSE, "api")
    overlay_values = selected_mapping_values(
        overlay_api,
        OVERLAY_API_HUMAN_SESSION_ENV,
        "deploy/compose.human-session.yaml api environment",
    )
    drifted_overlay = {
        key: value
        for key, value in overlay_values.items()
        if value != OVERLAY_API_HUMAN_SESSION_ENV[key]
    }
    if drifted_overlay:
        raise SecurityGateError(
            "Human Session overlay does not fail closed: "
            + ", ".join(sorted(drifted_overlay))
        )
    if re.search(
        r"(?m)^\s+COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE\s*:",
        overlay_api,
    ):
        raise SecurityGateError(
            "Human login admission must have one default in the base API "
            "environment; the Human Session overlay must inherit it"
        )

    overlay_text = HUMAN_SESSION_COMPOSE.read_text(encoding="utf-8")
    service_names = set(
        re.findall(r"(?m)^  ([a-z][a-z0-9-]*):\s*$", overlay_text)
    )
    if service_names != {"api"}:
        raise SecurityGateError(
            "Human Session secret overlay must target only the API service"
        )
    secret_mount = re.compile(
        r"(?m)^\s+- type: bind\s*\n"
        r"\s+source: \./secrets/human-session\.key\s*\n"
        rf"\s+target: {re.escape(HUMAN_SESSION_KEY_CONTAINER_PATH)}\s*\n"
        r"\s+read_only: true\s*$"
    )
    if len(secret_mount.findall(overlay_api)) != 1:
        raise SecurityGateError(
            "Human Session key must have one read-only API bind mount"
        )
    for compose_path in sorted((ROOT / "deploy").glob(COMPOSE_GLOB)):
        if compose_path == HUMAN_SESSION_COMPOSE:
            continue
        if "human-session.key" in compose_path.read_text(encoding="utf-8"):
            raise SecurityGateError(
                f"{compose_path.relative_to(ROOT)} must not mount the Human "
                "Session key"
            )

    environment = parse_simple_assignments(ENV_EXAMPLE)
    if environment.get("COPILOTMAIL_REQUIRE_HUMAN_SESSION") != "false":
        raise SecurityGateError(
            "COPILOTMAIL_REQUIRE_HUMAN_SESSION must default to false"
        )
    if (
        environment.get("COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE")
        != HUMAN_LOGIN_RATE_DEFAULT
    ):
        raise SecurityGateError(
            "COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE must default to 600"
        )
    if "COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE" in environment:
        raise SecurityGateError(
            "the Human Session encryption key path belongs in the secret "
            "overlay, not .env.v2.example"
        )
    if HUMAN_SESSION_KEY.exists():
        if not HUMAN_SESSION_KEY.is_file() or HUMAN_SESSION_KEY.stat().st_size != 32:
            raise SecurityGateError(
                "deploy/secrets/human-session.key must contain exactly 32 bytes"
            )


def nginx_location_block(path: pathlib.Path, declaration: str) -> str:
    lines = path.read_text(encoding="utf-8").splitlines()
    try:
        start = next(
            index
            for index, line in enumerate(lines)
            if line.strip() == declaration
        )
    except StopIteration as error:
        raise SecurityGateError(
            f"{path.relative_to(ROOT)} is missing {declaration!r}"
        ) from error
    depth = 0
    for end in range(start, len(lines)):
        depth += lines[end].count("{") - lines[end].count("}")
        if depth == 0:
            return "\n".join(lines[start : end + 1])
    raise SecurityGateError(
        f"{path.relative_to(ROOT)} has an unterminated sensitive auth location"
    )


def validate_sensitive_auth_logging() -> None:
    declaration = (
        "location ~ ^/api/v2/auth/(admin|mail)/(login|callback)$ {"
    )
    route_pattern = re.compile(r"^/api/v2/auth/(admin|mail)/(login|callback)$")
    expected_routes = {
        f"/api/v2/auth/{client}/{action}"
        for client in ("admin", "mail")
        for action in ("login", "callback")
    }
    if {
        route for route in expected_routes if route_pattern.fullmatch(route)
    } != expected_routes:
        raise SecurityGateError(
            "internal error: sensitive Human auth route pattern drifted"
        )
    for path in NGINX_CONFIGS:
        block = nginx_location_block(path, declaration)
        for directive in (
            "access_log off;",
            "error_log /dev/null emerg;",
            "proxy_pass http://api:8081;",
        ):
            if block.count(directive) != 1:
                raise SecurityGateError(
                    f"{path.relative_to(ROOT)} sensitive auth location must "
                    f"contain exactly one {directive!r}"
                )


def yaml_sequence_values(text: str, key: str, context: str) -> list[str]:
    lines = text.splitlines()
    matches = [
        (index, len(line) - len(line.lstrip()))
        for index, line in enumerate(lines)
        if line.strip() == f"{key}:"
    ]
    if len(matches) != 1:
        raise SecurityGateError(
            f"{context} must contain exactly one block sequence {key!r}"
        )
    start, key_indent = matches[0]
    values: list[str] = []
    for line in lines[start + 1 :]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= key_indent:
            break
        if not stripped.startswith("- "):
            raise SecurityGateError(
                f"{context} {key!r} must use a simple block sequence"
            )
        values.append(unquote(stripped[2:].strip()))
    return values


def validate_web_frontend_service_block(block: str, context: str) -> None:
    for invariant in (
        'user: "101:101"',
        "read_only: true",
        'security_opt: ["no-new-privileges:true"]',
        'cap_drop: ["ALL"]',
    ):
        if block.count(invariant) != 1:
            raise SecurityGateError(
                f"{context} must contain exactly one {invariant!r}"
            )
    if tuple(yaml_sequence_values(block, "tmpfs", context)) != WEB_FRONTEND_TMPFS:
        raise SecurityGateError(
            f"{context} must mount exactly the ownership-scoped Nginx tmpfs paths"
        )
    if re.search(r"(?m)^    volumes\s*:", block):
        raise SecurityGateError(f"{context} must not add writable volumes")


def validate_final_stage_nginx_user(text: str, context: str) -> None:
    lines = text.splitlines()
    from_lines = [
        index
        for index, line in enumerate(lines)
        if re.match(r"^\s*FROM(?:\s|$)", line, re.IGNORECASE)
    ]
    if not from_lines:
        raise SecurityGateError(f"{context} has no FROM instruction")
    users: list[str] = []
    for line in lines[from_lines[-1] + 1 :]:
        instruction = line.split("#", 1)[0].strip()
        match = re.match(r"^USER\s+(.+?)\s*$", instruction, re.IGNORECASE)
        if match:
            users.append(match.group(1))
    if users != ["nginx"]:
        raise SecurityGateError(
            f"{context} final stage must set USER nginx exactly once and last"
        )
    logical_text = text.replace("\\\n", " ")
    uid_assertion = re.compile(
        r'RUN\s+test\s+"\$\(id -u nginx\):\$\(id -g nginx\)"'
        r'\s*=\s*"101:101"'
    )
    if len(uid_assertion.findall(logical_text)) != 1:
        raise SecurityGateError(
            f"{context} must fail closed if the nginx UID/GID drifts"
        )


def validate_web_frontend_sandbox() -> None:
    """Keep Nginx runnable without restoring root or Linux capabilities."""

    for service, dockerfile in WEB_FRONTEND_DOCKERFILES.items():
        context = f"deploy/compose.dev.yaml {service}"
        validate_web_frontend_service_block(
            service_block(BASE_COMPOSE, service), context
        )
        validate_final_stage_nginx_user(
            dockerfile.read_text(encoding="utf-8"),
            str(dockerfile.relative_to(ROOT)),
        )


def validate_dockerignore() -> None:
    rules = {
        line.strip()
        for line in DOCKERIGNORE.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }
    missing = REQUIRED_DOCKERIGNORE_RULES - rules
    if missing:
        raise SecurityGateError(
            ".dockerignore does not exclude deployment secrets/build-local state: "
            + ", ".join(sorted(missing))
        )
    forbidden_negations = {
        rule
        for rule in rules
        if rule.startswith("!")
        and rule != "!deploy/secrets/README.md"
        and (
            ".env" in rule
            or ".local" in rule
            or rule.startswith("!deploy/secrets/")
        )
    }
    if forbidden_negations:
        raise SecurityGateError(
            ".dockerignore re-includes deployment secret paths: "
            + ", ".join(sorted(forbidden_negations))
        )

    git_rules = {
        line.strip()
        for line in GITIGNORE.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }
    missing_git_rules = REQUIRED_GITIGNORE_RULES - git_rules
    if missing_git_rules:
        raise SecurityGateError(
            ".gitignore does not exclude deployment secrets: "
            + ", ".join(sorted(missing_git_rules))
        )


def self_test() -> None:
    valid = (
        "${COPILOTMAIL_DEV_BIND_HOST:-127.0.0.1}:"
        "${COPILOTMAIL_HTTP_PORT:-8080}:8080"
    )
    combined = "${COPILOTMAIL_BIND:-127.0.0.1:9090}:9090"
    for candidate in (valid, combined):
        if not LOOPBACK_PORT.fullmatch(expand_compose_defaults(candidate)):
            raise SecurityGateError("self-test rejected a loopback-bound port")
    for candidate in (
        "${COPILOTMAIL_HTTP_PORT:-8080}:8080",
        "0.0.0.0:8080:8080",
        "8080:8080",
    ):
        if LOOPBACK_PORT.fullmatch(expand_compose_defaults(candidate)):
            raise SecurityGateError("self-test accepted an exposed port")
    if url_origin(
        "http://host.docker.internal:8082", "public"
    ) != url_origin(
        "http://host.docker.internal:8082/.well-known/jmap", "smoke"
    ):
        raise SecurityGateError("self-test failed equal-origin normalization")
    if url_origin("http://127.0.0.1:8082", "public") == url_origin(
        "http://host.docker.internal:8082/.well-known/jmap", "smoke"
    ):
        raise SecurityGateError("self-test accepted origin drift")

    valid_web_block = """\
  admin:
    user: "101:101"
    read_only: true
    tmpfs:
      - /tmp:uid=101,gid=101,mode=1770
      - /var/cache/nginx:uid=101,gid=101,mode=0750
      - /var/run:uid=101,gid=101,mode=0750
    security_opt: ["no-new-privileges:true"]
    cap_drop: ["ALL"]
"""
    validate_web_frontend_service_block(valid_web_block, "self-test web")
    invalid_web_blocks = (
        valid_web_block.replace('user: "101:101"', 'user: "0:0"'),
        valid_web_block.replace("mode=1770", "mode=1777"),
        valid_web_block.replace(
            "    security_opt:",
            "      - /extra:uid=101,gid=101,mode=0750\n"
            "    security_opt:",
        ),
        valid_web_block + "    volumes:\n      - /host:/container\n",
    )
    for invalid_block in invalid_web_blocks:
        try:
            validate_web_frontend_service_block(
                invalid_block, "self-test invalid web"
            )
        except SecurityGateError:
            pass
        else:
            raise SecurityGateError(
                "self-test accepted a weakened web frontend sandbox"
            )

    valid_runtime_block = """\
  worker:
    mem_limit: 1g
    cpus: 2.0
    pids_limit: 256
    stop_signal: SIGTERM
    stop_grace_period: 1m
    restart: unless-stopped
"""
    validate_service_scalars(
        valid_runtime_block,
        PILOT_SERVICE_SCALARS["worker"],
        "self-test Pilot Worker",
    )
    try:
        validate_service_scalars(
            valid_runtime_block.replace(
                "stop_signal: SIGTERM", "stop_signal: SIGKILL"
            ),
            PILOT_SERVICE_SCALARS["worker"],
            "self-test invalid Pilot Worker",
        )
    except SecurityGateError:
        pass
    else:
        raise SecurityGateError(
            "self-test accepted a Pilot Worker that bypasses graceful shutdown"
        )

    valid_compose = """\
services:
  api:
    privileged: false
    volumes:
      - ./secrets/token:/run/secrets/copilotmail/token:ro
"""
    validate_compose_hazard_text(valid_compose, "self-test safe Compose")
    validate_compose_bind_mounts(valid_compose, "self-test safe Compose")
    for invalid_compose in (
        valid_compose.replace("privileged: false", "privileged: true"),
        valid_compose.replace("privileged: false", "network_mode: host"),
        valid_compose.replace(
            "privileged: false", "network_mode: ${NETWORK_MODE:-host}"
        ),
        valid_compose.replace("privileged: false", 'pid: "host"'),
        valid_compose.replace("privileged: false", "ipc: host"),
        valid_compose.replace("privileged: false", "cap_add: []"),
        valid_compose.replace("privileged: false", "devices:"),
        valid_compose.replace(
            "./secrets/token:/run/secrets/copilotmail/token:ro",
            "/run/docker.sock:/run/docker.sock",
        ),
    ):
        try:
            validate_compose_hazard_text(
                invalid_compose, "self-test invalid Compose"
            )
        except SecurityGateError:
            pass
        else:
            raise SecurityGateError(
                "self-test accepted a Compose container-boundary bypass"
            )
    invalid_bind_mounts = (
        valid_compose.replace(
            "./secrets/token:/run/secrets/copilotmail/token:ro",
            "/etc:/host-etc:ro",
        ),
        valid_compose.replace(
            "./secrets/token:/run/secrets/copilotmail/token:ro",
            "../outside:/run/secrets/copilotmail/token:ro",
        ),
        valid_compose.replace(
            "./secrets/token:/run/secrets/copilotmail/token:ro",
            "./secrets/token:/run/secrets/copilotmail/token:rw",
        ),
        valid_compose.replace(
            "- ./secrets/token:/run/secrets/copilotmail/token:ro",
            "- type: bind\n"
            "        source: ./secrets/token\n"
            "        target: /run/secrets/copilotmail/token",
        ),
    )
    for invalid_compose in invalid_bind_mounts:
        try:
            validate_compose_bind_mounts(
                invalid_compose, "self-test invalid bind mount"
            )
        except SecurityGateError:
            pass
        else:
            raise SecurityGateError(
                "self-test accepted a writable or broad host bind mount"
            )

    valid_dockerfile = """\
FROM node:22 AS builder
USER root
FROM nginx:1
RUN test "$(id -u nginx):$(id -g nginx)" = "101:101"
USER nginx
"""
    validate_final_stage_nginx_user(valid_dockerfile, "self-test Dockerfile")
    for invalid_dockerfile in (
        valid_dockerfile.replace("USER nginx", "USER root"),
        valid_dockerfile + "USER root\n",
        valid_dockerfile.replace(' = "101:101"', ' = "0:0"'),
    ):
        try:
            validate_final_stage_nginx_user(
                invalid_dockerfile, "self-test invalid Dockerfile"
            )
        except SecurityGateError:
            pass
        else:
            raise SecurityGateError(
                "self-test accepted a weakened final Nginx image stage"
            )


def main() -> None:
    parser = argparse.ArgumentParser(description="Check Compose deployment security")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="exercise rejection logic before checking repository files",
    )
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        checked_ports = validate_published_ports()
        validate_stalwart_origin()
        validate_mail_fail_closed()
        validate_human_session_deployment()
        validate_pilot_runtime()
        validate_compose_hazards()
        validate_sensitive_auth_logging()
        validate_web_frontend_sandbox()
        validate_dockerignore()
    except (OSError, SecurityGateError) as error:
        print(f"compose security gate failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    print(
        "compose security gate passed: "
        f"{checked_ports} loopback-bound published ports, "
        "one Stalwart origin, mail fail-closed and bootstrap isolated, "
        "Human Session key isolated, "
        "sensitive auth logging disabled, non-root web sandboxes enforced, "
        "container boundary and read-only bind allowlist enforced, "
        "Pilot runtime bounded, "
        "deployment secrets excluded"
    )


if __name__ == "__main__":
    main()
