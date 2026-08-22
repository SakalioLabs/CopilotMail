import type {
  ApiErrorBody,
  AuditEventFilters,
  AuditEventSearchPage,
  AuditExportArtifact,
  AuditExportGrant,
  ApprovalRequest,
  ApprovalRequestDetail,
  ApprovalStatus,
  ApprovalView,
  CaseStatus,
  ComplianceCase,
  HoldStatus,
  LegalHold,
  Page,
  RetentionPolicy,
  RetentionResourceKind,
  Role,
  RoleAssignment,
} from "./types";
import { sameOriginApiBaseUrl } from "./config";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly body: ApiErrorBody,
  ) {
    super(body.message);
    this.name = "ApiError";
  }
}

export function buildQuery(
  values: Record<string, string | number | boolean | undefined | null>,
): string {
  const query = new URLSearchParams();
  Object.entries(values).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== "") {
      query.set(key, String(value));
    }
  });
  const serialized = query.toString();
  return serialized ? `?${serialized}` : "";
}

export class ControlPlaneClient {
  private readonly baseUrl: string;

  constructor(
    baseUrl: string,
    private readonly organizationId: string,
  ) {
    this.baseUrl = sameOriginApiBaseUrl(baseUrl);
  }

  listCases(input: {
    status?: CaseStatus;
    cursor?: string;
    limit?: number;
  }): Promise<Page<ComplianceCase>> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/compliance-cases${buildQuery(input)}`,
    );
  }

  listApprovalRequests(input: {
    view: ApprovalView;
    status?: ApprovalStatus;
    cursor?: string;
    limit?: number;
  }): Promise<Page<ApprovalRequest>> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/approval-requests${buildQuery(input)}`,
    );
  }

  getApprovalRequest(requestId: string): Promise<ApprovalRequestDetail> {
    return this.request(`/api/v2/approval-requests/${requestId}`);
  }

  decideApprovalRequest(
    requestId: string,
    decision: "approved" | "rejected",
    reason: string,
  ): Promise<{ approval_request_id: string; status: ApprovalStatus; approval_count: number }> {
    return this.request(`/api/v2/approval-requests/${requestId}/decisions`, {
      method: "POST",
      body: JSON.stringify({ decision, reason }),
    });
  }

  cancelApprovalRequest(
    requestId: string,
  ): Promise<{ approval_request_id: string; status: ApprovalStatus; approval_count: number }> {
    return this.request(`/api/v2/approval-requests/${requestId}/cancellation`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  listRoles(): Promise<{ items: Role[] }> {
    return this.request(`/api/v2/organizations/${this.organizationId}/roles`);
  }

  listRoleAssignments(input: {
    principal_id?: string;
    include_inactive?: boolean;
    cursor?: string;
    limit?: number;
  }): Promise<Page<RoleAssignment>> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/role-assignments${buildQuery(input)}`,
    );
  }

  grantRoleAssignment(input: {
    principal_id: string;
    role_id: string;
    valid_until: string;
    approval_request_id: string;
  }): Promise<RoleAssignment> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/role-assignments`,
      { method: "POST", body: JSON.stringify(input) },
    );
  }

  revokeRoleAssignment(assignmentId: string): Promise<RoleAssignment> {
    return this.request(`/api/v2/role-assignments/${assignmentId}/revocation`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  searchAuditEvents(
    filters: AuditEventFilters,
    input: { cursor?: string; limit?: number } = {},
  ): Promise<AuditEventSearchPage> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/audit-events${buildQuery({
        ...filters,
        ...input,
      })}`,
    );
  }

  authorizeAuditExport(
    archiveId: string,
    approvalRequestId: string,
  ): Promise<AuditExportGrant> {
    return this.request(`/api/v2/audit-archives/${archiveId}/exports`, {
      method: "POST",
      body: JSON.stringify({ approval_request_id: approvalRequestId }),
    });
  }

  getAuditExportGrant(grantId: string): Promise<AuditExportGrant> {
    return this.request(`/api/v2/audit-export-grants/${grantId}`);
  }

  createRetentionPolicy(input: {
    policy_key: string;
    display_name: string;
    resource_kind: RetentionResourceKind;
    minimum_retention_days: number;
    archive_after_days: number | null;
    delete_after_days: number | null;
    rule: Record<string, unknown>;
  }): Promise<RetentionPolicy> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/retention-policies`,
      { method: "POST", body: JSON.stringify(input) },
    );
  }

  getRetentionPolicy(policyId: string): Promise<RetentionPolicy> {
    return this.request(`/api/v2/retention-policies/${policyId}`);
  }

  appendRetentionPolicyVersion(
    policyId: string,
    input: {
      minimum_retention_days: number;
      archive_after_days: number | null;
      delete_after_days: number | null;
      rule: Record<string, unknown>;
    },
  ): Promise<RetentionPolicy["latest_version"]> {
    return this.request(`/api/v2/retention-policies/${policyId}/versions`, {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  activateRetentionPolicyVersion(
    policyId: string,
    versionId: string,
    approvalRequestId: string,
  ): Promise<RetentionPolicy["latest_version"]> {
    return this.request(
      `/api/v2/retention-policies/${policyId}/versions/${versionId}/activation`,
      {
        method: "POST",
        body: JSON.stringify({ approval_request_id: approvalRequestId }),
      },
    );
  }

  getCase(caseId: string): Promise<ComplianceCase> {
    return this.request(`/api/v2/compliance-cases/${caseId}`);
  }

  createCase(input: {
    external_reference: string;
    title: string;
    purpose: string;
  }): Promise<ComplianceCase> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/compliance-cases`,
      { method: "POST", body: JSON.stringify(input) },
    );
  }

  closeCase(caseId: string): Promise<ComplianceCase> {
    return this.request(`/api/v2/compliance-cases/${caseId}/closure`, {
      method: "POST",
    });
  }

  listHolds(input: {
    status?: HoldStatus;
    case_id?: string;
    cursor?: string;
    limit?: number;
  }): Promise<Page<LegalHold>> {
    return this.request(
      `/api/v2/organizations/${this.organizationId}/legal-holds${buildQuery(input)}`,
    );
  }

  getHold(holdId: string): Promise<LegalHold> {
    return this.request(`/api/v2/legal-holds/${holdId}`);
  }

  applyHold(
    archiveId: string,
    input: {
      case_id: string;
      approval_request_id: string;
      reason: string;
    },
  ): Promise<LegalHold> {
    return this.request(`/api/v2/audit-archives/${archiveId}/legal-holds`, {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  releaseHold(
    holdId: string,
    approvalRequestId: string,
  ): Promise<LegalHold> {
    return this.request(`/api/v2/legal-holds/${holdId}/release`, {
      method: "POST",
      body: JSON.stringify({ approval_request_id: approvalRequestId }),
    });
  }

  /**
   * Retrieves one verified archive artifact. The control plane already
   * reverified the object against the recorded digest, so the returned blob is
   * paired with that digest for optional client-side checking.
   */
  async downloadAuditExportArtifact(
    grantId: string,
    artifact: AuditExportArtifact,
  ): Promise<{ blob: Blob; sha256: string; fileName: string }> {
    const requestId = crypto.randomUUID();
    const response = await fetch(
      `${this.baseUrl}/api/v2/audit-export-grants/${grantId}/artifacts/${artifact}`,
      {
        method: "POST",
        credentials: "same-origin",
        headers: {
          Accept: "application/octet-stream",
          "x-request-id": requestId,
        },
      },
    );
    if (!response.ok) {
      let body: ApiErrorBody;
      try {
        body = (await response.json()) as ApiErrorBody;
      } catch {
        body = {
          code: "unexpected_response",
          message: "The control plane returned an unreadable response.",
          request_id: requestId,
          trace_id: requestId,
          retryable: response.status >= 500,
        };
      }
      throw new ApiError(response.status, body);
    }
    return {
      blob: await response.blob(),
      sha256: response.headers.get("x-copilotmail-artifact-sha256") ?? "",
      fileName: `${grantId}-${artifact}.${artifact === "events" ? "jsonl" : "json"}`,
    };
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const requestId = crypto.randomUUID();
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...init,
      credentials: "same-origin",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "x-request-id": requestId,
        ...init.headers,
      },
    });
    if (response.ok) {
      return (await response.json()) as T;
    }
    let body: ApiErrorBody;
    try {
      body = (await response.json()) as ApiErrorBody;
    } catch {
      body = {
        code: "unexpected_response",
        message: "The control plane returned an unreadable response.",
        request_id: requestId,
        trace_id: requestId,
        retryable: response.status >= 500,
      };
    }
    throw new ApiError(response.status, body);
  }
}
