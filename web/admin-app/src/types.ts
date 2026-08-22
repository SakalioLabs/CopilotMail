export type Locale = "zh-CN" | "en";
export type CaseStatus = "open" | "closed";
export type HoldStatus = "active" | "released";
export type AuditOutcome = "success" | "failure" | "denied";
export type ApprovalStatus =
  | "pending"
  | "approved"
  | "rejected"
  | "expired"
  | "cancelled";
export type ApprovalView = "actionable" | "requested";

export interface RuntimeConfig {
  apiBaseUrl: string;
  organizationId: string;
}

export interface HumanSessionResponse {
  principal_id: string;
  organization_id: string;
  client: "admin" | "mail";
  expires_at: string;
}

export interface ComplianceCase {
  case_id: string;
  organization_id: string;
  external_reference: string;
  title: string;
  purpose: string;
  status: CaseStatus;
  created_by: string;
  created_at: string;
  closed_by: string | null;
  closed_at: string | null;
}

export interface LegalHold {
  legal_hold_id: string;
  organization_id: string;
  case_id: string;
  archive_id: string;
  reason: string;
  status: HoldStatus;
  apply_approval_request_id: string;
  created_by: string;
  created_at: string;
  release_approval_request_id: string | null;
  released_by: string | null;
  released_at: string | null;
}

export interface Page<T> {
  items: T[];
  next_cursor: string | null;
  has_more: boolean;
}

export interface ApprovalRequest {
  approval_request_id: string;
  organization_id: string;
  policy_version_id: string;
  action: string;
  resource_kind: string;
  resource_id: string;
  requested_by: string;
  reason: string;
  ticket_reference: string | null;
  required_approvals: number;
  approval_count: number;
  rejection_count: number;
  status: ApprovalStatus;
  created_at: string;
  expires_at: string;
  completed_at: string | null;
  cancelled_at: string | null;
}

export interface ApprovalStep {
  approval_step_id: string;
  approver_principal_id: string;
  decision: "approved" | "rejected";
  reason: string;
  decided_at: string;
}

export interface ApprovalRequestDetail {
  request: ApprovalRequest;
  steps: ApprovalStep[];
}

export type SystemRole =
  | "org_owner"
  | "mail_admin"
  | "security_admin"
  | "compliance_auditor"
  | "content_reviewer"
  | "helpdesk"
  | "agent_operator"
  | "employee"
  | "break_glass";

export interface Role {
  role_id: string;
  organization_id: string;
  system_role: SystemRole;
  display_name: string;
  permissions: string[];
  created_at: string;
}

export interface RoleAssignment {
  role_assignment_id: string;
  organization_id: string;
  principal_id: string;
  principal_kind: "human" | "agent" | "service";
  principal_status: "draft" | "active" | "suspended" | "retired";
  principal_display_name: string;
  role_id: string;
  system_role: SystemRole;
  role_display_name: string;
  created_by: string;
  approval_request_id: string | null;
  valid_from: string;
  valid_until: string | null;
  revoked_at: string | null;
  created_at: string;
  active: boolean;
}

export interface AuditEvent {
  event_id: string;
  organization_id: string;
  actor_principal_id: string;
  actor_kind: "human" | "agent" | "service";
  on_behalf_of_principal_id: string | null;
  credential_id: string | null;
  subject_principal_id: string | null;
  action: string;
  resource_type: string;
  resource_id: string;
  outcome: AuditOutcome;
  trace_id: string;
  request_id: string;
  source_ip: string | null;
  client_id: string | null;
  session_id: string | null;
  token_jti: string | null;
  policy_id: string | null;
  policy_version_id: string | null;
  approval_request_id: string | null;
  ticket_reference: string | null;
  environment: string | null;
  region: string | null;
  details: Record<string, unknown>;
  occurred_at: string;
  received_at: string;
  chain_sequence: string;
  hash_version: number;
  previous_hash: string | null;
  event_hash: string;
  signature_batch_id: string | null;
}

export interface AuditEventSearchPage extends Page<AuditEvent> {
  query_id: string;
  occurred_from: string;
  occurred_to: string;
}

export interface AuditEventFilters {
  occurred_from: string;
  occurred_to: string;
  action?: string;
  actor_principal_id?: string;
  resource_type?: string;
  resource_id?: string;
  outcome?: AuditOutcome;
  request_id?: string;
}

export type AuditExportArtifact = "events" | "manifest";

export interface AuditExportGrant {
  audit_export_grant_id: string;
  organization_id: string;
  archive_id: string;
  approval_request_id: string;
  requested_by: string;
  created_at: string;
  expires_at: string;
}

export type RetentionResourceKind = "audit_evidence" | "mail_content";

export interface RetentionPolicyVersion {
  retention_policy_version_id: string;
  version: number;
  minimum_retention_days: number;
  archive_after_days: number | null;
  delete_after_days: number | null;
  rule: Record<string, unknown>;
  created_by: string;
  created_at: string;
}

export interface RetentionPolicy {
  retention_policy_id: string;
  organization_id: string;
  policy_key: string;
  display_name: string;
  resource_kind: RetentionResourceKind;
  active_version: RetentionPolicyVersion | null;
  latest_version: RetentionPolicyVersion;
  created_by: string;
  created_at: string;
}

export interface ApiErrorBody {
  code: string;
  message: string;
  request_id: string;
  trace_id: string;
  retryable: boolean;
  details?: unknown;
}
