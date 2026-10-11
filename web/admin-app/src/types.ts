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
export type MailDomainStatus =
  | "draft"
  | "pending_dns"
  | "verified"
  | "active"
  | "suspended"
  | "retired";
export type MailDomainRouteMode = "direct_mx" | "smart_host";
export type MailEngineProjectionStatus =
  | "not_projected"
  | "pending"
  | "ready"
  | "failed";
export type ApprovalAction =
  | "mail.send_as"
  | "mail.send_on_behalf_of"
  | "compliance.content_review"
  | "audit.export"
  | "compliance.retention_change"
  | "authorization.privilege_elevation"
  | "compliance.legal_hold"
  | "security.break_glass"
  | "mail.bulk_delete"
  | "agent.production_mail"
  | "agent.first_contact";
export type PrincipalStatus = "draft" | "active" | "suspended" | "retired";
export type AgentEnvironment = "development" | "testing" | "production";
export type AgentRiskLevel = "low" | "medium" | "high";
export type AgentMailSubmissionStatus =
  | "pending"
  | "reconciling"
  | "accepted"
  | "rejected"
  | "manual_review";
export type AgentInspectorReviewState = "scheduled" | "due";
export type AgentInspectorTokenAccessState =
  | "identity_inactive"
  | "no_active_credential"
  | "no_effective_scope"
  | "effective";
export type AgentInspectorMailAdmissionState =
  | "not_required"
  | "blocked_no_active_policy"
  | "active";

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

/**
 * Tenant-bound read model for a managed mail domain.
 *
 * Deliberately excludes the ownership challenge token. That token is returned
 * only by the registration and rotation responses below, and must never be
 * copied into cached domain state, URL state, logs, or browser persistence.
 */
export interface MailDomain {
  mail_domain_id: string;
  organization_id: string;
  /** Canonical lowercase ASCII/Punycode DNS name. */
  name: string;
  status: MailDomainStatus;
  route_mode: MailDomainRouteMode;
  ownership_challenge_version: number;
  route_version: number;
  inbound_enabled: boolean;
  outbound_enabled: boolean;
  smart_host_ref: string | null;
  dkim_key_ref: string | null;
  mail_engine_domain_ref: string | null;
  mail_engine_projection_status: MailEngineProjectionStatus;
  mail_engine_projected_enabled: boolean | null;
  mail_engine_projection_last_attempt_at: string | null;
  mail_engine_projection_succeeded_at: string | null;
  mail_engine_projection_error_code: string | null;
  latest_dns_observed_at: string | null;
  latest_dns_valid_until: string | null;
  latest_dns_activation_ready: boolean;
  latest_dns_failure_codes: string[];
}

/** Canonical-name cursor page returned by GET /mail-domains. */
export interface MailDomainPage {
  items: MailDomain[];
  /** Exact canonical name of the final item when another page exists. */
  next_after: string | null;
}

/** Exact allowlist accepted by POST /organizations/{id}/mail-domains. */
export interface MailDomainCreateInput {
  name: string;
  route_mode: MailDomainRouteMode;
}

/**
 * One-time secret disclosure from registration.
 *
 * `ownership_token` is intentionally absent from MailDomain and MailDomainPage.
 * A caller may show or copy it during this immediate acknowledgement only; it
 * must not be persisted, logged, retried from storage, or placed in a URL.
 */
export interface MailDomainCreateResponse {
  mail_domain_id: string;
  organization_id: string;
  name: string;
  status: "pending_dns";
  ownership_challenge_version: number;
  route_version: number;
  ownership_record_name: string;
  readonly ownership_token: string;
}

/** Exact allowlist accepted by POST /ownership-challenge. */
export interface MailDomainChallengeRotationInput {
  expected_version: number;
}

/** One-time secret disclosure after a successful challenge rotation. */
export interface MailDomainChallengeRotationResponse {
  mail_domain_id: string;
  status: "pending_dns";
  ownership_challenge_version: number;
  ownership_record_name: string;
  readonly ownership_token: string;
}

/** Exact allowlist accepted by POST /dns-verifications. */
export interface MailDomainDnsVerificationInput {
  domain: string;
  /** Current one-time ownership token; sent only in this request body. */
  ownership_token: string;
  dkim_selector: string;
}

export interface MailDomainDnsVerificationResponse {
  mail_domain_id: string;
  status: "pending_dns" | "verified" | "active";
  activation_ready: boolean;
  valid_until: string;
  failure_codes: string[];
}

export interface MailDomainSuspendInput {
  reason_code: string;
}

export interface MailDomainActionResponse {
  mail_domain_id: string;
  status: "active" | "suspended";
}

/** Exact allowlist accepted by PUT /route. */
export interface MailDomainRouteUpdateInput {
  expected_version: number;
  inbound_enabled: boolean;
  outbound_enabled: boolean;
  smart_host_ref: string | null;
  dkim_key_ref: string | null;
}

export interface MailDomainRouteResponse {
  mail_domain_id: string;
  version: number;
  inbound_enabled: boolean;
  outbound_enabled: boolean;
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

/** Exact body accepted by POST /api/v2/approval-requests. */
export interface ApprovalRequestCreateInput {
  policy_key: string;
  action: ApprovalAction;
  resource_kind: string;
  resource_id: string;
  reason: string;
  ticket_reference: string | null;
  expires_at: string;
}

/** Minimal, authoritative result returned when an approval request is created. */
export interface ApprovalRequestCreated {
  approval_request_id: string;
  status: "pending";
}

export interface ApprovalDecisionResult {
  approval_request_id: string;
  status: ApprovalStatus;
  approval_count: number;
}

export interface ApprovalDecisionExpectation {
  approval_count: number;
  rejection_count: number;
  required_approvals: number;
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
  principal_status: PrincipalStatus;
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

/**
 * A deliberately bounded control-plane observation, not an Agent runtime,
 * delivery, heartbeat, mailbox, delegation, or credential-material view.
 */
export interface AgentInspector {
  agent_id: string;
  display_name: string;
  principal_status: PrincipalStatus;
  owner: AgentInspectorOwner;
  profile: AgentInspectorProfile;
  token_access: AgentInspectorTokenAccess;
  control_plane: AgentInspectorControlPlane;
  observed_at: string;
}

export interface AgentInspectorOwner {
  principal_id: string;
  display_name: string;
  principal_status: PrincipalStatus;
}

export interface AgentInspectorProfile {
  environment: AgentEnvironment;
  risk_level: AgentRiskLevel;
  purpose: string;
  review_due_at: string;
  review_state: AgentInspectorReviewState;
}

export interface AgentInspectorTokenAccess {
  state: AgentInspectorTokenAccessState;
  effective_scopes: Array<
    "organization.read" | "agent.operate" | "agent.mail.read" | "mailbox.use"
  >;
  active_credential_count: number;
  next_credential_expiry_at: string | null;
  last_authenticated_at: string | null;
}

export interface AgentInspectorControlPlane {
  mail_admission_state: AgentInspectorMailAdmissionState;
  active_processing_subscription_count: number;
  paused_processing_subscription_count: number;
  retired_processing_subscription_count: number;
  last_processing_activity_at: string | null;
}

/**
 * A Human-only, exact evidence read for one production Agent submission.
 * It contains only immutable admission/approval facts, an optional aggregate
 * delivery observation, and minimal audit references. It is not an Agent
 * runtime, execution, delegation, mailbox, recipient, provider, SMTP, or
 * content view.
 */
export interface AgentMailSubmissionEvidence {
  agent_id: string;
  submission: AgentMailSubmissionEvidenceSubmission;
  admission: AgentMailSubmissionEvidenceAdmission;
  delivery: AgentMailSubmissionEvidenceDelivery | null;
  audit_references: AgentMailSubmissionEvidenceAuditReference[];
  observed_at: string;
}

export interface AgentMailSubmissionEvidenceSubmission {
  submission_id: string;
  status: AgentMailSubmissionStatus;
  created_at: string;
  updated_at: string;
}

export interface AgentMailSubmissionEvidenceAdmission {
  admission_id: string;
  admitted_at: string;
  policy: AgentMailSubmissionEvidencePolicy;
  recipient_count: number;
  first_contact_count: number;
  first_contact_approval_request_id: string | null;
}

export interface AgentMailSubmissionEvidencePolicy {
  activation_id: string;
  activated_at: string;
  version_id: string;
  version: number;
  approval_request_id: string;
}

export interface AgentMailSubmissionEvidenceDelivery {
  observed_at: string;
  terminal: boolean;
  recipient_count: number;
  delivered_count: number;
  failed_count: number;
  queued_count: number;
  unknown_count: number;
}

export interface AgentMailSubmissionEvidenceAuditReference {
  event_id: string;
  action: string;
  occurred_at: string;
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
