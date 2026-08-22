#!/usr/bin/env python3
"""Render a secret-bearing Stalwart PoC account plan to stdout as NDJSON.

Pipe stdout directly to `stalwart-cli apply --stdin`; do not redirect it to a
repository file. The script never prints passwords to stderr.
"""

from __future__ import annotations

import json
import os
import re
import sys
from collections.abc import Mapping


LOCAL_PART = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,62}[A-Za-z0-9])?$")
DOMAIN = re.compile(
    r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$"
)


def required(name: str) -> str:
    value = os.environ.get(name, "")
    if not value or value != value.strip() or any(ord(char) < 32 for char in value):
        raise ValueError(f"{name} is required and must not contain controls")
    return value


def build_plan(
    domain: str,
    sender: str,
    recipient: str,
    sender_password: str,
    recipient_password: str,
) -> list[dict[str, object]]:
    domain = domain.lower()
    if not DOMAIN.fullmatch(domain):
        raise ValueError("COPILOTMAIL_STALWART_POC_DOMAIN is invalid")
    if not LOCAL_PART.fullmatch(sender) or not LOCAL_PART.fullmatch(recipient):
        raise ValueError("PoC account local parts are invalid")
    if sender.casefold() == recipient.casefold():
        raise ValueError("PoC sender and recipient must be distinct")
    return [
        {
            "@type": "upsert",
            "object": "Domain",
            "matchOn": ["name"],
            "value": {
                "poc-domain": {
                    "name": domain,
                    "description": "CopilotMail isolated interoperability PoC",
                    "isEnabled": True,
                }
            },
        },
        {
            "@type": "upsert",
            "object": "Account",
            "matchOn": ["name", "domainId"],
            "value": {
                "poc-sender": {
                    "@type": "User",
                    "name": sender,
                    "domainId": "#poc-domain",
                    "description": "CopilotMail PoC sender",
                    "credentials": {
                        "0": {"@type": "Password", "secret": sender_password}
                    },
                },
                "poc-recipient": {
                    "@type": "User",
                    "name": recipient,
                    "domainId": "#poc-domain",
                    "description": "CopilotMail PoC recipient",
                    "credentials": {
                        "0": {"@type": "Password", "secret": recipient_password}
                    },
                },
            },
        },
    ]


def render() -> list[dict[str, object]]:
    return build_plan(
        required("COPILOTMAIL_STALWART_POC_DOMAIN"),
        required("COPILOTMAIL_STALWART_POC_SENDER_LOCAL"),
        required("COPILOTMAIL_STALWART_POC_RECIPIENT_LOCAL"),
        required("COPILOTMAIL_STALWART_POC_SENDER_PASSWORD"),
        required("COPILOTMAIL_STALWART_POC_RECIPIENT_PASSWORD"),
    )


def self_test() -> None:
    sender_secret = "sender-secret-that-must-stay-in-credentials"
    recipient_secret = "recipient-secret-that-must-stay-in-credentials"
    operations = build_plan(
        "Mail.Example.Test",
        "poc-sender",
        "poc-recipient",
        sender_secret,
        recipient_secret,
    )
    if len(operations) != 2:
        raise ValueError("self-test expected exactly two operations")
    domain = operations[0]
    accounts = operations[1]
    if domain.get("object") != "Domain" or accounts.get("object") != "Account":
        raise ValueError("self-test found an invalid object sequence")
    domain_values = domain.get("value")
    account_values = accounts.get("value")
    if not isinstance(domain_values, Mapping) or not isinstance(
        account_values, Mapping
    ):
        raise ValueError("self-test found invalid value maps")
    if domain_values["poc-domain"]["name"] != "mail.example.test":
        raise ValueError("self-test domain normalization failed")
    if account_values["poc-sender"]["domainId"] != "#poc-domain":
        raise ValueError("self-test sender domain reference failed")
    if account_values["poc-recipient"]["domainId"] != "#poc-domain":
        raise ValueError("self-test recipient domain reference failed")

    serialized = "\n".join(
        json.dumps(operation, separators=(",", ":"), sort_keys=True)
        for operation in operations
    )
    for secret, account_ref in (
        (sender_secret, "poc-sender"),
        (recipient_secret, "poc-recipient"),
    ):
        if serialized.count(secret) != 1:
            raise ValueError("self-test found duplicated or missing credential material")
        credentials = account_values[account_ref]["credentials"]
        if credentials["0"]["secret"] != secret:
            raise ValueError("self-test found a credential outside its expected field")

    invalid_cases = (
        ("single-label", "sender", "recipient"),
        ("mail.example.test", "sender", "SENDER"),
        ("mail.example.test", ".sender", "recipient"),
    )
    for invalid_domain, invalid_sender, invalid_recipient in invalid_cases:
        try:
            build_plan(
                invalid_domain,
                invalid_sender,
                invalid_recipient,
                sender_secret,
                recipient_secret,
            )
        except ValueError:
            continue
        raise ValueError("self-test accepted an invalid plan input")


def main() -> None:
    if sys.argv[1:] == ["--self-test"]:
        try:
            self_test()
        except ValueError as error:
            print(f"Stalwart PoC plan self-test error: {error}", file=sys.stderr)
            raise SystemExit(1) from error
        print('{"status":"ok","checks":["shape","references","secrets","validation"]}')
        return
    if sys.argv[1:]:
        print("usage: render_stalwart_poc_plan.py [--self-test]", file=sys.stderr)
        raise SystemExit(2)
    try:
        operations = render()
    except ValueError as error:
        print(f"Stalwart PoC plan error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    for operation in operations:
        print(json.dumps(operation, separators=(",", ":"), sort_keys=True))


if __name__ == "__main__":
    main()
