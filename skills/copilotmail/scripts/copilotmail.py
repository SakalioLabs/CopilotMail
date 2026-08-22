#!/usr/bin/env python3
"""Minimal dependency-free CopilotMail V2 Agent client."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys
import urllib.error
import urllib.parse
import urllib.request


BASE = os.environ.get("COPILOTMAIL_URL", "http://127.0.0.1:8080").rstrip("/")
TIMEOUT_SECONDS = 30


def bearer_token() -> str:
    token_file = os.environ.get("COPILOTMAIL_TOKEN_FILE")
    token = (
        pathlib.Path(token_file).read_text(encoding="utf-8").strip()
        if token_file
        else os.environ.get("COPILOTMAIL_TOKEN", "").strip()
    )
    if not token or any(character.isspace() for character in token):
        raise SystemExit(
            "set COPILOTMAIL_TOKEN_FILE or an ephemeral COPILOTMAIL_TOKEN"
        )
    return token


def validate_transport(path: str, authenticated: bool) -> None:
    parsed = urllib.parse.urlsplit(BASE)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise SystemExit("COPILOTMAIL_URL must be an absolute HTTP(S) URL")
    loopback = (parsed.hostname or "").lower() in {"127.0.0.1", "::1", "localhost"}
    if authenticated and parsed.scheme != "https" and not loopback:
        raise SystemExit("refusing to send a bearer token over non-loopback HTTP")
    if not path.startswith("/") or path.startswith("//"):
        raise SystemExit("invalid API path")


def stable_error(error: urllib.error.HTTPError) -> dict[str, object]:
    payload = error.read(64 * 1024)
    try:
        decoded = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError):
        decoded = {}
    if not isinstance(decoded, dict):
        decoded = {}
    result: dict[str, object] = {"status": error.code}
    for key in ("code", "message", "request_id", "retryable", "details"):
        value = decoded.get(key)
        if value is not None:
            result[key] = value
    retry_after = error.headers.get("Retry-After")
    if retry_after:
        result["retry_after"] = retry_after
    return result


def request(
    method: str,
    path: str,
    body: dict[str, object] | None = None,
    *,
    authenticated: bool = True,
    idempotency_key: str | None = None,
) -> tuple[int, object | None]:
    validate_transport(path, authenticated)
    headers = {"Accept": "application/json"}
    if authenticated:
        headers["Authorization"] = f"Bearer {bearer_token()}"
    if idempotency_key:
        headers["Idempotency-Key"] = idempotency_key
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode(
            "utf-8"
        )
    req = urllib.request.Request(
        BASE + path, data=data, headers=headers, method=method
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT_SECONDS) as response:
            payload = response.read(2 * 1024 * 1024)
            if not payload:
                return response.status, None
            return response.status, json.loads(payload)
    except urllib.error.HTTPError as error:
        print(json.dumps(stable_error(error), ensure_ascii=False), file=sys.stderr)
        raise SystemExit(error.code)
    except urllib.error.URLError as error:
        print(
            json.dumps(
                {"status": 0, "code": "transport_error", "message": str(error.reason)},
                ensure_ascii=False,
            ),
            file=sys.stderr,
        )
        raise SystemExit(2)


def query_path(path: str, **values: object | None) -> str:
    query = urllib.parse.urlencode(
        {key: value for key, value in values.items() if value is not None}
    )
    return f"{path}?{query}" if query else path


def processing_path(subscription_id: str, suffix: str) -> str:
    return f"/api/v2/agent/processing-subscriptions/{subscription_id}/{suffix}"


def add_key(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--idempotency-key",
        required=True,
        help="stable 22-200 character key; reuse only for an exact retry",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="CopilotMail V2 Agent API client")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("health")
    sub.add_parser("policy")

    ensure = sub.add_parser("ensure-subscription")
    ensure.add_argument("mailbox_id")

    lifecycle = sub.add_parser("subscription-status")
    lifecycle.add_argument("subscription_id")
    lifecycle.add_argument("expected_status", choices=("active", "paused"))
    lifecycle.add_argument("status", choices=("active", "paused", "retired"))
    add_key(lifecycle)

    claim = sub.add_parser("claim")
    claim.add_argument("subscription_id")
    claim.add_argument("--lease-seconds", type=int, default=60)
    add_key(claim)

    content = sub.add_parser("content")
    content.add_argument("mailbox_id")
    content.add_argument("message_id")

    extend = sub.add_parser("extend")
    extend.add_argument("subscription_id")
    extend.add_argument("message_id")
    extend.add_argument("lease_id")
    extend.add_argument("--lease-seconds", type=int, default=60)
    add_key(extend)

    ack = sub.add_parser("ack")
    ack.add_argument("subscription_id")
    ack.add_argument("message_id")
    ack.add_argument("lease_id")
    add_key(ack)

    nack = sub.add_parser("nack")
    nack.add_argument("subscription_id")
    nack.add_argument("message_id")
    nack.add_argument("lease_id")
    nack.add_argument("reason_code")
    add_key(nack)

    dead = sub.add_parser("dead-letters")
    dead.add_argument("subscription_id")
    dead.add_argument("--cursor")
    dead.add_argument("--limit", type=int, default=50)

    events = sub.add_parser("events")
    events.add_argument("subscription_id")
    events.add_argument("--cursor")
    events.add_argument("--limit", type=int, default=50)

    draft = sub.add_parser("draft")
    draft.add_argument("mailbox_id")
    draft.add_argument("--to", action="append", required=True)
    draft.add_argument("--cc", action="append", default=[])
    draft.add_argument("--bcc", action="append", default=[])
    draft.add_argument("--subject", required=True)
    draft.add_argument("--text", required=True)
    draft.add_argument("--draft-id")
    draft.add_argument("--expected-version", type=int)
    draft.add_argument("--reply-to-message-id")

    submit = sub.add_parser("submit")
    submit.add_argument("mailbox_id")
    submit.add_argument("draft_id")
    submit.add_argument("expected_version", type=int)
    submit.add_argument("--approval-request-id")
    add_key(submit)

    sandbox = sub.add_parser("sandbox-submit")
    sandbox.add_argument("mailbox_id")
    sandbox.add_argument("draft_id")
    sandbox.add_argument("expected_version", type=int)
    add_key(sandbox)

    status = sub.add_parser("submission")
    status.add_argument("submission_id")
    return parser


def execute(args: argparse.Namespace) -> tuple[int, object | None]:
    if args.command == "health":
        return request("GET", "/healthz", authenticated=False)
    if args.command == "policy":
        return request("GET", "/api/v2/agent/mail-policy")
    if args.command == "ensure-subscription":
        return request(
            "PUT",
            f"/api/v2/agent/mailboxes/{args.mailbox_id}/processing-subscription",
        )
    if args.command == "subscription-status":
        return request(
            "PATCH",
            f"/api/v2/agent/processing-subscriptions/{args.subscription_id}",
            {"expected_status": args.expected_status, "status": args.status},
            idempotency_key=args.idempotency_key,
        )
    if args.command == "claim":
        return request(
            "POST",
            processing_path(args.subscription_id, "claims"),
            {"lease_seconds": args.lease_seconds},
            idempotency_key=args.idempotency_key,
        )
    if args.command == "content":
        return request(
            "GET",
            f"/api/v2/agent/mailboxes/{args.mailbox_id}"
            f"/messages/{args.message_id}/content",
        )
    if args.command == "extend":
        return request(
            "POST",
            processing_path(
                args.subscription_id,
                f"messages/{args.message_id}/lease-extension",
            ),
            {"lease_id": args.lease_id, "lease_seconds": args.lease_seconds},
            idempotency_key=args.idempotency_key,
        )
    if args.command == "ack":
        return request(
            "POST",
            processing_path(
                args.subscription_id,
                f"messages/{args.message_id}/acknowledgement",
            ),
            {"lease_id": args.lease_id},
            idempotency_key=args.idempotency_key,
        )
    if args.command == "nack":
        return request(
            "POST",
            processing_path(
                args.subscription_id, f"messages/{args.message_id}/nack"
            ),
            {"lease_id": args.lease_id, "reason_code": args.reason_code},
            idempotency_key=args.idempotency_key,
        )
    if args.command == "dead-letters":
        return request(
            "GET",
            query_path(
                processing_path(args.subscription_id, "dead-letters"),
                cursor=args.cursor,
                limit=args.limit,
            ),
        )
    if args.command == "events":
        return request(
            "GET",
            query_path(
                f"/api/v2/webhook-subscriptions/{args.subscription_id}/events",
                cursor=args.cursor,
                limit=args.limit,
            ),
        )
    if args.command == "draft":
        if (args.draft_id is None) != (args.expected_version is None):
            raise SystemExit(
                "--draft-id and --expected-version must be supplied together"
            )
        return request(
            "POST",
            f"/api/v2/mailboxes/{args.mailbox_id}/drafts",
            {
                "draft_id": args.draft_id,
                "expected_version": args.expected_version,
                "reply_to_message_id": args.reply_to_message_id,
                "to": args.to,
                "cc": args.cc,
                "bcc": args.bcc,
                "subject": args.subject,
                "text_body": args.text,
            },
        )
    if args.command == "submit":
        return request(
            "POST",
            f"/api/v2/mailboxes/{args.mailbox_id}/submissions",
            {
                "draft_id": args.draft_id,
                "expected_draft_version": args.expected_version,
                "first_contact_approval_request_id": args.approval_request_id,
            },
            idempotency_key=args.idempotency_key,
        )
    if args.command == "sandbox-submit":
        return request(
            "POST",
            f"/api/v2/agent/mailboxes/{args.mailbox_id}/sandbox-submissions",
            {
                "draft_id": args.draft_id,
                "expected_draft_version": args.expected_version,
            },
            idempotency_key=args.idempotency_key,
        )
    return request(
        "GET", f"/api/v2/mail-submissions/{args.submission_id}"
    )


def main() -> None:
    status, payload = execute(build_parser().parse_args())
    if payload is None:
        print(json.dumps({"status": status}))
    else:
        print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
