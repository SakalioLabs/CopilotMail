#!/usr/bin/env python3
"""External Stalwart SMTP/IMAP/JMAP interoperability and durability smoke test.

This test intentionally depends only on the Python standard library. Secrets
are read from environment variables and are never included in output receipts.
It is not part of the default CI job because it requires a fully bootstrapped
Stalwart instance and ordinary test accounts.
"""

from __future__ import annotations

import argparse
import base64
import dataclasses
import datetime as dt
import email.policy
import imaplib
import json
import os
import pathlib
import re
import smtplib
import ssl
import sys
import time
import urllib.parse
import urllib.request
import uuid
from email.message import EmailMessage
from typing import Any
from mail_client_transport import (
    MailTransportError as SmokeFailure, TlsMode, close_imap,
    connect_imap, connect_smtp, validate_endpoint,
)


JMAP_CORE = "urn:ietf:params:jmap:core"
JMAP_MAIL = "urn:ietf:params:jmap:mail"
MAX_JMAP_BYTES = 1024 * 1024
MAX_IMAP_MESSAGE_BYTES = 2 * 1024 * 1024
RECEIPT_KEYS = {
    "version",
    "message_id",
    "marker",
    "sender",
    "recipient",
    "sent_at",
}
ADDRESS_PATTERN = re.compile(
    r"^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@"
    r"(?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+"
    r"[A-Za-z]{2,63}$"
)


def env_bool(name: str, default: bool = False) -> bool:
    value = os.environ.get(name)
    if value is None:
        return default
    if value.lower() in {"1", "true", "yes"}:
        return True
    if value.lower() in {"0", "false", "no"}:
        return False
    raise SmokeFailure(f"{name} must be true or false")


def env_int(name: str, default: int, minimum: int, maximum: int) -> int:
    raw = os.environ.get(name, str(default))
    try:
        value = int(raw)
    except ValueError as error:
        raise SmokeFailure(f"{name} must be an integer") from error
    if not minimum <= value <= maximum:
        raise SmokeFailure(f"{name} must be between {minimum} and {maximum}")
    return value


def required_env(name: str) -> str:
    value = os.environ.get(name, "")
    if not value or value != value.strip() or any(ord(char) < 32 for char in value):
        raise SmokeFailure(f"{name} is required and must not contain control characters")
    return value


def validated_address(name: str) -> str:
    value = required_env(name)
    if len(value) > 254 or not value.isascii() or not ADDRESS_PATTERN.fullmatch(value):
        raise SmokeFailure(f"{name} must be one canonical ASCII mailbox address")
    return value


@dataclasses.dataclass(frozen=True)
class SmokeConfig:
    host: str
    relay_port: int
    submission_port: int
    imap_port: int
    jmap_url: str
    sender_address: str
    sender_password: str = dataclasses.field(repr=False)
    recipient_address: str
    recipient_password: str = dataclasses.field(repr=False)
    timeout_seconds: int
    poll_seconds: int
    allow_insecure_tls: bool
    allow_insecure_http: bool
    ca_file: str | None
    submission_tls_mode: TlsMode = TlsMode.STARTTLS
    imap_tls_mode: TlsMode = TlsMode.STARTTLS

    @classmethod
    def from_environment(cls) -> "SmokeConfig":
        submission_mode = TlsMode.parse(os.environ.get(
            "COPILOTMAIL_STALWART_SMOKE_SUBMISSION_TLS_MODE", "starttls"))
        imap_mode = TlsMode.parse(os.environ.get(
            "COPILOTMAIL_STALWART_SMOKE_IMAP_TLS_MODE", "starttls"))
        config = cls(
            host=required_env("COPILOTMAIL_STALWART_SMOKE_HOST"),
            relay_port=env_int(
                "COPILOTMAIL_STALWART_SMOKE_RELAY_PORT", 2525, 1, 65535
            ),
            submission_port=env_int(
                "COPILOTMAIL_STALWART_SMOKE_SUBMISSION_PORT",
                2465 if submission_mode is TlsMode.IMPLICIT_TLS else 2587, 1, 65535
            ),
            imap_port=env_int(
                "COPILOTMAIL_STALWART_SMOKE_IMAP_PORT",
                2993 if imap_mode is TlsMode.IMPLICIT_TLS else 2143, 1, 65535
            ),
            jmap_url=required_env("COPILOTMAIL_STALWART_SMOKE_JMAP_URL"),
            sender_address=validated_address(
                "COPILOTMAIL_STALWART_SMOKE_SENDER_ADDRESS"
            ),
            sender_password=required_env(
                "COPILOTMAIL_STALWART_SMOKE_SENDER_PASSWORD"
            ),
            recipient_address=validated_address(
                "COPILOTMAIL_STALWART_SMOKE_RECIPIENT_ADDRESS"
            ),
            recipient_password=required_env(
                "COPILOTMAIL_STALWART_SMOKE_RECIPIENT_PASSWORD"
            ),
            timeout_seconds=env_int(
                "COPILOTMAIL_STALWART_SMOKE_TIMEOUT_SECONDS", 10, 1, 120
            ),
            poll_seconds=env_int(
                "COPILOTMAIL_STALWART_SMOKE_POLL_SECONDS", 30, 1, 600
            ),
            allow_insecure_tls=env_bool(
                "COPILOTMAIL_STALWART_SMOKE_ALLOW_INSECURE_TLS"
            ),
            allow_insecure_http=env_bool(
                "COPILOTMAIL_STALWART_SMOKE_ALLOW_INSECURE_HTTP"
            ),
            ca_file=os.environ.get("COPILOTMAIL_STALWART_SMOKE_CA_FILE") or None,
            submission_tls_mode=submission_mode,
            imap_tls_mode=imap_mode,
        )
        config.validate()
        return config

    def validate(self) -> None:
        validate_endpoint(self.host, self.relay_port, self.timeout_seconds, TlsMode.STARTTLS)
        validate_endpoint(self.host, self.submission_port, self.timeout_seconds, self.submission_tls_mode)
        validate_endpoint(self.host, self.imap_port, self.timeout_seconds, self.imap_tls_mode)
        if len({self.relay_port, self.submission_port, self.imap_port}) != 3:
            raise SmokeFailure("relay, submission, and IMAP ports must be distinct")
        parsed = urllib.parse.urlsplit(self.jmap_url)
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            raise SmokeFailure("JMAP URL must be an absolute HTTP(S) URL")
        if parsed.username or parsed.password or parsed.query or parsed.fragment:
            raise SmokeFailure("JMAP URL must not contain credentials, query, or fragment")
        normalized_http_origin(self.jmap_url)
        if parsed.scheme == "http" and not self.allow_insecure_http:
            raise SmokeFailure("cleartext JMAP requires explicit local-test opt-in")
        if self.sender_address == self.recipient_address:
            raise SmokeFailure("sender and recipient accounts must be distinct")
        if self.allow_insecure_tls and self.ca_file:
            raise SmokeFailure("choose either a trusted CA file or insecure local TLS")


def tls_context(config: SmokeConfig) -> ssl.SSLContext:
    context = ssl.create_default_context(cafile=config.ca_file)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    if config.allow_insecure_tls:
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
    return context


def normalized_http_origin(url: str) -> tuple[str, str, int]:
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme not in {"http", "https"}
        or not parsed.hostname
        or parsed.username
        or parsed.password
    ):
        raise SmokeFailure("JMAP URL has an invalid origin")
    default_port = 443 if parsed.scheme == "https" else 80
    try:
        port = parsed.port or default_port
    except ValueError as error:
        raise SmokeFailure("JMAP URL has an invalid port") from error
    return parsed.scheme, parsed.hostname.casefold(), port


def smtp_connection(
    config: SmokeConfig, port: int, mode: TlsMode = TlsMode.STARTTLS,
) -> smtplib.SMTP:
    return connect_smtp(config.host, port, config.timeout_seconds, tls_context(config), mode)


def imap_connection(config: SmokeConfig) -> imaplib.IMAP4:
    return connect_imap(config.host, config.imap_port, config.timeout_seconds,
                        tls_context(config), config.imap_tls_mode)


def assert_protocol_separation(config: SmokeConfig) -> None:
    with smtp_connection(config, config.relay_port) as relay:
        if relay.has_extn("auth"):
            raise SmokeFailure("public relay listener unexpectedly advertises AUTH")
    with smtp_connection(config, config.submission_port, config.submission_tls_mode) as submission:
        if not submission.has_extn("auth"):
            raise SmokeFailure("submission listener does not advertise AUTH after TLS")


def assert_not_open_relay(config: SmokeConfig) -> None:
    with smtp_connection(config, config.relay_port) as relay:
        mail_code, _ = relay.mail("probe@sender.invalid")
        if mail_code >= 400:
            return
        rcpt_code, _ = relay.rcpt("probe@recipient.invalid")
        if 200 <= rcpt_code < 300:
            raise SmokeFailure(
                "relay accepted an unauthenticated external-to-external recipient"
            )


def assert_imap_tls(config: SmokeConfig) -> None:
    close_imap(imap_connection(config))


def build_message(config: SmokeConfig) -> tuple[EmailMessage, dict[str, str]]:
    marker = f"copilotmail-poc-{uuid.uuid4()}"
    domain = config.sender_address.rsplit("@", 1)[1]
    message_id = f"<{marker}@{domain}>"
    message = EmailMessage(policy=email.policy.SMTP)
    message["From"] = config.sender_address
    message["To"] = config.recipient_address
    message["Subject"] = f"CopilotMail Stalwart PoC {marker[-12:]}"
    message["Message-ID"] = message_id
    message["X-CopilotMail-PoC-ID"] = marker
    message["Auto-Submitted"] = "no"
    message.set_content(f"CopilotMail durable interoperability probe {marker}\n")
    return message, {
        "version": "1",
        "message_id": message_id,
        "marker": marker,
        "sender": config.sender_address,
        "recipient": config.recipient_address,
        "sent_at": dt.datetime.now(dt.timezone.utc).isoformat(),
    }


def submit_message(config: SmokeConfig) -> dict[str, str]:
    message, receipt = build_message(config)
    with smtp_connection(config, config.submission_port, config.submission_tls_mode) as submission:
        submission.login(config.sender_address, config.sender_password)
        refused = submission.send_message(
            message,
            from_addr=config.sender_address,
            to_addrs=[config.recipient_address],
        )
        if refused:
            raise SmokeFailure("submission refused the configured recipient")
    return receipt


def imap_find_message(config: SmokeConfig, receipt: dict[str, str]) -> bool:
    validate_receipt(config, receipt)
    message_id = receipt["message_id"]
    marker = receipt["marker"]
    imap = imap_connection(config)
    try:
        imap.login(config.recipient_address, config.recipient_password)
        status, _ = imap.select("INBOX", readonly=True)
        if status != "OK":
            raise SmokeFailure("recipient INBOX cannot be selected")
        status, data = imap.uid(
            "search", None, "HEADER", "Message-ID", f'"{message_id}"'
        )
        if status != "OK" or not data or not data[0]:
            return False
        uid = data[0].split()[-1]
        status, fetched = imap.uid("fetch", uid, "(BODY.PEEK[])")
        if status != "OK":
            raise SmokeFailure("matched message cannot be fetched")
        raw = b"".join(
            item[1] for item in fetched if isinstance(item, tuple) and item[1]
        )
        if len(raw) > MAX_IMAP_MESSAGE_BYTES:
            raise SmokeFailure("matched message exceeds smoke-test safety bound")
        if marker.encode("ascii") not in raw:
            raise SmokeFailure("IMAP result does not contain the exact probe marker")
        return True
    finally:
        close_imap(imap)


def wait_for_message(config: SmokeConfig, receipt: dict[str, str]) -> None:
    deadline = time.monotonic() + config.poll_seconds
    while True:
        if imap_find_message(config, receipt):
            return
        if time.monotonic() >= deadline:
            raise SmokeFailure("probe message did not appear before the poll deadline")
        time.sleep(min(2, max(0.1, deadline - time.monotonic())))


class NoJmapRedirect(urllib.request.HTTPRedirectHandler):
    """Reject before urllib can forward the ordinary account's Authorization."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise SmokeFailure("JMAP redirects are not permitted")


def bounded_json_response(
    request: urllib.request.Request, context: ssl.SSLContext, timeout: int
) -> dict[str, Any]:
    opener = urllib.request.build_opener(
        urllib.request.HTTPSHandler(context=context), NoJmapRedirect(),
    )
    with opener.open(request, timeout=timeout) as response:
        if normalized_http_origin(request.full_url) != normalized_http_origin(
            response.geturl()
        ):
            raise SmokeFailure("JMAP request redirected across origin or scheme")
        length = response.headers.get("Content-Length")
        if length is not None:
            try:
                declared_length = int(length)
            except ValueError as error:
                raise SmokeFailure("JMAP response has an invalid Content-Length") from error
            if declared_length < 0 or declared_length > MAX_JMAP_BYTES:
                raise SmokeFailure("JMAP response declares an excessive size")
        body = response.read(MAX_JMAP_BYTES + 1)
    if len(body) > MAX_JMAP_BYTES:
        raise SmokeFailure("JMAP response exceeds the safety bound")
    try:
        value = json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise SmokeFailure("JMAP returned invalid JSON") from error
    if not isinstance(value, dict):
        raise SmokeFailure("JMAP response must be an object")
    return value


def basic_authorization(username: str, password: str) -> str:
    encoded = base64.b64encode(f"{username}:{password}".encode()).decode("ascii")
    return f"Basic {encoded}"


def assert_jmap_mail(config: SmokeConfig) -> None:
    headers = {
        "Accept": "application/json",
        "Authorization": basic_authorization(
            config.recipient_address, config.recipient_password
        ),
    }
    context = tls_context(config)
    session = bounded_json_response(
        urllib.request.Request(config.jmap_url, headers=headers),
        context,
        config.timeout_seconds,
    )
    capabilities = session.get("capabilities", {})
    if not isinstance(capabilities, dict) or JMAP_MAIL not in capabilities:
        raise SmokeFailure("ordinary account JMAP session lacks Mail capability")
    accounts = session.get("accounts", {})
    primary = session.get("primaryAccounts", {})
    account_id = primary.get(JMAP_MAIL) if isinstance(primary, dict) else None
    if not account_id or not isinstance(accounts, dict) or account_id not in accounts:
        raise SmokeFailure("ordinary account lacks a primary JMAP Mail account")
    api_url = session.get("apiUrl")
    if not isinstance(api_url, str):
        raise SmokeFailure("JMAP session lacks apiUrl")
    parsed = urllib.parse.urlsplit(api_url)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise SmokeFailure("JMAP apiUrl is not absolute")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise SmokeFailure("JMAP apiUrl contains forbidden URL components")
    if normalized_http_origin(api_url) != normalized_http_origin(config.jmap_url):
        raise SmokeFailure("JMAP apiUrl is not same-origin with discovery")
    if parsed.scheme == "http" and not config.allow_insecure_http:
        raise SmokeFailure("JMAP apiUrl downgraded to cleartext")
    payload = json.dumps(
        {
            "using": [JMAP_CORE, JMAP_MAIL],
            "methodCalls": [
                [
                    "Mailbox/get",
                    {"accountId": account_id, "ids": None, "properties": ["id", "role"]},
                    "m",
                ]
            ],
        }
    ).encode()
    response = bounded_json_response(
        urllib.request.Request(
            api_url,
            data=payload,
            method="POST",
            headers={**headers, "Content-Type": "application/json"},
        ),
        context,
        config.timeout_seconds,
    )
    calls = response.get("methodResponses")
    if (
        not isinstance(calls, list)
        or len(calls) != 1
        or not isinstance(calls[0], list)
        or calls[0][0] != "Mailbox/get"
    ):
        raise SmokeFailure("JMAP Mailbox/get did not succeed")


def write_receipt(path: pathlib.Path, receipt: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    temporary.write_text(json.dumps(receipt, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def validate_receipt(config: SmokeConfig, value: dict[str, str]) -> None:
    if set(value) != RECEIPT_KEYS or value.get("version") != "1":
        raise SmokeFailure("receipt has an unsupported schema")
    if value["sender"] != config.sender_address:
        raise SmokeFailure("receipt sender does not match configured account")
    if value["recipient"] != config.recipient_address:
        raise SmokeFailure("receipt recipient does not match configured account")
    marker = value["marker"]
    try:
        marker_id = uuid.UUID(marker.removeprefix("copilotmail-poc-"))
    except (ValueError, TypeError, AttributeError) as error:
        raise SmokeFailure("receipt contains invalid probe identity") from error
    if (marker != f"copilotmail-poc-{marker_id}" or marker_id.version != 4
            or value["message_id"] != f"<{marker}@{config.sender_address.rsplit('@', 1)[1]}>"):
        raise SmokeFailure("receipt contains invalid probe identity")
    try:
        sent_at = dt.datetime.fromisoformat(value["sent_at"])
    except ValueError as error:
        raise SmokeFailure("receipt sent_at is invalid") from error
    if sent_at.tzinfo is None:
        raise SmokeFailure("receipt sent_at must include a timezone")


def read_receipt(path: pathlib.Path, config: SmokeConfig) -> dict[str, str]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SmokeFailure("receipt cannot be read") from error
    if not isinstance(value, dict) or any(not isinstance(v, str) for v in value.values()):
        raise SmokeFailure("receipt must be a string-valued JSON object")
    validate_receipt(config, value)
    return value


def self_test() -> None:
    marker = "copilotmail-poc-40000000-0000-4000-8000-000000000001"
    message_id = f"<{marker}@example.test>"
    receipt = {
        "version": "1",
        "message_id": message_id,
        "marker": marker,
        "sender": "alice@example.test",
        "recipient": "bob@example.test",
        "sent_at": "2026-07-27T00:00:00+00:00",
    }
    if json.loads(json.dumps(receipt)) != receipt:
        raise SmokeFailure("receipt round trip failed")
    valid = SmokeConfig(
        host="mail.example.test",
        relay_port=25,
        submission_port=587,
        imap_port=143,
        jmap_url="https://mail.example.test/.well-known/jmap",
        sender_address="alice@example.test",
        sender_password="secret-a",
        recipient_address="bob@example.test",
        recipient_password="secret-b",
        timeout_seconds=10,
        poll_seconds=30,
        allow_insecure_tls=False,
        allow_insecure_http=False,
        ca_file=None,
    )
    valid.validate()
    validate_receipt(valid, receipt)
    if normalized_http_origin("https://MAIL.example.test/path") != (
        "https",
        "mail.example.test",
        443,
    ):
        raise SmokeFailure("default HTTPS origin normalization failed")
    if normalized_http_origin("http://mail.example.test:8080/path") != (
        "http",
        "mail.example.test",
        8080,
    ):
        raise SmokeFailure("explicit HTTP origin normalization failed")
    invalid = dataclasses.replace(
        valid,
        submission_port=25,
    )
    try:
        invalid.validate()
    except SmokeFailure:
        pass
    else:
        raise SmokeFailure("listener-port collision was accepted")
    try:
        dataclasses.replace(invalid, submission_port=587, jmap_url="http://mail.test").validate()
    except SmokeFailure:
        pass
    else:
        raise SmokeFailure("cleartext JMAP was accepted without opt-in")
    for field, replacement in (
        ("sender", "mallory@example.test"),
        ("version", "2"),
        ("sent_at", "not-a-time"),
    ):
        tampered = {**receipt, field: replacement}
        try:
            validate_receipt(valid, tampered)
        except SmokeFailure:
            continue
        raise SmokeFailure("tampered receipt was accepted")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run external Stalwart SMTP/IMAP/JMAP PoC checks"
    )
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("self-test")
    subcommands.add_parser("preflight")
    send = subcommands.add_parser("send")
    send.add_argument("--receipt", type=pathlib.Path, required=True)
    verify = subcommands.add_parser("verify")
    verify.add_argument("--receipt", type=pathlib.Path, required=True)
    roundtrip = subcommands.add_parser("roundtrip")
    roundtrip.add_argument("--receipt", type=pathlib.Path)
    return parser


def execute(args: argparse.Namespace) -> dict[str, Any]:
    if args.command == "self-test":
        self_test()
        return {"status": "ok", "checks": ["configuration", "receipt"]}
    config = SmokeConfig.from_environment()
    if args.command == "preflight":
        assert_protocol_separation(config)
        assert_not_open_relay(config)
        assert_imap_tls(config)
        assert_jmap_mail(config)
        return {
            "status": "ok",
            "checks": [
                "listener_separation",
                "mail_client_tls",
                "not_open_relay",
                "imap",
                "jmap_mail",
            ],
            "transport_modes": transport_modes(config),
        }
    if args.command == "send":
        receipt = submit_message(config)
        write_receipt(args.receipt, receipt)
        return {
            "status": "sent",
            "message_id": receipt["message_id"],
            "receipt": str(args.receipt),
        }
    if args.command == "verify":
        receipt = read_receipt(args.receipt, config)
        wait_for_message(config, receipt)
        assert_jmap_mail(config)
        return {
            "status": "verified",
            "message_id": receipt["message_id"],
            "durability_boundary": "external_restart_performed_by_operator",
        }
    assert_protocol_separation(config)
    assert_not_open_relay(config)
    assert_imap_tls(config)
    assert_jmap_mail(config)
    receipt = submit_message(config)
    if args.receipt:
        write_receipt(args.receipt, receipt)
    wait_for_message(config, receipt)
    return {
        "status": "ok",
        "message_id": receipt["message_id"],
        "checks": [
            "listener_separation",
            "mail_client_tls",
            "not_open_relay",
            "smtp_submission",
            "imap_delivery",
            "jmap_mail",
        ],
        "transport_modes": transport_modes(config),
    }


def transport_modes(config: SmokeConfig) -> dict[str, str]:
    return {"smtp_submission": config.submission_tls_mode.value,
            "imap": config.imap_tls_mode.value}


def safe_failure_message(error: Exception) -> str:
    # SMTP/IMAP/HTTP errors may echo passwords or message contents chosen by a
    # server. Only our own bounded validation messages may reach output.
    if isinstance(error, SmokeFailure):
        return str(error)
    return "mail interoperability dependency failed"


def main() -> None:
    try:
        result = execute(build_parser().parse_args())
    except (SmokeFailure, OSError, smtplib.SMTPException, imaplib.IMAP4.error) as error:
        print(
            json.dumps(
                {"status": "failed", "error": safe_failure_message(error)},
                ensure_ascii=False,
            ),
            file=sys.stderr,
        )
        raise SystemExit(1) from error
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
