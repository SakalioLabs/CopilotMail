import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ApiError,
  buildQuery,
  ControlPlaneClient,
  InvalidApiResponseError,
} from "./api";

const ORGANIZATION_ID = "019b0000-0000-7000-8000-000000000001";
const OTHER_ORGANIZATION_ID = "019b0000-0000-7000-8000-000000000002";
const AUDIT_QUERY_ID = "019b0000-0000-7000-8000-000000000003";
const AUDIT_EVENT_ID = "019b0000-0000-7000-8000-000000000004";
const AUDIT_ACTOR_ID = "019b0000-0000-7000-8000-000000000005";
const ARCHIVE_ID = "019b0000-0000-7000-8000-000000000006";
const APPROVAL_ID = "019b0000-0000-7000-8000-000000000007";
const GRANT_ID = "019b0000-0000-7000-8000-000000000008";
const REQUESTED_BY_ID = "019b0000-0000-7000-8000-000000000009";
const APPROVAL_POLICY_VERSION_ID = "019b0000-0000-7000-8000-00000000000a";
const SECOND_APPROVAL_ID = "019b0000-0000-7000-8000-00000000000b";
const APPROVAL_STEP_ID = "019b0000-0000-7000-8000-00000000000c";
const APPROVER_ID = "019b0000-0000-7000-8000-00000000000d";
const ARTIFACT_BODY = "{\"sequence\":\"1\"}\n";
const ARTIFACT_SHA256 = "8dadc4e06f72d219341660d4abb575833a878434620dde26f59c43ef23fcf72a";
const MAIL_DOMAIN_ID = "019b0000-0000-7000-8000-00000000000e";
const SECOND_MAIL_DOMAIN_ID = "019b0000-0000-7000-8000-00000000000f";
const MAIL_DOMAIN_NAME = "example.test";
const OWNERSHIP_TOKEN = `cmv1_${"A".repeat(43)}`;
const ROTATED_OWNERSHIP_TOKEN = `cmv1_${"B".repeat(43)}`;
const DNS_VALID_UNTIL = "2026-08-25T02:00:00+00:00";
const DNS_OBSERVED_AT = "2026-08-25T01:00:00+00:00";
const AGENT_MAIL_EVIDENCE_AGENT_ID = "019b0000-0000-7000-8000-000000000111";
const AGENT_MAIL_EVIDENCE_SUBMISSION_ID = "019b0000-0000-7000-8000-000000000112";
const AGENT_MAIL_EVIDENCE_ADMISSION_ID = "019b0000-0000-7000-8000-000000000113";
const AGENT_MAIL_EVIDENCE_ACTIVATION_ID = "019b0000-0000-7000-8000-000000000114";
const AGENT_MAIL_EVIDENCE_VERSION_ID = "019b0000-0000-7000-8000-000000000115";
const AGENT_MAIL_EVIDENCE_POLICY_APPROVAL_ID = "019b0000-0000-7000-8000-000000000116";
const AGENT_MAIL_EVIDENCE_FIRST_CONTACT_APPROVAL_ID = "019b0000-0000-7000-8000-000000000117";
const AGENT_MAIL_EVIDENCE_REQUESTED_EVENT_ID = "019b0000-0000-7000-8000-000000000118";
const AGENT_MAIL_EVIDENCE_ADMITTED_EVENT_ID = "019b0000-0000-7000-8000-000000000119";
const AGENT_MAIL_EVIDENCE_TERMINAL_EVENT_ID = "019b0000-0000-7000-8000-00000000011a";

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function mailDomainResponse(overrides: Record<string, unknown> = {}) {
  return {
    mail_domain_id: MAIL_DOMAIN_ID,
    organization_id: ORGANIZATION_ID,
    name: MAIL_DOMAIN_NAME,
    status: "pending_dns",
    route_mode: "direct_mx",
    ownership_challenge_version: 1,
    route_version: 1,
    inbound_enabled: false,
    outbound_enabled: false,
    smart_host_ref: null,
    dkim_key_ref: null,
    mail_engine_domain_ref: null,
    mail_engine_projection_status: "not_projected",
    mail_engine_projected_enabled: null,
    mail_engine_projection_last_attempt_at: null,
    mail_engine_projection_succeeded_at: null,
    mail_engine_projection_error_code: null,
    latest_dns_observed_at: null,
    latest_dns_valid_until: null,
    latest_dns_activation_ready: false,
    latest_dns_failure_codes: [],
    ...overrides,
  };
}

function mailDomainCreateResponse(overrides: Record<string, unknown> = {}) {
  return {
    mail_domain_id: MAIL_DOMAIN_ID,
    organization_id: ORGANIZATION_ID,
    name: MAIL_DOMAIN_NAME,
    status: "pending_dns",
    ownership_challenge_version: 1,
    route_version: 1,
    ownership_record_name: `_copilotmail-verification.${MAIL_DOMAIN_NAME}`,
    ownership_token: OWNERSHIP_TOKEN,
    ...overrides,
  };
}

function mailDomainChallengeResponse(overrides: Record<string, unknown> = {}) {
  return {
    mail_domain_id: MAIL_DOMAIN_ID,
    status: "pending_dns",
    ownership_challenge_version: 2,
    ownership_record_name: `_copilotmail-verification.${MAIL_DOMAIN_NAME}`,
    ownership_token: ROTATED_OWNERSHIP_TOKEN,
    ...overrides,
  };
}

function mailDomainDnsVerificationResponse(overrides: Record<string, unknown> = {}) {
  return {
    mail_domain_id: MAIL_DOMAIN_ID,
    status: "verified",
    activation_ready: true,
    valid_until: DNS_VALID_UNTIL,
    failure_codes: [],
    ...overrides,
  };
}

function auditEventResponse(overrides: Record<string, unknown> = {}) {
  return {
    event_id: AUDIT_EVENT_ID,
    organization_id: ORGANIZATION_ID,
    actor_principal_id: AUDIT_ACTOR_ID,
    actor_kind: "human",
    on_behalf_of_principal_id: null,
    credential_id: null,
    subject_principal_id: null,
    action: "compliance.case.queried",
    resource_type: "compliance.case_query",
    resource_id: "case-1",
    outcome: "success",
    trace_id: "trace-1",
    request_id: "source-request",
    source_ip: null,
    client_id: null,
    session_id: null,
    token_jti: null,
    policy_id: null,
    policy_version_id: null,
    approval_request_id: null,
    ticket_reference: null,
    environment: null,
    region: null,
    details: {},
    occurred_at: "2026-07-25T12:00:00+00:00",
    received_at: "2026-07-25T12:00:01+00:00",
    chain_sequence: "42",
    hash_version: 1,
    previous_hash: null,
    event_hash: "a".repeat(64),
    signature_batch_id: null,
    ...overrides,
  };
}

function auditPageResponse(
  items: unknown[],
  overrides: Record<string, unknown> = {},
) {
  const last = items.at(-1) as { chain_sequence?: string } | undefined;
  return {
    query_id: AUDIT_QUERY_ID,
    occurred_from: "2026-07-25T00:00:00+00:00",
    occurred_to: "2026-07-26T00:00:00+00:00",
    items,
    next_cursor: last ? `v1.${ORGANIZATION_ID}.${last.chain_sequence}` : null,
    has_more: false,
    ...overrides,
  };
}

function auditGrantResponse(overrides: Record<string, unknown> = {}) {
  return {
    audit_export_grant_id: GRANT_ID,
    organization_id: ORGANIZATION_ID,
    archive_id: ARCHIVE_ID,
    approval_request_id: APPROVAL_ID,
    requested_by: REQUESTED_BY_ID,
    created_at: "2026-07-26T00:00:00+00:00",
    expires_at: "2026-07-26T00:15:00+00:00",
    ...overrides,
  };
}

function agentInspectorResponse(overrides: Record<string, unknown> = {}) {
  return {
    agent_id: "019b0000-0000-7000-8000-000000000101",
    display_name: "Inbound triage",
    principal_status: "active",
    owner: {
      principal_id: "019b0000-0000-7000-8000-000000000102",
      display_name: "Owner",
      principal_status: "active",
    },
    profile: {
      environment: "production",
      risk_level: "medium",
      purpose: "Triage inbound support mail",
      review_due_at: "2026-09-01T00:00:00Z",
      review_state: "scheduled",
    },
    token_access: {
      state: "effective",
      effective_scopes: ["organization.read"],
      active_credential_count: 1,
      next_credential_expiry_at: "2026-09-15T00:00:00Z",
      last_authenticated_at: "2026-08-23T09:00:00Z",
    },
    control_plane: {
      mail_admission_state: "active",
      active_processing_subscription_count: 1,
      paused_processing_subscription_count: 0,
      retired_processing_subscription_count: 0,
      last_processing_activity_at: "2026-08-23T09:00:00Z",
    },
    observed_at: "2026-08-23T09:01:00Z",
    ...overrides,
  };
}

/**
 * Exact public DTO for one Agent production-mail admission. It intentionally
 * has no recipient, content, mailbox, runtime, execution, provider, SMTP, or
 * internal delivery-snapshot identity fields.
 */
function agentMailSubmissionEvidenceResponse(overrides: Record<string, unknown> = {}) {
  return {
    agent_id: AGENT_MAIL_EVIDENCE_AGENT_ID,
    submission: {
      submission_id: AGENT_MAIL_EVIDENCE_SUBMISSION_ID,
      status: "accepted",
      created_at: "2026-08-24T09:00:00Z",
      updated_at: "2026-08-24T09:01:00Z",
    },
    admission: {
      admission_id: AGENT_MAIL_EVIDENCE_ADMISSION_ID,
      admitted_at: "2026-08-24T09:00:01Z",
      policy: {
        activation_id: AGENT_MAIL_EVIDENCE_ACTIVATION_ID,
        activated_at: "2026-08-24T08:00:00Z",
        version_id: AGENT_MAIL_EVIDENCE_VERSION_ID,
        version: 2,
        approval_request_id: AGENT_MAIL_EVIDENCE_POLICY_APPROVAL_ID,
      },
      recipient_count: 2,
      first_contact_count: 1,
      first_contact_approval_request_id: AGENT_MAIL_EVIDENCE_FIRST_CONTACT_APPROVAL_ID,
    },
    delivery: {
      observed_at: "2026-08-24T09:02:00Z",
      terminal: false,
      recipient_count: 2,
      delivered_count: 1,
      failed_count: 0,
      queued_count: 1,
      unknown_count: 0,
    },
    audit_references: [
      {
        event_id: AGENT_MAIL_EVIDENCE_REQUESTED_EVENT_ID,
        action: "mail.submission.requested",
        occurred_at: "2026-08-24T09:00:00Z",
      },
      {
        event_id: AGENT_MAIL_EVIDENCE_ADMITTED_EVENT_ID,
        action: "agent.mail.admitted",
        occurred_at: "2026-08-24T09:00:01Z",
      },
      {
        event_id: AGENT_MAIL_EVIDENCE_TERMINAL_EVENT_ID,
        action: "mail.submission.accepted",
        occurred_at: "2026-08-24T09:01:00Z",
      },
    ],
    observed_at: "2026-08-24T09:03:00Z",
    ...overrides,
  };
}

function approvalRequestResponse(overrides: Record<string, unknown> = {}) {
  return {
    approval_request_id: APPROVAL_ID,
    organization_id: ORGANIZATION_ID,
    policy_version_id: APPROVAL_POLICY_VERSION_ID,
    action: "audit.export",
    resource_kind: "audit_archive",
    resource_id: ARCHIVE_ID,
    requested_by: REQUESTED_BY_ID,
    reason: "Export evidence for case AC-42",
    ticket_reference: "AC-42",
    required_approvals: 2,
    approval_count: 0,
    rejection_count: 0,
    status: "pending",
    created_at: "2026-08-24T01:00:00+00:00",
    expires_at: "2026-08-25T01:00:00+00:00",
    completed_at: null,
    cancelled_at: null,
    ...overrides,
  };
}

function binaryArtifactResponse(
  overrides: Record<string, string> = {},
  body = ARTIFACT_BODY,
) {
  return new Response(body, {
    status: 200,
    headers: {
      "Content-Type": "application/octet-stream",
      "Content-Length": String(new TextEncoder().encode(body).byteLength),
      "Content-Disposition": `attachment; filename=\"${GRANT_ID}-events.jsonl\"`,
      "Cache-Control": "no-store",
      "x-copilotmail-artifact-sha256": ARTIFACT_SHA256,
      ...overrides,
    },
  });
}

function caseResponse(overrides: Record<string, unknown> = {}) {
  return {
    case_id: "case-1",
    organization_id: "org-1",
    external_reference: "SEC-1",
    title: "Evidence review",
    purpose: "Preserve evidence",
    status: "open",
    created_by: "human-1",
    created_at: "2026-08-24T01:00:00Z",
    closed_by: null,
    closed_at: null,
    ...overrides,
  };
}

function holdResponse(overrides: Record<string, unknown> = {}) {
  return {
    legal_hold_id: "hold-1",
    organization_id: "org-1",
    case_id: "case-1",
    archive_id: "archive-1",
    reason: "Preserve evidence",
    status: "active",
    apply_approval_request_id: "approval-apply-1",
    created_by: "human-1",
    created_at: "2026-08-24T01:00:00Z",
    release_approval_request_id: null,
    released_by: null,
    released_at: null,
    ...overrides,
  };
}

describe("buildQuery", () => {
  it("omits absent values and encodes filter values", () => {
    expect(buildQuery({ status: "active", case_id: "a/b", cursor: undefined, limit: 25 }))
      .toBe("?status=active&case_id=a%2Fb&limit=25");
  });

  it("does not append a question mark for an empty query", () => {
    expect(buildQuery({ status: "", cursor: null })).toBe("");
  });
});

describe("ControlPlaneClient", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("binds tenant and request ID while using only the same-origin session cookie", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      items: [],
      next_cursor: null,
      has_more: false,
    }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("crypto", { ...crypto, randomUUID: () => "request-1" });
    const client = new ControlPlaneClient("", "org-1");

    await client.listCases({ status: "open", limit: 25 });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v2/organizations/org-1/compliance-cases?status=open&limit=25",
      expect.objectContaining({
        credentials: "same-origin",
        headers: expect.objectContaining({
          "x-request-id": "request-1",
        }),
      }),
    );
    expect((fetchMock.mock.calls[0][1] as RequestInit).headers)
      .not.toHaveProperty("Authorization");
  });

  it("preserves the server error code and request identifier", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({
      code: "compliance_cursor_expired_or_invalid",
      message: "cursor expired",
      request_id: "request-2",
      trace_id: "trace-2",
      retryable: false,
    }), { status: 410, headers: { "Content-Type": "application/json" } })));
    const client = new ControlPlaneClient("", "org-1");

    const error = await client.listHolds({ cursor: "stale" }).catch((cause) => cause);

    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      status: 410,
      body: { code: "compliance_cursor_expired_or_invalid", request_id: "request-2" },
    });
  });

  it("rejects cross-tenant, lifecycle-invalid, and malformed Case/Hold pages", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [caseResponse({ organization_id: "org-2" })],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [holdResponse({ released_at: "2026-08-24T02:00:00Z" })],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [],
        next_cursor: null,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await expect(client.listCases({ limit: 25 })).rejects.toBeInstanceOf(
      InvalidApiResponseError,
    );
    await expect(client.listHolds({ limit: 25 })).rejects.toBeInstanceOf(
      InvalidApiResponseError,
    );
    await expect(client.listCases({ limit: 25 })).rejects.toBeInstanceOf(
      InvalidApiResponseError,
    );
  });

  it("binds Case/Hold reads to the requested identity and active filters", async () => {
    const response = (body: unknown) => new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
    const closedCase = caseResponse({
      status: "closed",
      closed_by: "human-2",
      closed_at: "2026-08-24T02:00:00Z",
    });
    const releasedHold = holdResponse({
      status: "released",
      release_approval_request_id: "approval-release-1",
      released_by: "human-2",
      released_at: "2026-08-24T02:00:00Z",
    });
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(response({
        items: [closedCase], next_cursor: null, has_more: false,
      }))
      .mockResolvedValueOnce(response({
        items: [releasedHold], next_cursor: null, has_more: false,
      }))
      .mockResolvedValueOnce(response({
        items: [holdResponse({ case_id: "case-2" })],
        next_cursor: null,
        has_more: false,
      }))
      .mockResolvedValueOnce(response(caseResponse({ case_id: "case-2" })))
      .mockResolvedValueOnce(response(holdResponse({ legal_hold_id: "hold-2" })));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await expect(client.listCases({ status: "open", limit: 25 }))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.listHolds({ status: "active", limit: 25 }))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.listHolds({ case_id: "case-1", limit: 25 }))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getCase("case-1"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getHold("hold-1"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
  });

  it("binds terminal write evidence and reports protected 401 responses", async () => {
    const onUnauthorized = vi.fn();
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(holdResponse({
        status: "released",
        release_approval_request_id: "approval-other",
        released_by: "human-2",
        released_at: "2026-08-24T02:00:00Z",
      })), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        code: "unauthenticated",
        message: "session expired",
        request_id: "request-401",
        trace_id: "trace-401",
        retryable: false,
      }), { status: 401, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1", onUnauthorized);

    await expect(client.releaseHold("hold-1", "approval-release-1"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.listCases({ limit: 25 })).rejects.toBeInstanceOf(ApiError);
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });

  it("reuses the exact audit window and cursor with every active filter", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(
      auditPageResponse([], { next_cursor: `v1.${ORGANIZATION_ID}.42` }),
    ), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await client.searchAuditEvents({
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      action: "compliance.case.queried",
      actor_principal_id: AUDIT_ACTOR_ID,
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success",
      request_id: "source-request",
    }, { cursor: `v1.${ORGANIZATION_ID}.42`, limit: 50 });

    const url = new URL(String(fetchMock.mock.calls[0][0]), "https://control.example");
    expect(Object.fromEntries(url.searchParams)).toEqual({
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      action: "compliance.case.queried",
      actor_principal_id: AUDIT_ACTOR_ID,
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success",
      request_id: "source-request",
      cursor: `v1.${ORGANIZATION_ID}.42`,
      limit: "50",
    });
  });

  it("strictly parses and request-binds every audit event page field", async () => {
    const filters = {
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      action: "compliance.case.queried",
      actor_principal_id: AUDIT_ACTOR_ID,
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success" as const,
      request_id: "source-request",
    };
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(
      auditPageResponse([auditEventResponse()]),
    ), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    const page = await client.searchAuditEvents(filters, { limit: 50 });

    expect(page.items).toHaveLength(1);
    expect(page.items[0]).toMatchObject({
      event_id: AUDIT_EVENT_ID,
      organization_id: ORGANIZATION_ID,
      actor_kind: "human",
      outcome: "success",
      chain_sequence: "42",
      event_hash: "a".repeat(64),
    });
    expect(page.next_cursor).toBe(`v1.${ORGANIZATION_ID}.42`);
  });

  it("rejects malformed, cross-tenant, oversized, filter-conflicting, and cursor-invalid audit pages", async () => {
    const filters = {
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      action: "compliance.case.queried",
    };
    const response = (body: unknown) => new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse({ organization_id: OTHER_ORGANIZATION_ID }),
      ])))
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse({ actor_kind: "robot" }),
      ])))
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse({ occurred_at: "2026-02-30T12:00:00Z" }),
      ])))
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse({ event_hash: "ABC".repeat(21) + "A" }),
      ])))
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse({ details: { value: "x".repeat(65 * 1024) } }),
      ])))
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse({ action: "mail.sent" }),
      ])))
      .mockResolvedValueOnce(response(auditPageResponse([
        auditEventResponse(),
      ], { next_cursor: `v1.${OTHER_ORGANIZATION_ID}.42` })))
      .mockResolvedValueOnce(response(auditPageResponse(
        Array.from({ length: 2 }, (_, index) => auditEventResponse({
          event_id: index === 0
            ? AUDIT_EVENT_ID
            : "019b0000-0000-7000-8000-000000000099",
          chain_sequence: String(42 - index),
        })),
      )))
      .mockResolvedValueOnce(response({
        ...auditPageResponse([]),
        unexpected: "field",
      }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    for (let index = 0; index < 7; index += 1) {
      await expect(client.searchAuditEvents(filters, { limit: 50 }))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
    }
    await expect(client.searchAuditEvents(filters, { limit: 1 }))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.searchAuditEvents(filters, { limit: 50 }))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
  });

  it("binds an export authorization to the archive path and approval body", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(
      auditGrantResponse(),
    ), { status: 201, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await client.authorizeAuditExport(ARCHIVE_ID, APPROVAL_ID);

    expect(fetchMock).toHaveBeenCalledWith(
      `/api/v2/audit-archives/${ARCHIVE_ID}/exports`,
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ approval_request_id: APPROVAL_ID }),
      }),
    );
  });

  it("reconciles an unresolved export authorization through its exact read-only relationship", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(auditGrantResponse()));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    const grant = await client.getAuditExportGrantByApproval(ARCHIVE_ID, APPROVAL_ID);

    expect(grant).toMatchObject({
      audit_export_grant_id: GRANT_ID,
      archive_id: ARCHIVE_ID,
      approval_request_id: APPROVAL_ID,
    });
    expect(fetchMock).toHaveBeenCalledWith(
      `/api/v2/audit-archives/${ARCHIVE_ID}/export-grants/${APPROVAL_ID}`,
      expect.objectContaining({
        cache: "no-store",
        credentials: "same-origin",
      }),
    );
    expect((fetchMock.mock.calls[0]?.[1] as RequestInit).method).toBeUndefined();
  });

  it("rejects a read-only export reconciliation that does not bind the requested relationship", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(jsonResponse(auditGrantResponse({
        archive_id: "019b0000-0000-7000-8000-000000000061",
      })))
      .mockResolvedValueOnce(jsonResponse(auditGrantResponse({
        approval_request_id: SECOND_APPROVAL_ID,
      })));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await expect(client.getAuditExportGrantByApproval(ARCHIVE_ID, APPROVAL_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getAuditExportGrantByApproval(ARCHIVE_ID, APPROVAL_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("rejects export grants that do not exactly bind tenant, archive, approval, grant, schema, and lifetime", async () => {
    const response = (body: unknown) => new Response(JSON.stringify(body), {
      status: 201,
      headers: { "Content-Type": "application/json" },
    });
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(response(auditGrantResponse({
        organization_id: OTHER_ORGANIZATION_ID,
      })))
      .mockResolvedValueOnce(response(auditGrantResponse({
        archive_id: "019b0000-0000-7000-8000-000000000061",
      })))
      .mockResolvedValueOnce(response(auditGrantResponse({
        approval_request_id: "019b0000-0000-7000-8000-000000000071",
      })))
      .mockResolvedValueOnce(response(auditGrantResponse({
        expires_at: "2026-07-26T00:31:00+00:00",
      })))
      .mockResolvedValueOnce(response(auditGrantResponse({
        created_at: "2026-07-26 00:00:00Z",
      })))
      .mockResolvedValueOnce(response({ ...auditGrantResponse(), unexpected: true }))
      .mockResolvedValueOnce(new Response(JSON.stringify(auditGrantResponse({
        audit_export_grant_id: "019b0000-0000-7000-8000-000000000081",
      })), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    for (let index = 0; index < 6; index += 1) {
      await expect(client.authorizeAuditExport(ARCHIVE_ID, APPROVAL_ID))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
    }
    await expect(client.getAuditExportGrant(GRANT_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
  });

  it("does not send malformed export relationship identifiers", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await expect(client.authorizeAuditExport("../archive", APPROVAL_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.authorizeAuditExport(ARCHIVE_ID, "approval"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getAuditExportGrant("grant"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getAuditExportGrantByApproval("../archive", APPROVAL_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getAuditExportGrantByApproval(ARCHIVE_ID, "approval"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.getAuditExportGrantByApproval(ARCHIVE_ID.toUpperCase(), APPROVAL_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("reads an explicitly requested Agent snapshot without caching or browser bearer state", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      agent_id: "019b0000-0000-7000-8000-000000000101",
      display_name: "Inbound triage",
      principal_status: "active",
      owner: {
        principal_id: "019b0000-0000-7000-8000-000000000102",
        display_name: "Owner",
        principal_status: "active",
      },
      profile: {
        environment: "production",
        risk_level: "medium",
        purpose: "Triage inbound support mail",
        review_due_at: "2026-09-01T00:00:00Z",
        review_state: "scheduled",
      },
      token_access: {
        state: "effective",
        effective_scopes: ["organization.read"],
        active_credential_count: 1,
        next_credential_expiry_at: "2026-09-15T00:00:00Z",
        last_authenticated_at: null,
      },
      control_plane: {
        mail_admission_state: "active",
        active_processing_subscription_count: 1,
        paused_processing_subscription_count: 0,
        retired_processing_subscription_count: 0,
        last_processing_activity_at: null,
      },
      observed_at: "2026-08-23T09:01:00Z",
    }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await client.getAgentInspector("019b0000-0000-7000-8000-000000000101");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v2/agents/019b0000-0000-7000-8000-000000000101/inspector",
      expect.objectContaining({
        cache: "no-store",
        credentials: "same-origin",
      }),
    );
    expect((fetchMock.mock.calls[0][1] as RequestInit).headers)
      .not.toHaveProperty("Authorization");
  });

  it("fails closed on malformed or contradictory Agent Inspector snapshots", async () => {
    const agentId = "019b0000-0000-7000-8000-000000000101";
    const profileWithoutPurpose: Record<string, unknown> = { ...agentInspectorResponse().profile };
    delete profileWithoutPurpose.purpose;
    const malformed: unknown[] = [
      agentInspectorResponse({ runtime_status: "online" }),
      agentInspectorResponse({ agent_id: "019b0000-0000-7000-8000-000000000103" }),
      agentInspectorResponse({ owner: {
        ...agentInspectorResponse().owner,
        credential_fingerprint: "must-not-be-disclosed",
      } }),
      agentInspectorResponse({ profile: {
        ...profileWithoutPurpose,
        runtime_status: "online",
      } }),
      agentInspectorResponse({ profile: {
        ...agentInspectorResponse().profile,
        review_state: "due",
      } }),
      agentInspectorResponse({ token_access: {
        ...agentInspectorResponse().token_access,
        effective_scopes: ["organization.read", "organization.read"],
      } }),
      agentInspectorResponse({ token_access: {
        ...agentInspectorResponse().token_access,
        effective_scopes: ["runtime.admin"],
      } }),
      agentInspectorResponse({ token_access: {
        ...agentInspectorResponse().token_access,
        active_credential_count: 4_294_967_296,
      } }),
      agentInspectorResponse({ token_access: {
        ...agentInspectorResponse().token_access,
        state: "no_effective_scope",
      } }),
      agentInspectorResponse({ token_access: {
        ...agentInspectorResponse().token_access,
        next_credential_expiry_at: null,
      } }),
      agentInspectorResponse({ control_plane: {
        ...agentInspectorResponse().control_plane,
        last_processing_activity_at: "2026-08-23T09:02:00Z",
      } }),
      agentInspectorResponse({ profile: {
        ...agentInspectorResponse().profile,
        environment: "testing",
      } }),
    ];
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    for (const body of malformed) {
      fetchMock.mockResolvedValueOnce(jsonResponse(body));
      await expect(client.getAgentInspector(agentId))
        .rejects.toMatchObject({ resource: "agent_inspector" });
    }
  });

  it("rejects malformed and nil Agent Inspector path IDs before fetch", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await expect(client.getAgentInspector("not-a-uuid"))
      .rejects.toMatchObject({ resource: "agent_inspector" });
    await expect(client.getAgentInspector("00000000-0000-0000-0000-000000000000"))
      .rejects.toMatchObject({ resource: "agent_inspector" });
    await expect(client.getAgentInspector("019B0000-0000-7000-8000-000000000101"))
      .rejects.toMatchObject({ resource: "agent_inspector" });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("reads one exact Agent mail-submission evidence chain without caching or browser bearer state", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(
      agentMailSubmissionEvidenceResponse({ delivery: null }),
    ));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    const evidence = await client.getAgentMailSubmissionEvidence(
      AGENT_MAIL_EVIDENCE_AGENT_ID,
      AGENT_MAIL_EVIDENCE_SUBMISSION_ID,
    );

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(
      `/api/v2/agents/${AGENT_MAIL_EVIDENCE_AGENT_ID}/mail-submissions/${AGENT_MAIL_EVIDENCE_SUBMISSION_ID}/evidence`,
    );
    expect(init.cache).toBe("no-store");
    expect(init.credentials).toBe("same-origin");
    expect(init.method).toBe("POST");
    expect(init.body).toBeUndefined();
    expect(init.headers).not.toHaveProperty("Authorization");
    expect(evidence).toMatchObject({
      agent_id: AGENT_MAIL_EVIDENCE_AGENT_ID,
      submission: {
        submission_id: AGENT_MAIL_EVIDENCE_SUBMISSION_ID,
        status: "accepted",
      },
      delivery: null,
    });
    expect(JSON.stringify(evidence)).not.toContain("snapshot_id");
  });

  it("accepts a pending admission recorded after the initial intent timestamp", async () => {
    const base = agentMailSubmissionEvidenceResponse();
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(agentMailSubmissionEvidenceResponse({
      submission: {
        ...base.submission,
        status: "pending",
        updated_at: "2026-08-24T09:00:00.000000001Z",
      },
      delivery: null,
      audit_references: base.audit_references.slice(0, 2),
    })));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    const evidence = await client.getAgentMailSubmissionEvidence(
      AGENT_MAIL_EVIDENCE_AGENT_ID,
      AGENT_MAIL_EVIDENCE_SUBMISSION_ID,
    );

    expect(evidence.submission.status).toBe("pending");
    expect(evidence.delivery).toBeNull();
    expect(evidence.audit_references).toHaveLength(2);
  });

  it("does not send malformed, nil, or noncanonical Agent submission evidence identifiers", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    for (const [agentId, submissionId] of [
      ["not-a-uuid", AGENT_MAIL_EVIDENCE_SUBMISSION_ID],
      ["00000000-0000-0000-0000-000000000000", AGENT_MAIL_EVIDENCE_SUBMISSION_ID],
      [AGENT_MAIL_EVIDENCE_AGENT_ID.toUpperCase(), AGENT_MAIL_EVIDENCE_SUBMISSION_ID],
      [AGENT_MAIL_EVIDENCE_AGENT_ID, "not-a-uuid"],
      [AGENT_MAIL_EVIDENCE_AGENT_ID, "00000000-0000-0000-0000-000000000000"],
      [AGENT_MAIL_EVIDENCE_AGENT_ID, AGENT_MAIL_EVIDENCE_SUBMISSION_ID.toUpperCase()],
    ]) {
      await expect(client.getAgentMailSubmissionEvidence(agentId, submissionId))
        .rejects.toMatchObject({ resource: "agent_mail_submission_evidence" });
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("fails closed on unbounded, sensitive, or mismatched Agent submission evidence", async () => {
    const base = agentMailSubmissionEvidenceResponse();
    const alternativeId = "019b0000-0000-7000-8000-00000000011b";
    const malformed: unknown[] = [
      agentMailSubmissionEvidenceResponse({ recipient: "must-not-cross-contract" }),
      agentMailSubmissionEvidenceResponse({ runtime_status: "online" }),
      agentMailSubmissionEvidenceResponse({ execution: { state: "running" } }),
      agentMailSubmissionEvidenceResponse({
        agent_id: alternativeId,
      }),
      agentMailSubmissionEvidenceResponse({
        submission: { ...base.submission, submission_id: alternativeId },
      }),
      agentMailSubmissionEvidenceResponse({
        submission: { ...base.submission, status: "delivered" },
      }),
      agentMailSubmissionEvidenceResponse({
        submission: { ...base.submission, updated_at: "not-an-rfc3339-time" },
      }),
      agentMailSubmissionEvidenceResponse({
        submission: {
          ...base.submission,
          created_at: "2026-08-24T09:01:01Z",
        },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: {
          ...base.admission,
          policy: {
            ...base.admission.policy,
            activated_at: "2026-08-24T09:00:02Z",
          },
        },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: { ...base.admission, recipient_count: 0 },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: { ...base.admission, recipient_count: 101 },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: { ...base.admission, first_contact_count: 3 },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: {
          ...base.admission,
          first_contact_approval_request_id: null,
        },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: {
          ...base.admission,
          first_contact_count: 0,
        },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: {
          ...base.admission,
          first_contact_approval_request_id: "00000000-0000-0000-0000-000000000000",
        },
      }),
      agentMailSubmissionEvidenceResponse({
        admission: {
          ...base.admission,
          policy: { ...base.admission.policy, version: 0 },
        },
      }),
      agentMailSubmissionEvidenceResponse({
        delivery: { ...base.delivery, snapshot_id: alternativeId },
      }),
      agentMailSubmissionEvidenceResponse({
        delivery: {
          ...base.delivery,
          delivered_count: 2,
          failed_count: 1,
          queued_count: 0,
          unknown_count: 0,
          terminal: true,
        },
      }),
      agentMailSubmissionEvidenceResponse({
        delivery: { ...base.delivery, terminal: true },
      }),
      agentMailSubmissionEvidenceResponse({
        submission: { ...base.submission, status: "pending" },
      }),
      agentMailSubmissionEvidenceResponse({
        audit_references: [
          base.audit_references[0],
          base.audit_references[1],
          {
            ...base.audit_references[2],
            event_id: AGENT_MAIL_EVIDENCE_ADMITTED_EVENT_ID,
          },
        ],
      }),
      agentMailSubmissionEvidenceResponse({
        audit_references: [
          base.audit_references[0],
          base.audit_references[1],
          { ...base.audit_references[2], action: "mail.submission.rejected" },
        ],
      }),
      agentMailSubmissionEvidenceResponse({
        audit_references: [
          { ...base.audit_references[0], occurred_at: "2026-08-24T09:00:01Z" },
          base.audit_references[1],
          base.audit_references[2],
        ],
      }),
      agentMailSubmissionEvidenceResponse({
        audit_references: [
          base.audit_references[0],
          { ...base.audit_references[1], occurred_at: "2026-08-24T09:01:00Z" },
          base.audit_references[2],
        ],
      }),
      agentMailSubmissionEvidenceResponse({
        audit_references: [
          base.audit_references[0],
          base.audit_references[1],
          { ...base.audit_references[2], occurred_at: "2026-08-24T09:00:00.999999999Z" },
        ],
      }),
      agentMailSubmissionEvidenceResponse({
        delivery: { ...base.delivery, observed_at: "2026-08-24T09:00:59.999999999Z" },
      }),
      agentMailSubmissionEvidenceResponse({
        observed_at: "2026-08-24T09:00:30Z",
      }),
    ];
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    for (const body of malformed) {
      fetchMock.mockResolvedValueOnce(jsonResponse(body));
      await expect(client.getAgentMailSubmissionEvidence(
        AGENT_MAIL_EVIDENCE_AGENT_ID,
        AGENT_MAIL_EVIDENCE_SUBMISSION_ID,
      )).rejects.toMatchObject({ resource: "agent_mail_submission_evidence" });
    }
  });

  it("keeps approval inbox filters scoped and binds decisions to the path", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        request: approvalRequestResponse({ approval_request_id: SECOND_APPROVAL_ID }),
        steps: [],
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: APPROVAL_ID,
        status: "pending",
        approval_count: 1,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: SECOND_APPROVAL_ID,
        status: "cancelled",
        approval_count: 0,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);
    const cursor = `v1.approval.requested.pending.${ORGANIZATION_ID}.${SECOND_APPROVAL_ID}`;

    await client.listApprovalRequests({
      view: "requested",
      status: "pending",
      cursor,
      limit: 25,
    });
    await client.decideApprovalRequest(APPROVAL_ID, "approved", "Scope verified", {
      approval_count: 0,
      rejection_count: 0,
      required_approvals: 2,
    });
    await client.cancelApprovalRequest(SECOND_APPROVAL_ID);

    expect(fetchMock.mock.calls[0][0]).toBe(
      `/api/v2/approval-requests/${SECOND_APPROVAL_ID}`,
    );
    expect(fetchMock.mock.calls[1][0]).toBe(
      `/api/v2/organizations/${ORGANIZATION_ID}/approval-requests?view=requested&status=pending&cursor=${encodeURIComponent(cursor)}&limit=25`,
    );
    expect(fetchMock.mock.calls[2]).toEqual([
      `/api/v2/approval-requests/${APPROVAL_ID}/decisions`,
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ decision: "approved", reason: "Scope verified" }),
      }),
    ]);
    expect(fetchMock.mock.calls[3]).toEqual([
      `/api/v2/approval-requests/${SECOND_APPROVAL_ID}/cancellation`,
      expect.objectContaining({ method: "POST", body: "{}" }),
    ]);
  });

  it("creates an approval request with the exact deny-unknown-fields body", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: APPROVAL_ID,
        status: "pending",
      }), { status: 201, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        request: approvalRequestResponse({
          resource_id: "archive-1",
          expires_at: "2026-08-25T09:00:00.000Z",
        }),
        steps: [],
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);
    const input = {
      policy_key: "audit.export",
      action: "audit.export" as const,
      resource_kind: "audit_archive",
      resource_id: "archive-1",
      reason: "Export evidence for case AC-42",
      ticket_reference: "AC-42",
      expires_at: "2026-08-25T09:00:00.000Z",
      unexpected_view_state: "must-not-cross-the-contract-boundary",
    };

    const result = await client.createApprovalRequest(input);

    expect(result).toEqual({
      approval_request_id: APPROVAL_ID,
      status: "pending",
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v2/approval-requests",
      expect.objectContaining({ method: "POST" }),
    );
    const body = JSON.parse(
      String((fetchMock.mock.calls[0][1] as RequestInit).body),
    ) as Record<string, unknown>;
    expect(body).toEqual({
      policy_key: "audit.export",
      action: "audit.export",
      resource_kind: "audit_archive",
      resource_id: "archive-1",
      reason: "Export evidence for case AC-42",
      ticket_reference: "AC-42",
      expires_at: "2026-08-25T09:00:00.000Z",
    });
    expect(body).not.toHaveProperty("unexpected_view_state");
  });

  it("strictly parses and request-binds approval pages, details, and outcomes", async () => {
    const pendingWithStep = approvalRequestResponse({ approval_count: 1 });
    const nextCursor = `v1.approval.actionable.all.${ORGANIZATION_ID}.${APPROVAL_ID}`;
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [approvalRequestResponse()],
        next_cursor: nextCursor,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        request: pendingWithStep,
        steps: [{
          approval_step_id: APPROVAL_STEP_ID,
          approver_principal_id: APPROVER_ID,
          decision: "approved",
          reason: "Verified independent evidence",
          decided_at: "2026-08-24T02:00:00+00:00",
        }],
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: APPROVAL_ID,
        status: "rejected",
        approval_count: 1,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await expect(client.listApprovalRequests({ view: "actionable", limit: 1 }))
      .resolves.toMatchObject({ has_more: true, next_cursor: nextCursor });
    await expect(client.getApprovalRequest(APPROVAL_ID)).resolves.toMatchObject({
      request: { approval_request_id: APPROVAL_ID, organization_id: ORGANIZATION_ID },
      steps: [{ approval_step_id: APPROVAL_STEP_ID, decision: "approved" }],
    });
    await expect(client.decideApprovalRequest(
      APPROVAL_ID,
      "rejected",
      "Evidence conflicts",
      { approval_count: 1, rejection_count: 0, required_approvals: 2 },
    )).resolves.toEqual({
      approval_request_id: APPROVAL_ID,
      status: "rejected",
      approval_count: 1,
    });
  });

  it("proves every cursor page is ordered strictly after its immutable anchor", async () => {
    const anchor = approvalRequestResponse({
      approval_request_id: SECOND_APPROVAL_ID,
      created_at: "2026-08-24T02:00:00+00:00",
      expires_at: "2026-08-25T02:00:00+00:00",
    });
    const invalidNewerItem = approvalRequestResponse({
      created_at: "2026-08-24T03:00:00+00:00",
      expires_at: "2026-08-25T03:00:00+00:00",
    });
    const cursor = `v1.approval.requested.all.${ORGANIZATION_ID}.${SECOND_APPROVAL_ID}`;
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        request: anchor,
        steps: [],
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [invalidNewerItem],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(new ControlPlaneClient("", ORGANIZATION_ID)
      .listApprovalRequests({ view: "requested", cursor, limit: 25 }))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      `/api/v2/approval-requests/${SECOND_APPROVAL_ID}`,
      `/api/v2/organizations/${ORGANIZATION_ID}/approval-requests?view=requested&cursor=${encodeURIComponent(cursor)}&limit=25`,
    ]);
  });

  it("rejects unknown, cross-tenant, lifecycle-invalid, and unbound approval 2xx bodies", async () => {
    const malformedPages: unknown[] = [
      {
        items: [], next_cursor: null, has_more: false, unexpected: true,
      },
      {
        items: [approvalRequestResponse({ organization_id: OTHER_ORGANIZATION_ID })],
        next_cursor: null,
        has_more: false,
      },
      {
        items: [approvalRequestResponse({ action: "audit.unknown" })],
        next_cursor: null,
        has_more: false,
      },
      {
        items: [approvalRequestResponse({ created_at: "2026-08-24 01:00:00Z" })],
        next_cursor: null,
        has_more: false,
      },
      {
        items: [approvalRequestResponse({ approval_count: 2 })],
        next_cursor: null,
        has_more: false,
      },
      {
        items: [approvalRequestResponse({ resource_id: "x".repeat(501) })],
        next_cursor: null,
        has_more: false,
      },
      {
        items: [approvalRequestResponse()],
        next_cursor: `v1.approval.actionable.all.${ORGANIZATION_ID}.${SECOND_APPROVAL_ID}`,
        has_more: true,
      },
    ];
    for (const body of malformedPages) {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify(body), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })));
      await expect(new ControlPlaneClient("", ORGANIZATION_ID)
        .listApprovalRequests({ view: "actionable", limit: 1 }))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
    }

    const malformedDetails: unknown[] = [
      { request: approvalRequestResponse(), steps: [], unexpected: true },
      {
        request: approvalRequestResponse({ approval_request_id: SECOND_APPROVAL_ID }),
        steps: [],
      },
      {
        request: approvalRequestResponse(),
        steps: [{
          approval_step_id: APPROVAL_STEP_ID,
          approver_principal_id: APPROVER_ID,
          decision: "approved",
          reason: "Uncounted decision",
          decided_at: "2026-08-24T02:00:00+00:00",
        }],
      },
      {
        request: approvalRequestResponse({ approval_count: 1 }),
        steps: [{
          approval_step_id: APPROVAL_STEP_ID,
          approver_principal_id: APPROVER_ID,
          decision: "approved",
          reason: "Unknown field",
          decided_at: "2026-08-24T02:00:00+00:00",
          unexpected: true,
        }],
      },
    ];
    for (const body of malformedDetails) {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify(body), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })));
      await expect(new ControlPlaneClient("", ORGANIZATION_ID)
        .getApprovalRequest(APPROVAL_ID))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
    }

    const malformedOutcomes: unknown[] = [
      { approval_request_id: APPROVAL_ID, status: "pending", approval_count: 1, extra: true },
      { approval_request_id: SECOND_APPROVAL_ID, status: "pending", approval_count: 1 },
      { approval_request_id: APPROVAL_ID, status: "cancelled", approval_count: 1 },
      { approval_request_id: APPROVAL_ID, status: "pending", approval_count: 6 },
    ];
    for (const body of malformedOutcomes) {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify(body), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })));
      await expect(new ControlPlaneClient("", ORGANIZATION_ID)
        .decideApprovalRequest(APPROVAL_ID, "approved", "Verified", {
          approval_count: 0,
          rejection_count: 0,
          required_approvals: 2,
        }))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
    }

    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);
    await expect(client.getApprovalRequest("../other-tenant"))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
    await expect(client.listApprovalRequests({
      view: "requested",
      cursor: `v1.approval.actionable.all.${ORGANIZATION_ID}.${APPROVAL_ID}`,
    })).rejects.toBeInstanceOf(InvalidApiResponseError);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects malformed approval create and cancellation responses", async () => {
    const createBodies = [
      { approval_request_id: APPROVAL_ID, status: "pending", extra: true },
      { approval_request_id: "not-a-uuid", status: "pending" },
      { approval_request_id: APPROVAL_ID, status: "approved" },
    ];
    for (const body of createBodies) {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify(body), {
        status: 201,
        headers: { "Content-Type": "application/json" },
      })));
      await expect(new ControlPlaneClient("", ORGANIZATION_ID).createApprovalRequest({
        policy_key: "audit.export",
        action: "audit.export",
        resource_kind: "audit_archive",
        resource_id: ARCHIVE_ID,
        reason: "Export evidence",
        ticket_reference: null,
        expires_at: "2026-08-25T09:00:00.000Z",
      })).rejects.toBeInstanceOf(InvalidApiResponseError);
    }

    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({
      approval_request_id: APPROVAL_ID,
      status: "pending",
      approval_count: 0,
    }), { status: 200, headers: { "Content-Type": "application/json" } })));
    await expect(new ControlPlaneClient("", ORGANIZATION_ID)
      .cancelApprovalRequest(APPROVAL_ID))
      .rejects.toBeInstanceOf(InvalidApiResponseError);
  });

  it("does not accept create success until a strict read-back binds immutable command fields", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: APPROVAL_ID,
        status: "pending",
      }), { status: 201, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        request: approvalRequestResponse({ resource_id: SECOND_APPROVAL_ID }),
        steps: [],
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(new ControlPlaneClient("", ORGANIZATION_ID).createApprovalRequest({
      policy_key: "audit.export",
      action: "audit.export",
      resource_kind: "audit_archive",
      resource_id: ARCHIVE_ID,
      reason: "Export evidence for case AC-42",
      ticket_reference: "AC-42",
      expires_at: "2026-08-25T01:00:00+00:00",
    })).rejects.toMatchObject({ resource: "approval_request_created" });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("binds decision outcomes to the exact pre-write count and threshold", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      approval_request_id: APPROVAL_ID,
      status: "pending",
      approval_count: 2,
    }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await expect(client.decideApprovalRequest(APPROVAL_ID, "approved", "Verified", {
      approval_count: 1,
      rejection_count: 0,
      required_approvals: 2,
    })).rejects.toBeInstanceOf(InvalidApiResponseError);

    const noFetch = vi.fn();
    vi.stubGlobal("fetch", noFetch);
    await expect(client.decideApprovalRequest(APPROVAL_ID, "approved", "Verified", {
      approval_count: 2,
      rejection_count: 0,
      required_approvals: 2,
    })).rejects.toBeInstanceOf(InvalidApiResponseError);
    expect(noFetch).not.toHaveBeenCalled();
  });

  it("downloads an exact export artifact and exposes its verified digest", async () => {
    const fetchMock = vi.fn().mockResolvedValue(binaryArtifactResponse());
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    const result = await client.downloadAuditExportArtifact(GRANT_ID, "events");

    expect(fetchMock).toHaveBeenCalledWith(
      `/api/v2/audit-export-grants/${GRANT_ID}/artifacts/events`,
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
      }),
    );
    expect((fetchMock.mock.calls[0][1] as RequestInit).headers)
      .not.toHaveProperty("Authorization");
    expect(result.sha256).toBe(ARTIFACT_SHA256);
    expect(result.fileName).toBe(`${GRANT_ID}-events.jsonl`);
    expect(await result.blob.text()).toBe(ARTIFACT_BODY);
  });

  it("rejects malformed artifact enums, sizes, headers, and digest evidence", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(binaryArtifactResponse({ "Content-Length": "16" }))
      .mockResolvedValueOnce(binaryArtifactResponse({
        "x-copilotmail-artifact-sha256": "b".repeat(64),
      }))
      .mockResolvedValueOnce(binaryArtifactResponse({ "Cache-Control": "private" }))
      .mockResolvedValueOnce(binaryArtifactResponse({
        "Content-Length": String(256 * 1024 * 1024 + 1),
      }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", ORGANIZATION_ID);

    await expect(client.downloadAuditExportArtifact(
      GRANT_ID,
      "archive" as unknown as "events",
    )).rejects.toBeInstanceOf(InvalidApiResponseError);
    expect(fetchMock).not.toHaveBeenCalled();
    for (let index = 0; index < 4; index += 1) {
      await expect(client.downloadAuditExportArtifact(GRANT_ID, "events"))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
    }
  });

  it("revalidates the Session when an export download returns 401", async () => {
    const onUnauthorized = vi.fn();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({
      code: "unauthenticated",
      message: "session expired",
      request_id: "request-export-401",
      trace_id: "trace-export-401",
      retryable: false,
    }), { status: 401, headers: { "Content-Type": "application/json" } })));
    const client = new ControlPlaneClient("", ORGANIZATION_ID, onUnauthorized);

    await expect(client.downloadAuditExportArtifact(GRANT_ID, "events"))
      .rejects.toBeInstanceOf(ApiError);
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });

  it("keeps retention drafts separate from approval-bound activation", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        retention_policy_id: "policy-1",
        organization_id: "org-1",
        policy_key: "audit.standard",
        display_name: "Standard",
        resource_kind: "audit_evidence",
        active_version: null,
        latest_version: {
          retention_policy_version_id: "version-1",
          version: 1,
          minimum_retention_days: 2555,
          archive_after_days: 30,
          delete_after_days: null,
          rule: {},
          created_by: "human-1",
          created_at: "2026-07-26T00:00:00Z",
        },
        created_by: "human-1",
        created_at: "2026-07-26T00:00:00Z",
      }), { status: 201, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        retention_policy_version_id: "version-1",
        version: 1,
        minimum_retention_days: 2555,
        archive_after_days: 30,
        delete_after_days: null,
        rule: {},
        created_by: "human-1",
        created_at: "2026-07-26T00:00:00Z",
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    const created = await client.createRetentionPolicy({
      policy_key: "audit.standard",
      display_name: "Standard",
      resource_kind: "audit_evidence",
      minimum_retention_days: 2555,
      archive_after_days: 30,
      delete_after_days: null,
      rule: {},
    });
    await client.activateRetentionPolicyVersion(
      created.retention_policy_id,
      created.latest_version.retention_policy_version_id,
      "approval-1",
    );

    expect(fetchMock.mock.calls[0][0]).toBe(
      "/api/v2/organizations/org-1/retention-policies",
    );
    expect(fetchMock.mock.calls[1]).toEqual([
      "/api/v2/retention-policies/policy-1/versions/version-1/activation",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ approval_request_id: "approval-1" }),
      }),
    ]);
  });

  it("binds role assignment filters, approval grants, and revocation paths", async () => {
    const emptyPage = { items: [], next_cursor: null, has_more: false };
    const assignment = {
      role_assignment_id: "assignment-1",
      organization_id: "org-1",
      principal_id: "principal-1",
      principal_kind: "human",
      principal_status: "active",
      principal_display_name: "A. User",
      role_id: "role-1",
      system_role: "employee",
      role_display_name: "Employee",
      created_by: "owner-1",
      approval_request_id: "approval-1",
      valid_from: "2026-07-26T00:00:00Z",
      valid_until: "2026-08-01T00:00:00Z",
      revoked_at: null,
      created_at: "2026-07-26T00:00:00Z",
      active: true,
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(emptyPage), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }))
      .mockResolvedValueOnce(new Response(JSON.stringify(assignment), {
        status: 201,
        headers: { "Content-Type": "application/json" },
      }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        ...assignment,
        active: false,
        revoked_at: "2026-07-27T00:00:00Z",
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await client.listRoleAssignments({
      principal_id: "principal-1",
      include_inactive: true,
      cursor: "cursor-1",
      limit: 25,
    });
    await client.grantRoleAssignment({
      principal_id: "principal-1",
      role_id: "role-1",
      valid_until: "2026-08-01T00:00:00.000Z",
      approval_request_id: "approval-1",
    });
    await client.revokeRoleAssignment("assignment-1");

    expect(fetchMock.mock.calls[0][0]).toBe(
      "/api/v2/organizations/org-1/role-assignments?principal_id=principal-1&include_inactive=true&cursor=cursor-1&limit=25",
    );
    expect(fetchMock.mock.calls[1]).toEqual([
      "/api/v2/organizations/org-1/role-assignments",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          principal_id: "principal-1",
          role_id: "role-1",
          valid_until: "2026-08-01T00:00:00.000Z",
          approval_request_id: "approval-1",
        }),
      }),
    ]);
    expect(fetchMock.mock.calls[2]).toEqual([
      "/api/v2/role-assignments/assignment-1/revocation",
      expect.objectContaining({ method: "POST", body: "{}" }),
    ]);
  });

  describe("mail domain DTO and client boundary", () => {
    it("accepts only tenant-bound, canonical-name ordered domain pages", async () => {
      const alpha = mailDomainResponse({
        mail_domain_id: SECOND_MAIL_DOMAIN_ID,
        name: "alpha.example",
      });
      const beta = mailDomainResponse({ name: "beta.example" });
      const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
        items: [alpha, beta],
        next_after: "beta.example",
      }));
      vi.stubGlobal("fetch", fetchMock);
      const client = new ControlPlaneClient("", ORGANIZATION_ID);

      const page = await client.listMailDomains({
        after: "aardvark.example",
        limit: 2,
      });

      expect(page.items.map((item) => item.name)).toEqual(["alpha.example", "beta.example"]);
      expect(page.next_after).toBe("beta.example");
      expect(fetchMock.mock.calls[0][0]).toBe(
        `/api/v2/organizations/${ORGANIZATION_ID}/mail-domains?after=aardvark.example&limit=2`,
      );
      expect(JSON.stringify(page)).not.toContain("ownership_token");
    });

    it("rejects cross-tenant, noncanonical, unordered, stale-cursor, and extra page data", async () => {
      const fetchMock = vi.fn()
        .mockResolvedValueOnce(jsonResponse({
          items: [mailDomainResponse({ organization_id: OTHER_ORGANIZATION_ID })],
          next_after: null,
        }))
        .mockResolvedValueOnce(jsonResponse({
          items: [mailDomainResponse({ name: "Example.test" })],
          next_after: null,
        }))
        .mockResolvedValueOnce(jsonResponse({
          items: [
            mailDomainResponse({ mail_domain_id: SECOND_MAIL_DOMAIN_ID, name: "zeta.example" }),
            mailDomainResponse({ name: "alpha.example" }),
          ],
          next_after: null,
        }))
        .mockResolvedValueOnce(jsonResponse({
          items: [mailDomainResponse({ name: "alpha.example" })],
          next_after: "not-the-final.example",
        }))
        .mockResolvedValueOnce(jsonResponse({
          items: [],
          next_after: null,
          unexpected: true,
        }));
      vi.stubGlobal("fetch", fetchMock);
      const client = new ControlPlaneClient("", ORGANIZATION_ID);

      for (let index = 0; index < 5; index += 1) {
        await expect(client.listMailDomains({ limit: 1 }))
          .rejects.toBeInstanceOf(InvalidApiResponseError);
      }

      const noFetch = vi.fn();
      vi.stubGlobal("fetch", noFetch);
      await expect(client.listMailDomains({ after: "Example.test" }))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
      await expect(client.listMailDomains({ limit: 101 }))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
      expect(noFetch).not.toHaveBeenCalled();
    });

    it("whitelists registration and challenge bodies while retaining one-time tokens only in disclosures", async () => {
      const canonicalName = "xn--bcher-kva.example";
      const fetchMock = vi.fn()
        .mockResolvedValueOnce(jsonResponse(mailDomainCreateResponse({
          name: canonicalName,
          ownership_record_name: `_copilotmail-verification.${canonicalName}`,
        }), 201))
        .mockResolvedValueOnce(jsonResponse(mailDomainResponse({ name: canonicalName })))
        .mockResolvedValueOnce(jsonResponse(mailDomainResponse()))
        .mockResolvedValueOnce(jsonResponse(mailDomainChallengeResponse()))
        .mockResolvedValueOnce(jsonResponse(mailDomainResponse({
          ownership_challenge_version: 2,
        })));
      vi.stubGlobal("fetch", fetchMock);
      const client = new ControlPlaneClient("", ORGANIZATION_ID);

      const created = await client.createMailDomain({
        name: "BÜCHER.example.",
        route_mode: "direct_mx",
        unexpected_view_state: "must not cross the boundary",
      } as never);
      const rotated = await client.rotateMailDomainOwnershipChallenge(MAIL_DOMAIN_ID, 1);

      expect(created).toMatchObject({
        name: canonicalName,
        ownership_token: OWNERSHIP_TOKEN,
      });
      expect(rotated).toMatchObject({
        ownership_challenge_version: 2,
        ownership_token: ROTATED_OWNERSHIP_TOKEN,
      });
      expect(JSON.parse(String((fetchMock.mock.calls[0][1] as RequestInit).body))).toEqual({
        name: canonicalName,
        route_mode: "direct_mx",
      });
      expect(JSON.parse(String((fetchMock.mock.calls[3][1] as RequestInit).body))).toEqual({
        expected_version: 1,
      });
      expect(JSON.stringify(client)).not.toContain(OWNERSHIP_TOKEN);
      expect(JSON.stringify(client)).not.toContain(ROTATED_OWNERSHIP_TOKEN);
    });

    it("binds DNS verification to the selected canonical domain and never puts its token in a URL", async () => {
      const verifiedDomain = mailDomainResponse({
        status: "verified",
        latest_dns_observed_at: DNS_OBSERVED_AT,
        latest_dns_valid_until: DNS_VALID_UNTIL,
        latest_dns_activation_ready: true,
      });
      const fetchMock = vi.fn()
        .mockResolvedValueOnce(jsonResponse(mailDomainResponse()))
        .mockResolvedValueOnce(jsonResponse(mailDomainDnsVerificationResponse()))
        .mockResolvedValueOnce(jsonResponse(verifiedDomain));
      vi.stubGlobal("fetch", fetchMock);
      const client = new ControlPlaneClient("", ORGANIZATION_ID);

      const result = await client.verifyMailDomainDns(MAIL_DOMAIN_ID, {
        domain: MAIL_DOMAIN_NAME,
        ownership_token: OWNERSHIP_TOKEN,
        dkim_selector: "mail2026",
        injected_field: "must not cross the boundary",
      } as never);

      expect(result).toMatchObject({
        mail_domain_id: MAIL_DOMAIN_ID,
        status: "verified",
        activation_ready: true,
      });
      const [url, init] = fetchMock.mock.calls[1] as [string, RequestInit];
      expect(url).toBe(`/api/v2/mail-domains/${MAIL_DOMAIN_ID}/dns-verifications`);
      expect(url).not.toContain(OWNERSHIP_TOKEN);
      expect(JSON.parse(String(init.body))).toEqual({
        domain: MAIL_DOMAIN_NAME,
        ownership_token: OWNERSHIP_TOKEN,
        dkim_selector: "mail2026",
      });
      expect(JSON.stringify(result)).not.toContain(OWNERSHIP_TOKEN);
    });

    it("read-backs bind activation, suspension, and route CAS results to their exact operations", async () => {
      const verified = mailDomainResponse({
        status: "verified",
        latest_dns_observed_at: DNS_OBSERVED_AT,
        latest_dns_valid_until: DNS_VALID_UNTIL,
        latest_dns_activation_ready: true,
      });
      const active = mailDomainResponse({
        status: "active",
        mail_engine_projection_status: "pending",
        latest_dns_observed_at: DNS_OBSERVED_AT,
        latest_dns_valid_until: DNS_VALID_UNTIL,
        latest_dns_activation_ready: true,
      });
      const suspended = mailDomainResponse({
        status: "suspended",
        route_version: 2,
        mail_engine_projection_status: "pending",
        latest_dns_observed_at: DNS_OBSERVED_AT,
        latest_dns_valid_until: DNS_VALID_UNTIL,
        latest_dns_activation_ready: true,
      });
      const routePreimage = mailDomainResponse({
        status: "active",
        route_version: 2,
        mail_engine_projection_status: "pending",
      });
      const routeReadBack = mailDomainResponse({
        status: "active",
        route_version: 3,
        inbound_enabled: true,
        outbound_enabled: true,
        dkim_key_ref: "vault://dkim/key-1",
        mail_engine_projection_status: "pending",
      });
      const fetchMock = vi.fn()
        .mockResolvedValueOnce(jsonResponse(verified))
        .mockResolvedValueOnce(jsonResponse({
          mail_domain_id: MAIL_DOMAIN_ID,
          status: "active",
        }))
        .mockResolvedValueOnce(jsonResponse(active))
        .mockResolvedValueOnce(jsonResponse(active))
        .mockResolvedValueOnce(jsonResponse({
          mail_domain_id: MAIL_DOMAIN_ID,
          status: "suspended",
        }))
        .mockResolvedValueOnce(jsonResponse(suspended))
        .mockResolvedValueOnce(jsonResponse(routePreimage))
        .mockResolvedValueOnce(jsonResponse({
          mail_domain_id: MAIL_DOMAIN_ID,
          version: 3,
          inbound_enabled: true,
          outbound_enabled: true,
        }))
        .mockResolvedValueOnce(jsonResponse(routeReadBack));
      vi.stubGlobal("fetch", fetchMock);
      const client = new ControlPlaneClient("", ORGANIZATION_ID);

      await expect(client.activateMailDomain(MAIL_DOMAIN_ID)).resolves.toMatchObject({
        status: "active",
      });
      await expect(client.suspendMailDomain(MAIL_DOMAIN_ID, {
        reason_code: "operator_review",
      })).resolves.toMatchObject({ status: "suspended" });
      await expect(client.updateMailDomainRoute(MAIL_DOMAIN_ID, {
        expected_version: 2,
        inbound_enabled: true,
        outbound_enabled: true,
        smart_host_ref: null,
        dkim_key_ref: "vault://dkim/key-1",
        ignored_view_only_value: true,
      } as never)).resolves.toMatchObject({ version: 3 });

      expect((fetchMock.mock.calls[1][1] as RequestInit).body).toBeUndefined();
      expect(JSON.parse(String((fetchMock.mock.calls[4][1] as RequestInit).body))).toEqual({
        reason_code: "operator_review",
      });
      expect(JSON.parse(String((fetchMock.mock.calls[7][1] as RequestInit).body))).toEqual({
        expected_version: 2,
        inbound_enabled: true,
        outbound_enabled: true,
        smart_host_ref: null,
        dkim_key_ref: "vault://dkim/key-1",
      });
    });

    it("fails closed before any domain write for malformed IDs, versions, names, and bodies", async () => {
      const fetchMock = vi.fn();
      vi.stubGlobal("fetch", fetchMock);
      const client = new ControlPlaneClient("", ORGANIZATION_ID);

      await expect(client.getMailDomain("not-a-uuid"))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
      await expect(client.createMailDomain({
        name: "example.test",
        route_mode: "relay" as never,
      })).rejects.toBeInstanceOf(InvalidApiResponseError);
      await expect(client.rotateMailDomainOwnershipChallenge(MAIL_DOMAIN_ID, 0))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
      await expect(client.suspendMailDomain(MAIL_DOMAIN_ID, { reason_code: "not allowed" }))
        .rejects.toBeInstanceOf(InvalidApiResponseError);
      await expect(client.updateMailDomainRoute("not-a-uuid", {
        expected_version: 1,
        inbound_enabled: false,
        outbound_enabled: false,
        smart_host_ref: null,
        dkim_key_ref: null,
      })).rejects.toBeInstanceOf(InvalidApiResponseError);
      expect(fetchMock).not.toHaveBeenCalled();
    });
  });
});
