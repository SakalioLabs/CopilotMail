"""TLS-only SMTP/IMAP connection adapters for the external-client test runner.

No credentials are accepted here. A connection must finish its selected TLS
handshake before the caller is allowed to authenticate. Failure never falls
back to plaintext or to another TLS mode/port. Tests replace the network
constructors, so each protocol branch is independently verifiable offline.
"""

from __future__ import annotations

from contextlib import suppress
from enum import Enum
import imaplib
import ipaddress
import re
import smtplib
import ssl

_IMAP_ERROR = imaplib.IMAP4.error


class MailTransportError(RuntimeError):
    """Sanitized failure generated locally, not text reflected from a server."""


class TlsMode(str, Enum):
    STARTTLS = "starttls"
    IMPLICIT_TLS = "implicit_tls"

    @classmethod
    def parse(cls, value: str) -> "TlsMode":
        try:
            return cls(value)
        except (ValueError, TypeError) as error:
            raise MailTransportError("TLS mode must be starttls or implicit_tls") from error


def validate_endpoint(host: str, port: int, timeout: int, mode: TlsMode) -> None:
    if (not isinstance(host, str) or not host or len(host) > 253
            or host != host.strip() or any(char.isspace() or ord(char) < 32 for char in host)
            or any(char in host for char in "/\\@?#%") or not host.isascii()):
        raise MailTransportError("mail host must be a bare ASCII hostname or IP address")
    try:
        ipaddress.ip_address(host)
    except ValueError:
        labels = host.removesuffix(".").split(".")
        if not all(re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", label) for label in labels):
            raise MailTransportError("mail host must be a bare ASCII hostname or IP address")
    if type(port) is not int or not 1 <= port <= 65535:
        raise MailTransportError("mail port must be between 1 and 65535")
    if type(timeout) is not int or not 1 <= timeout <= 120:
        raise MailTransportError("mail timeout must be between 1 and 120 seconds")
    if not isinstance(mode, TlsMode):
        raise MailTransportError("mail TLS mode must be explicitly validated")


def validate_context(context: ssl.SSLContext) -> None:
    # SMTP_SSL(None) uses a stdlib compatibility context rather than the
    # application's verified context. Never silently accept that default.
    if not isinstance(context, ssl.SSLContext) or context.minimum_version < ssl.TLSVersion.TLSv1_2:
        raise MailTransportError("an explicit TLS 1.2-or-newer context is required")


def connect_smtp(
    host: str, port: int, timeout: int, context: ssl.SSLContext, mode: TlsMode,
) -> smtplib.SMTP:
    validate_endpoint(host, port, timeout, mode)
    validate_context(context)
    if mode is TlsMode.IMPLICIT_TLS:
        client = smtplib.SMTP_SSL(host, port, timeout=timeout, context=context)
    else:
        client = smtplib.SMTP(host, port, timeout=timeout)
    try:
        code, _ = client.ehlo()
        if code != 250:
            raise MailTransportError("SMTP listener rejected EHLO")
        if mode is TlsMode.STARTTLS:
            if not client.has_extn("starttls"):
                raise MailTransportError("SMTP listener does not advertise STARTTLS")
            code, _ = client.starttls(context=context)
            if code != 220:
                raise MailTransportError("SMTP STARTTLS did not succeed")
            code, _ = client.ehlo()
            if code != 250:
                raise MailTransportError("SMTP listener rejected EHLO after TLS")
        return client
    except BaseException:
        # Do not issue further protocol commands after a failed handshake.
        with suppress(Exception):
            client.close()
        raise


def connect_imap(
    host: str, port: int, timeout: int, context: ssl.SSLContext, mode: TlsMode,
) -> imaplib.IMAP4:
    validate_endpoint(host, port, timeout, mode)
    validate_context(context)
    if mode is TlsMode.IMPLICIT_TLS:
        client = imaplib.IMAP4_SSL(host, port, ssl_context=context, timeout=timeout)
    else:
        client = imaplib.IMAP4(host, port, timeout=timeout)
    try:
        if mode is TlsMode.STARTTLS:
            capabilities = {item.upper() for item in client.capabilities}
            if b"STARTTLS" not in capabilities:
                raise MailTransportError("IMAP listener does not advertise STARTTLS")
            status, _ = client.starttls(ssl_context=context)
            if status != "OK":
                raise MailTransportError("IMAP STARTTLS did not succeed")
        return client
    except BaseException:
        with suppress(Exception):
            client.shutdown()
        raise


def close_imap(client: imaplib.IMAP4) -> None:
    try:
        client.logout()
    except (_IMAP_ERROR, OSError):
        with suppress(Exception):
            client.shutdown()
