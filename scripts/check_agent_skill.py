#!/usr/bin/env python3
"""Fail closed when the distributable Agent Skill drifts from the V2 API."""

from __future__ import annotations

import argparse
import importlib.util
import os
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / "skills" / "copilotmail"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"agent skill governance failed: {message}")


def load_client():
    path = SKILL / "scripts" / "copilotmail.py"
    spec = importlib.util.spec_from_file_location("copilotmail_skill_client", path)
    require(spec is not None and spec.loader is not None, "client cannot be imported")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> None:
    skill_text = (SKILL / "SKILL.md").read_text(encoding="utf-8")
    api_text = (SKILL / "references" / "api.md").read_text(encoding="utf-8")
    agent_text = (SKILL / "agents" / "openai.yaml").read_text(encoding="utf-8")
    openapi_text = (ROOT / "docs" / "openapi-v2.yaml").read_text(encoding="utf-8")
    combined = "\n".join((skill_text, api_text, agent_text))

    require(skill_text.startswith("---\n"), "SKILL.md frontmatter is missing")
    require("\nname: copilotmail\n" in skill_text, "skill name is not copilotmail")
    require("\ndescription:" in skill_text, "skill description is missing")
    require("$copilotmail" in agent_text, "default prompt must invoke $copilotmail")

    for legacy in ("/v1/", "X-Admin-Key", "?token=", "address and password"):
        require(legacy not in combined, f"legacy V1 guidance remains: {legacy}")

    required_paths = (
        "/oauth2/token",
        "/api/v2/agent/mail-policy",
        "/api/v2/agent/mailboxes/{mailbox_id}/messages/{message_id}/content",
        "/api/v2/agent/mailboxes/{mailbox_id}/processing-subscription",
        "/api/v2/agent/processing-subscriptions/{subscription_id}",
        "/api/v2/agent/processing-subscriptions/{subscription_id}/claims",
        "/api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters",
        "/api/v2/mailboxes/{mailbox_id}/drafts",
        "/api/v2/mailboxes/{mailbox_id}/submissions",
        "/api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions",
        "/api/v2/mail-submissions/{submission_id}",
    )
    for path in required_paths:
        require(path in openapi_text, f"OpenAPI route is missing: {path}")
        require(path in combined, f"Skill reference is missing route: {path}")

    client = load_client()
    parser = client.build_parser()
    calls: list[tuple[object, ...]] = []

    def fake_request(method, path, body=None, **kwargs):
        calls.append((method, path, body, kwargs))
        return 200, {"ok": True}

    commands = (
        (
            "subscription-status s active paused "
            "--idempotency-key pause-key-12345678901234567"
        ),
        (
            "claim s --lease-seconds 90 "
            "--idempotency-key claim-key-1234567890123456"
        ),
        (
            "extend s m l --lease-seconds 120 "
            "--idempotency-key extend-key-123456789012345"
        ),
        "ack s m l --idempotency-key ack-key-123456789012345678",
        (
            "nack s m l dependency_timeout "
            "--idempotency-key nack-key-12345678901234567"
        ),
        "dead-letters s --cursor v1.s.m --limit 10",
        "events w --cursor v1.w.e --limit 5",
        "draft mb --to one@example.com --to two@example.com --subject S --text B",
        "submit mb d 3 --idempotency-key submit-key-123456789012345",
        (
            "sandbox-submit mb d 3 "
            "--idempotency-key sandbox-key-12345678901234"
        ),
        "submission sub",
    )
    with patch.object(client, "request", fake_request):
        for command in commands:
            client.execute(parser.parse_args(command.split()))

    require(len(calls) == len(commands), "not all client workflows were composed")
    require(calls[0][0] == "PATCH", "subscription lifecycle route drifted")
    require(
        calls[0][2] == {"expected_status": "active", "status": "paused"},
        "subscription lifecycle body drifted",
    )
    require(calls[1][1].endswith("/claims"), "claim route drifted")
    require(calls[1][2] == {"lease_seconds": 90}, "claim body drifted")
    require(
        calls[5][1].endswith("cursor=v1.s.m&limit=10"),
        "dead-letter Cursor encoding drifted",
    )
    require(
        calls[7][2]["to"] == ["one@example.com", "two@example.com"],
        "draft recipients drifted",
    )
    require(calls[8][2]["expected_draft_version"] == 3, "submission body drifted")
    require(
        calls[9][1].endswith("/sandbox-submissions"),
        "sandbox submission route drifted",
    )
    require(
        calls[9][2]["expected_draft_version"] == 3,
        "sandbox submission body drifted",
    )

    previous_url = client.BASE
    client.BASE = "http://mail.example"
    try:
        with patch.dict(os.environ, {"COPILOTMAIL_TOKEN": "secret-token"}):
            try:
                client.validate_transport("/api/v2/agent/mail-policy", True)
            except SystemExit:
                pass
            else:
                raise SystemExit(
                    "agent skill governance failed: cleartext bearer was allowed"
                )
    finally:
        client.BASE = previous_url

    print(f"agent skill governance ok: {len(commands)} client workflows")


if __name__ == "__main__":
    main()
