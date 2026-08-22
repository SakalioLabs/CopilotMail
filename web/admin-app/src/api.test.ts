import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, buildQuery, ControlPlaneClient } from "./api";

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

  it("reuses the exact audit window and cursor with every active filter", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      query_id: "query-1",
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      items: [],
      next_cursor: null,
      has_more: false,
    }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await client.searchAuditEvents({
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      action: "compliance.case.queried",
      actor_principal_id: "actor-1",
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success",
      request_id: "source-request",
    }, { cursor: "v1.org-1.42", limit: 50 });

    const url = new URL(String(fetchMock.mock.calls[0][0]), "https://control.example");
    expect(Object.fromEntries(url.searchParams)).toEqual({
      occurred_from: "2026-07-25T00:00:00Z",
      occurred_to: "2026-07-26T00:00:00Z",
      action: "compliance.case.queried",
      actor_principal_id: "actor-1",
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success",
      request_id: "source-request",
      cursor: "v1.org-1.42",
      limit: "50",
    });
  });

  it("binds an export authorization to the archive path and approval body", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      audit_export_grant_id: "grant-1",
      organization_id: "org-1",
      archive_id: "archive-1",
      approval_request_id: "approval-1",
      requested_by: "human-1",
      created_at: "2026-07-26T00:00:00Z",
      expires_at: "2026-07-26T00:15:00Z",
    }), { status: 201, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await client.authorizeAuditExport("archive-1", "approval-1");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v2/audit-archives/archive-1/exports",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ approval_request_id: "approval-1" }),
      }),
    );
  });

  it("keeps approval inbox filters scoped and binds decisions to the path", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: "approval-1",
        status: "pending",
        approval_count: 1,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        approval_request_id: "approval-2",
        status: "cancelled",
        approval_count: 0,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    await client.listApprovalRequests({
      view: "requested",
      status: "pending",
      cursor: "cursor-1",
      limit: 25,
    });
    await client.decideApprovalRequest("approval-1", "approved", "Scope verified");
    await client.cancelApprovalRequest("approval-2");

    expect(fetchMock.mock.calls[0][0]).toBe(
      "/api/v2/organizations/org-1/approval-requests?view=requested&status=pending&cursor=cursor-1&limit=25",
    );
    expect(fetchMock.mock.calls[1]).toEqual([
      "/api/v2/approval-requests/approval-1/decisions",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ decision: "approved", reason: "Scope verified" }),
      }),
    ]);
    expect(fetchMock.mock.calls[2]).toEqual([
      "/api/v2/approval-requests/approval-2/cancellation",
      expect.objectContaining({ method: "POST", body: "{}" }),
    ]);
  });

  it("downloads an exact export artifact and exposes its verified digest", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response("{\"sequence\":\"1\"}\n", {
        status: 200,
        headers: {
          "Content-Type": "application/octet-stream",
          "x-copilotmail-artifact-sha256": "a".repeat(64),
        },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const client = new ControlPlaneClient("", "org-1");

    const result = await client.downloadAuditExportArtifact("grant-1", "events");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v2/audit-export-grants/grant-1/artifacts/events",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
      }),
    );
    expect((fetchMock.mock.calls[0][1] as RequestInit).headers)
      .not.toHaveProperty("Authorization");
    expect(result.sha256).toBe("a".repeat(64));
    expect(result.fileName).toBe("grant-1-events.jsonl");
    expect(await result.blob.text()).toBe("{\"sequence\":\"1\"}\n");
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
});
