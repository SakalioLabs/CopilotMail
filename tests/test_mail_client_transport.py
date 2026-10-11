"""Offline contracts; no server, container, mailbox or credentials are required."""

from contextlib import ExitStack, contextmanager, redirect_stderr
import dataclasses
import imaplib
import io
import json
import os
import smtplib
import ssl
import unittest
from unittest.mock import MagicMock, call, patch
import urllib.request

import mail_client_transport as transport
from mail_client_transport import MailTransportError, TlsMode
import stalwart_poc as smoke


def config(**changes):
    value = smoke.SmokeConfig(
        host="mail.example.test", relay_port=2525, submission_port=2587, imap_port=2143,
        jmap_url="https://mail.example.test/.well-known/jmap",
        sender_address="alice@example.test", sender_password="sender-test-secret",
        recipient_address="bob@example.test", recipient_password="recipient-test-secret",
        timeout_seconds=10, poll_seconds=30, allow_insecure_tls=False,
        allow_insecure_http=False, ca_file=None,
    )
    return dataclasses.replace(value, **changes)


def smtp_client():
    client = MagicMock()
    client.__enter__.return_value = client
    client.ehlo.return_value = (250, b"ok")
    client.has_extn.return_value = True
    client.starttls.return_value = (220, b"ready")
    client.send_message.return_value = {}
    return client


def imap_client():
    client = MagicMock()
    client.capabilities = (b"IMAP4rev1", b"STARTTLS")
    client.starttls.return_value = ("OK", [])
    client.login.return_value = ("OK", [])
    client.select.return_value = ("OK", [])
    return client


@contextmanager
def factories():
    with ExitStack() as stack:
        yield {name: stack.enter_context(patch(target, side_effect=AssertionError("unexpected connection")))
               for name, target in {
                   "smtp": "mail_client_transport.smtplib.SMTP",
                   "smtps": "mail_client_transport.smtplib.SMTP_SSL",
                   "imap": "mail_client_transport.imaplib.IMAP4",
                   "imaps": "mail_client_transport.imaplib.IMAP4_SSL",
               }.items()}


def select_factory(factory, client):
    factory.side_effect = None
    factory.return_value = client


class MailClientTransportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.context = ssl.create_default_context()

    def setUp(self):
        self.network = ExitStack()
        for target in ("socket.create_connection", "socket.getaddrinfo", "socket.socket.connect"):
            self.network.enter_context(patch(target, side_effect=AssertionError("offline test attempted network")))
        self.addCleanup(self.network.close)

    def test_modes_are_explicit_and_plaintext_is_not_an_option(self):
        self.assertEqual(TlsMode.parse("starttls"), TlsMode.STARTTLS)
        self.assertEqual(TlsMode.parse("implicit_tls"), TlsMode.IMPLICIT_TLS)
        for value in ("", "none", "plain", "auto", "STARTTLS", " starttls", None, 1):
            with self.subTest(value=value), self.assertRaises(MailTransportError):
                TlsMode.parse(value)

    def test_bad_endpoints_fail_before_any_connection(self):
        with factories() as constructors:
            cases = [
                ("", 587, 10, TlsMode.STARTTLS), ("mail\r\ntest", 587, 10, TlsMode.STARTTLS),
                ("https://mail.test", 587, 10, TlsMode.STARTTLS),
                ("user@mail.test", 587, 10, TlsMode.STARTTLS),
                ("mail.test:587", 587, 10, TlsMode.STARTTLS),
                ("[::1]", 587, 10, TlsMode.STARTTLS),
                ("mail..test", 587, 10, TlsMode.STARTTLS),
                ("mail.test", 0, 10, TlsMode.STARTTLS), ("mail.test", 65536, 10, TlsMode.STARTTLS),
                ("mail.test", True, 10, TlsMode.STARTTLS), ("mail.test", 587, 0, TlsMode.STARTTLS),
                ("mail.test", 587, 121, TlsMode.STARTTLS), ("mail.test", 587, 10, "starttls"),
            ]
            for host, port, timeout, mode in cases:
                with self.subTest(host=host, port=port, mode=mode), self.assertRaises(MailTransportError):
                    transport.connect_smtp(host, port, timeout, self.context, mode)
                with self.assertRaises(MailTransportError):
                    transport.connect_imap(host, port, timeout, self.context, mode)
            for constructor in constructors.values():
                constructor.assert_not_called()

    def test_missing_tls_context_is_rejected_before_a_connection(self):
        weak = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        weak.minimum_version = ssl.TLSVersion.MINIMUM_SUPPORTED
        with factories() as constructors:
            for mode in TlsMode:
                for function in (transport.connect_smtp, transport.connect_imap):
                    for context in (None, weak):
                        with self.assertRaises(MailTransportError):
                            function("mail.test", 465, 10, context, mode)
            for constructor in constructors.values(): constructor.assert_not_called()

    def test_bare_ipv6_and_fqdn_are_validated_without_dns(self):
        for host in ("::1", "127.0.0.1", "mail.example.test.", "localhost"):
            transport.validate_endpoint(host, 993, 10, TlsMode.IMPLICIT_TLS)

    def test_smtp_starttls_finishes_handshake_and_repeats_ehlo(self):
        client = smtp_client()
        with factories() as constructors:
            select_factory(constructors["smtp"], client)
            self.assertIs(transport.connect_smtp("mail.test", 2587, 10, self.context, TlsMode.STARTTLS), client)
            self.assertEqual(client.method_calls, [call.ehlo(), call.has_extn("starttls"),
                                                   call.starttls(context=self.context), call.ehlo()])
            constructors["smtp"].assert_called_once_with("mail.test", 2587, timeout=10)
            constructors["smtps"].assert_not_called()
            client.login.assert_not_called()

    def test_smtps_uses_implicit_tls_even_on_a_custom_port(self):
        client = smtp_client()
        with factories() as constructors:
            select_factory(constructors["smtps"], client)
            self.assertIs(transport.connect_smtp("mail.test", 44465, 10, self.context, TlsMode.IMPLICIT_TLS), client)
            constructors["smtps"].assert_called_once_with("mail.test", 44465, timeout=10, context=self.context)
            constructors["smtp"].assert_not_called()
            client.starttls.assert_not_called()
            client.ehlo.assert_called_once()

    def test_smtp_handshake_failures_close_without_auth_or_fallback(self):
        scenarios = ["ehlo", "missing_starttls", "starttls_rejected", "certificate", "second_ehlo"]
        for scenario in scenarios:
            client = smtp_client()
            if scenario == "ehlo": client.ehlo.return_value = (500, b"no")
            if scenario == "missing_starttls": client.has_extn.return_value = False
            if scenario == "starttls_rejected": client.starttls.return_value = (454, b"no")
            if scenario == "certificate": client.starttls.side_effect = ssl.SSLCertVerificationError("untrusted")
            if scenario == "second_ehlo": client.ehlo.side_effect = [(250, b"ok"), (500, b"no")]
            with self.subTest(scenario=scenario), factories() as constructors:
                select_factory(constructors["smtp"], client)
                with self.assertRaises((MailTransportError, ssl.SSLError)):
                    transport.connect_smtp("mail.test", 587, 10, self.context, TlsMode.STARTTLS)
                client.close.assert_called_once()
                client.login.assert_not_called()
                constructors["smtps"].assert_not_called()

    def test_implicit_certificate_failure_never_tries_plaintext(self):
        for protocol in ("smtp", "imap"):
            with self.subTest(protocol=protocol), factories() as constructors:
                constructors[protocol + "s"].side_effect = ssl.SSLCertVerificationError("bad certificate")
                function = transport.connect_smtp if protocol == "smtp" else transport.connect_imap
                with self.assertRaises(ssl.SSLError):
                    function("mail.test", 465 if protocol == "smtp" else 993,
                             10, self.context, TlsMode.IMPLICIT_TLS)
                constructors[protocol].assert_not_called()

    def test_imap_starttls_and_imaps_do_not_mix_modes(self):
        for mode, key, port in ((TlsMode.STARTTLS, "imap", 143), (TlsMode.IMPLICIT_TLS, "imaps", 993)):
            client = imap_client()
            with self.subTest(mode=mode), factories() as constructors:
                select_factory(constructors[key], client)
                self.assertIs(transport.connect_imap("mail.test", port, 10, self.context, mode), client)
                if mode is TlsMode.STARTTLS:
                    client.starttls.assert_called_once_with(ssl_context=self.context)
                    constructors[key].assert_called_once_with("mail.test", port, timeout=10)
                else:
                    client.starttls.assert_not_called()
                    constructors[key].assert_called_once_with("mail.test", port, ssl_context=self.context, timeout=10)
                client.login.assert_not_called()

    def test_imap_failed_tls_never_authenticates_and_shuts_down(self):
        for failure in ("capability", "response", "certificate"):
            client = imap_client()
            if failure == "capability": client.capabilities = (b"IMAP4rev1",)
            if failure == "response": client.starttls.return_value = ("NO", [])
            if failure == "certificate": client.starttls.side_effect = ssl.SSLCertVerificationError("bad")
            with self.subTest(failure=failure), factories() as constructors:
                select_factory(constructors["imap"], client)
                with self.assertRaises((MailTransportError, ssl.SSLError)):
                    transport.connect_imap("mail.test", 143, 10, self.context, TlsMode.STARTTLS)
                client.login.assert_not_called()
                client.logout.assert_not_called()
                client.shutdown.assert_called_once()
                constructors["imaps"].assert_not_called()

    def test_close_imap_preserves_primary_result_when_logout_fails(self):
        client = imap_client()
        transport.close_imap(client)
        client.logout.assert_called_once()
        client.shutdown.assert_not_called()
        client.logout.side_effect = OSError("closed")
        client.shutdown.side_effect = OSError("closed again")
        transport.close_imap(client)
        client.shutdown.assert_called_once()

    def test_cancellation_also_closes_a_partly_open_connection(self):
        client = smtp_client()
        client.starttls.side_effect = KeyboardInterrupt()
        with factories() as constructors:
            select_factory(constructors["smtp"], client)
            with self.assertRaises(KeyboardInterrupt):
                transport.connect_smtp("mail.test", 587, 10, self.context, TlsMode.STARTTLS)
            client.close.assert_called_once()

    def test_submission_authenticates_only_after_tls_and_never_retries_unknown_send(self):
        value = config(submission_port=2465, submission_tls_mode=TlsMode.IMPLICIT_TLS)
        client = smtp_client()
        with factories() as constructors:
            select_factory(constructors["smtps"], client)
            receipt = smoke.submit_message(value)
            self.assertLess(client.method_calls.index(call.ehlo()), client.method_calls.index(
                call.login(value.sender_address, value.sender_password)))
            self.assertEqual(receipt["sender"], value.sender_address)
            self.assertEqual(client.send_message.call_args.kwargs["from_addr"], value.sender_address)
            self.assertEqual(client.send_message.call_args.kwargs["to_addrs"], [value.recipient_address])
            client.send_message.side_effect = smtplib.SMTPServerDisconnected("response unknown")
            before = constructors["smtps"].call_count
            with self.assertRaises(smtplib.SMTPServerDisconnected):
                smoke.submit_message(value)
            self.assertEqual(constructors["smtps"].call_count, before + 1)

    def test_imaps_readback_is_exact_readonly_and_does_not_set_seen(self):
        value = config(imap_port=2993, imap_tls_mode=TlsMode.IMPLICIT_TLS)
        message, receipt = smoke.build_message(value)
        client = imap_client()
        client.uid.side_effect = [("OK", [b"7"]), ("OK", [(b"header", message.as_bytes())])]
        with factories() as constructors:
            select_factory(constructors["imaps"], client)
            self.assertTrue(smoke.imap_find_message(value, receipt))
            client.starttls.assert_not_called()
            client.select.assert_called_once_with("INBOX", readonly=True)
            self.assertEqual(client.uid.call_args_list[0], call("search", None, "HEADER", "Message-ID", f'"{receipt["message_id"]}"'))
            self.assertEqual(client.uid.call_args_list[1], call("fetch", b"7", "(BODY.PEEK[])"))
            client.logout.assert_called_once()

    def test_environment_preserves_starttls_defaults_and_selects_implicit_ports(self):
        environment = {
            "COPILOTMAIL_STALWART_SMOKE_HOST": "mail.example.test",
            "COPILOTMAIL_STALWART_SMOKE_JMAP_URL": "https://mail.example.test/.well-known/jmap",
            "COPILOTMAIL_STALWART_SMOKE_SENDER_ADDRESS": "alice@example.test",
            "COPILOTMAIL_STALWART_SMOKE_SENDER_PASSWORD": "sender-test-secret",
            "COPILOTMAIL_STALWART_SMOKE_RECIPIENT_ADDRESS": "bob@example.test",
            "COPILOTMAIL_STALWART_SMOKE_RECIPIENT_PASSWORD": "recipient-test-secret",
        }
        with patch.dict(os.environ, environment, clear=True):
            original = smoke.SmokeConfig.from_environment()
            self.assertEqual((original.submission_port, original.imap_port), (2587, 2143))
            os.environ["COPILOTMAIL_STALWART_SMOKE_SUBMISSION_TLS_MODE"] = "implicit_tls"
            os.environ["COPILOTMAIL_STALWART_SMOKE_IMAP_TLS_MODE"] = "implicit_tls"
            implicit = smoke.SmokeConfig.from_environment()
            self.assertEqual((implicit.submission_port, implicit.imap_port), (2465, 2993))
            self.assertEqual(smoke.transport_modes(implicit), {"smtp_submission": "implicit_tls", "imap": "implicit_tls"})
            self.assertNotIn("sender-test-secret", repr(implicit))
            self.assertNotIn("recipient-test-secret", repr(implicit))
            os.environ["COPILOTMAIL_STALWART_SMOKE_IMAP_TLS_MODE"] = "plain"
            with self.assertRaises(MailTransportError): smoke.SmokeConfig.from_environment()

    def test_imap_login_failure_closes_without_reading_or_retrying(self):
        value = config(imap_tls_mode=TlsMode.IMPLICIT_TLS, imap_port=2993)
        _, receipt = smoke.build_message(value)
        client = imap_client()
        client.login.side_effect = imaplib.IMAP4.error("authentication refused")
        client.logout.side_effect = OSError("connection closed")
        expected_error = imaplib.IMAP4.error
        with factories() as constructors:
            select_factory(constructors["imaps"], client)
            with self.assertRaises(expected_error):
                smoke.imap_find_message(value, receipt)
            constructors["imaps"].assert_called_once()
            client.select.assert_not_called()
            client.uid.assert_not_called()
            client.shutdown.assert_called_once()

    def test_tampered_probe_receipt_cannot_inject_an_imap_command(self):
        value = config()
        _, receipt = smoke.build_message(value)
        for field, replacement in (("message_id", '<id>"\r\nFETCH 1 BODY[]'),
                                   ("marker", 'copilotmail-poc-"bad'),
                                   ("message_id", receipt["message_id"].replace("example.test", "other.test"))):
            with factories() as constructors:
                with self.assertRaises(MailTransportError):
                    smoke.imap_find_message(value, {**receipt, field: replacement})
                for constructor in constructors.values(): constructor.assert_not_called()

    def test_tls_context_verifies_hostname_and_enforces_a_modern_minimum(self):
        context = smoke.tls_context(config())
        self.assertTrue(context.check_hostname)
        self.assertEqual(context.verify_mode, ssl.CERT_REQUIRED)
        self.assertGreaterEqual(context.minimum_version, ssl.TLSVersion.TLSv1_2)

    def test_jmap_redirect_is_rejected_before_authorization_is_forwarded(self):
        request = urllib.request.Request("https://mail.test/jmap", headers={"Authorization": "Basic sensitive"})
        with self.assertRaises(MailTransportError):
            smoke.NoJmapRedirect().redirect_request(request, None, 302, "redirect", {}, "https://other.test/")
        response = MagicMock()
        response.__enter__.return_value = response
        response.geturl.return_value = request.full_url
        response.headers = {"Content-Length": "2"}
        response.read.return_value = b"{}"
        with patch("stalwart_poc.urllib.request.build_opener") as build:
            build.return_value.open.return_value = response
            self.assertEqual(smoke.bounded_json_response(request, self.context, 10), {})
            self.assertTrue(any(isinstance(handler, smoke.NoJmapRedirect) for handler in build.call_args.args))
            build.return_value.open.assert_called_once_with(request, timeout=10)

    def test_untrusted_protocol_errors_do_not_leak_credentials(self):
        errors = [smtplib.SMTPAuthenticationError(535, b"echo secret-password"),
                  imaplib.IMAP4.error("echo secret-password"), OSError("secret-password")]
        for error in errors:
            with self.subTest(error_type=type(error).__name__):
                stream = io.StringIO()
                with patch("sys.argv", ["stalwart_poc.py", "preflight"]), patch.object(smoke, "execute", side_effect=error), redirect_stderr(stream):
                    with self.assertRaises(SystemExit): smoke.main()
                result = json.loads(stream.getvalue())
                self.assertEqual(result, {"status": "failed", "error": "mail interoperability dependency failed"})
                self.assertNotIn("secret-password", stream.getvalue())
        self.assertEqual(smoke.safe_failure_message(MailTransportError("bounded")), "bounded")

    def test_jmap_response_bounds_and_invalid_encoding_fail_closed(self):
        request = urllib.request.Request("https://mail.test/jmap")
        cases = [
            ({"Content-Length": "-1"}, b"{}", request.full_url),
            ({"Content-Length": "invalid"}, b"{}", request.full_url),
            ({"Content-Length": str(smoke.MAX_JMAP_BYTES + 1)}, b"{}", request.full_url),
            ({}, b"x" * (smoke.MAX_JMAP_BYTES + 1), request.full_url),
            ({}, b"not-json", request.full_url), ({}, b"\xff", request.full_url),
            ({}, b"[]", request.full_url), ({}, b"{}", "https://other.test/jmap"),
        ]
        for headers, body, final_url in cases:
            response = MagicMock()
            response.__enter__.return_value = response
            response.headers = headers
            response.read.return_value = body
            response.geturl.return_value = final_url
            with patch("stalwart_poc.urllib.request.build_opener") as build:
                build.return_value.open.return_value = response
                with self.assertRaises(MailTransportError):
                    smoke.bounded_json_response(request, self.context, 10)
                if response.read.called:
                    response.read.assert_called_once_with(smoke.MAX_JMAP_BYTES + 1)


if __name__ == "__main__":
    unittest.main()
