# ADR 0008: Agent mTLS uses native rustls proof and certificate-bound tokens

## Status

Accepted.

## Context

Some production workloads have an enterprise PKI and require a credential that
does not depend on a signed client assertion. Accepting a certificate identity
from `X-Forwarded-Client-Cert` or a similar HTTP header would move the trust
boundary into proxy configuration and would be unsafe if the application were
reachable directly or the header were not stripped correctly.

Issuing an ordinary bearer token after mTLS authentication would also discard
proof of possession: a stolen token could be replayed without the certificate.

## Decision

CopilotMail optionally terminates HTTPS itself with rustls. It loads a server
certificate and key plus a dedicated Agent client CA. Client certificates are
optional at the handshake so Human OIDC clients can use the same listener, but
every presented certificate must validate to the configured client CA.

The listener computes the lowercase SHA-256 fingerprint of the verified leaf
certificate DER and places it in an in-process connection extension. No HTTP
header can populate this proof.

An Agent mTLS credential stores only that fingerprint, permitted scopes,
lifetime, Principal relationship, usage metadata, and revocation state.
OAuth2 Client Credentials requests omit `client_assertion`; the TLS fingerprint
must resolve to an active credential for the supplied Agent `client_id`.

Access tokens issued through mTLS contain RFC 8705 `cnf.x5t#S256`, encoded as
base64url without padding. Every protected request must present the same
TLS-verified leaf certificate. The normal live Principal, credential, expiry,
revocation, Scope, and RBAC checks still apply.

Certificate rotation uses overlapping credentials: register the new
certificate, switch the workload, verify, then revoke the old Credential ID.

## Consequences

Certificate-bound tokens are not useful to an attacker who lacks the matching
private key, and a spoofed proxy header cannot establish Agent identity.
Deployments using Agent mTLS must expose the native TLS listener end to end or
use a future cryptographically authenticated proxy-to-application proof
protocol; ordinary TLS termination followed by HTTP is insufficient.

The initial implementation requires a process restart for server certificate
or client CA changes and does not yet implement OCSP fetching, dynamic CRL
reload, SPIFFE/SPIRE, or cloud workload identity federation.
