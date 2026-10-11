import type {
  ApiErrorBody,
  AuditEvent,
  AuditEventFilters,
  AuditEventSearchPage,
  AuditExportArtifact,
  AuditExportGrant,
  AgentEnvironment,
  AgentInspector,
  AgentInspectorMailAdmissionState,
  AgentInspectorReviewState,
  AgentMailSubmissionEvidence,
  AgentMailSubmissionStatus,
  AgentInspectorTokenAccessState,
  AgentRiskLevel,
  ApprovalAction,
  ApprovalDecisionExpectation,
  ApprovalDecisionResult,
  ApprovalRequestCreateInput,
  ApprovalRequestCreated,
  ApprovalRequest,
  ApprovalRequestDetail,
  ApprovalStep,
  ApprovalStatus,
  ApprovalView,
  CaseStatus,
  ComplianceCase,
  HoldStatus,
  LegalHold,
  MailDomain,
  MailDomainActionResponse,
  MailDomainChallengeRotationInput,
  MailDomainChallengeRotationResponse,
  MailDomainCreateInput,
  MailDomainCreateResponse,
  MailDomainDnsVerificationInput,
  MailDomainDnsVerificationResponse,
  MailDomainPage,
  MailDomainRouteMode,
  MailDomainRouteResponse,
  MailDomainRouteUpdateInput,
  MailDomainStatus,
  MailDomainSuspendInput,
  MailEngineProjectionStatus,
  Page,
  PrincipalStatus,
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

/** A successful HTTP response that does not satisfy the public DTO contract. */
export class InvalidApiResponseError extends Error {
  constructor(readonly resource:
    | "audit_event"
    | "audit_event_page"
    | "audit_export_artifact"
    | "audit_export_grant"
    | "agent_inspector"
    | "agent_mail_submission_evidence"
    | "approval_decision"
    | "approval_request"
    | "approval_request_created"
    | "approval_request_detail"
    | "approval_request_page"
    | "approval_step"
    | "compliance_case"
    | "compliance_case_page"
    | "legal_hold"
    | "legal_hold_page"
    | "mail_domain"
    | "mail_domain_action"
    | "mail_domain_challenge"
    | "mail_domain_created"
    | "mail_domain_dns_verification"
    | "mail_domain_page"
    | "mail_domain_route") {
    super(`invalid_${resource}_response`);
    this.name = "InvalidApiResponseError";
  }
}

type JsonRecord = Record<string, unknown>;

const CANONICAL_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const NIL_UUID = "00000000-0000-0000-0000-000000000000";
const LOWER_HEX_SHA256 = /^[0-9a-f]{64}$/;
const RFC3339 = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|([+-])(\d{2}):(\d{2}))$/;
const MAX_AUDIT_EXPORT_ARTIFACT_BYTES = 256 * 1024 * 1024;
const MAX_AUDIT_DETAILS_BYTES = 64 * 1024;
const MAX_MAIL_DOMAIN_PAGE_SIZE = 100;
const MAX_MAIL_DOMAIN_FAILURE_CODES = 32;
const MAX_SAFE_VERSION = Number.MAX_SAFE_INTEGER;
const MAIL_DOMAIN_STATUSES: readonly MailDomainStatus[] = [
  "draft",
  "pending_dns",
  "verified",
  "active",
  "suspended",
  "retired",
];
const MAIL_DOMAIN_ROUTE_MODES: readonly MailDomainRouteMode[] = [
  "direct_mx",
  "smart_host",
];
const MAIL_ENGINE_PROJECTION_STATUSES: readonly MailEngineProjectionStatus[] = [
  "not_projected",
  "pending",
  "ready",
  "failed",
];
const OWNERSHIP_TOKEN = /^cmv1_[A-Za-z0-9_-]{43}$/;
const DNS_FAILURE_CODE = /^[a-z0-9_.-]+$/;
const DKIM_SELECTOR = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/;
const APPROVAL_ACTIONS: readonly ApprovalAction[] = [
  "mail.send_as",
  "mail.send_on_behalf_of",
  "compliance.content_review",
  "audit.export",
  "compliance.retention_change",
  "authorization.privilege_elevation",
  "compliance.legal_hold",
  "security.break_glass",
  "mail.bulk_delete",
  "agent.production_mail",
  "agent.first_contact",
];
const APPROVAL_STATUSES: readonly ApprovalStatus[] = [
  "pending",
  "approved",
  "rejected",
  "expired",
  "cancelled",
];
const PRINCIPAL_STATUSES: readonly PrincipalStatus[] = [
  "draft",
  "active",
  "suspended",
  "retired",
];
const AGENT_ENVIRONMENTS: readonly AgentEnvironment[] = [
  "development",
  "testing",
  "production",
];
const AGENT_RISK_LEVELS: readonly AgentRiskLevel[] = ["low", "medium", "high"];
const AGENT_REVIEW_STATES: readonly AgentInspectorReviewState[] = ["scheduled", "due"];
const AGENT_TOKEN_ACCESS_STATES: readonly AgentInspectorTokenAccessState[] = [
  "identity_inactive",
  "no_active_credential",
  "no_effective_scope",
  "effective",
];
const AGENT_MAIL_ADMISSION_STATES: readonly AgentInspectorMailAdmissionState[] = [
  "not_required",
  "blocked_no_active_policy",
  "active",
];
const AGENT_MAIL_SUBMISSION_STATUSES: readonly AgentMailSubmissionStatus[] = [
  "pending",
  "reconciling",
  "accepted",
  "rejected",
  "manual_review",
];
const AGENT_MAIL_SUBMISSION_AUDIT_ACTIONS = [
  "mail.submission.requested",
  "agent.mail.admitted",
  "mail.submission.accepted",
  "mail.submission.rejected",
  "mail.submission.review_required",
] as const;
const AGENT_INSPECTOR_SCOPES = [
  "organization.read",
  "agent.operate",
  "agent.mail.read",
  "mailbox.use",
] as const;
const APPROVAL_REQUEST_KEYS = [
  "approval_request_id",
  "organization_id",
  "policy_version_id",
  "action",
  "resource_kind",
  "resource_id",
  "requested_by",
  "reason",
  "ticket_reference",
  "required_approvals",
  "approval_count",
  "rejection_count",
  "status",
  "created_at",
  "expires_at",
  "completed_at",
  "cancelled_at",
] as const;
const AUDIT_EVENT_KEYS = [
  "event_id", "organization_id", "actor_principal_id", "actor_kind",
  "on_behalf_of_principal_id", "credential_id", "subject_principal_id",
  "action", "resource_type", "resource_id", "outcome", "trace_id",
  "request_id", "source_ip", "client_id", "session_id", "token_jti",
  "policy_id", "policy_version_id", "approval_request_id",
  "ticket_reference", "environment", "region", "details", "occurred_at",
  "received_at", "chain_sequence", "hash_version", "previous_hash",
  "event_hash", "signature_batch_id",
] as const;

function invalidResponse(
  resource: InvalidApiResponseError["resource"],
): never {
  throw new InvalidApiResponseError(resource);
}

function jsonRecord(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): JsonRecord {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return invalidResponse(resource);
  }
  return value as JsonRecord;
}

function exactKeys(
  record: JsonRecord,
  expected: readonly string[],
  resource: InvalidApiResponseError["resource"],
) {
  const actual = Object.keys(record);
  if (
    actual.length !== expected.length
    || actual.some((key) => !expected.includes(key))
  ) invalidResponse(resource);
}

/**
 * When every declared field is subsequently parsed as required, an exact own
 * key count rejects both added keys and key replacement without duplicating a
 * second copy of the schema's field names in the browser bundle.
 */
function exactKeyCount(
  record: JsonRecord,
  expectedCount: number,
  resource: InvalidApiResponseError["resource"],
) {
  if (Object.keys(record).length !== expectedCount) invalidResponse(resource);
}

function enumValue<T extends string>(
  record: JsonRecord,
  key: string,
  values: readonly T[],
  resource: InvalidApiResponseError["resource"],
): T {
  const value = record[key];
  if (typeof value !== "string" || !values.includes(value as T)) {
    return invalidResponse(resource);
  }
  return value as T;
}

function nonNegativeU32(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): number {
  const value = record[key];
  if (!Number.isInteger(value) || Number(value) < 0 || Number(value) > 4_294_967_295) {
    return invalidResponse(resource);
  }
  return Number(value);
}

function boundedString(
  record: JsonRecord,
  key: string,
  maximum: number,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = record[key];
  if (
    typeof value !== "string"
    || value.length === 0
    || value.length > maximum
    || value.trim() !== value
    || /[\u0000-\u001f\u007f]/.test(value)
  ) {
    return invalidResponse(resource);
  }
  return value;
}

function nullableBoundedString(
  record: JsonRecord,
  key: string,
  maximum: number,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  return boundedString(record, key, maximum, resource);
}

function timestamp(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 64, resource);
  if (!Number.isFinite(Date.parse(value))) return invalidResponse(resource);
  return value;
}

function nullableTimestamp(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  return timestamp(record, key, resource);
}

function inspectorDisplayName(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 200, resource);
  if (
    [...value].length > 200
    || value.split(/\s+/u).join(" ") !== value
  ) return invalidResponse(resource);
  return value;
}

function inspectorPurpose(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 1_000, resource);
  if ([...value].length > 1_000) return invalidResponse(resource);
  return value;
}

function canonicalUuid(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 36, resource);
  if (!CANONICAL_UUID.test(value)) return invalidResponse(resource);
  return value;
}

function nonNilCanonicalUuid(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = canonicalUuid(record, key, resource);
  if (value === NIL_UUID) return invalidResponse(resource);
  return value;
}

function nullableCanonicalUuid(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  return canonicalUuid(record, key, resource);
}

function nullableNonNilCanonicalUuid(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  return nonNilCanonicalUuid(record, key, resource);
}

function rfc3339Timestamp(
    record: JsonRecord,
    key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 40, resource);
  const match = RFC3339.exec(value);
  if (!match) return invalidResponse(resource);
  const [, year, month, day, hour, minute, second, , offsetHour, offsetMinute] = match;
  const numericYear = Number(year);
  const numericMonth = Number(month);
  const numericDay = Number(day);
  const daysInMonth = new Date(Date.UTC(numericYear, numericMonth, 0)).getUTCDate();
  if (
    numericYear < 1
    || numericMonth < 1
    || numericMonth > 12
    || numericDay < 1
    || numericDay > daysInMonth
    || Number(hour) > 23
    || Number(minute) > 59
    || Number(second) > 59
    || (offsetHour !== undefined && Number(offsetHour) > 23)
    || (offsetMinute !== undefined && Number(offsetMinute) > 59)
    || !Number.isFinite(Date.parse(value))
  ) return invalidResponse(resource);
  return value;
}

/**
 * Produces a lossless instant for the narrow evidence parser. `Date.parse`
 * only preserves milliseconds, which is not sufficient to validate immutable
 * server timestamps that may differ within one millisecond.
 */
function rfc3339EpochNanoseconds(
  value: string,
  resource: InvalidApiResponseError["resource"],
): bigint {
  const match = RFC3339.exec(value);
  if (!match) return invalidResponse(resource);
  const [
    ,
    year,
    month,
    day,
    hour,
    minute,
    second,
    fractional = "",
    offsetSign,
    offsetHour,
    offsetMinute,
  ] = match;
  const instant = new Date(0);
  instant.setUTCFullYear(Number(year), Number(month) - 1, Number(day));
  instant.setUTCHours(Number(hour), Number(minute), Number(second), 0);
  const milliseconds = instant.getTime();
  if (!Number.isFinite(milliseconds)) return invalidResponse(resource);

  const localNanoseconds = BigInt(milliseconds) * 1_000_000n
    + BigInt(fractional.padEnd(9, "0") || "0");
  if (offsetSign === undefined || offsetHour === undefined || offsetMinute === undefined) {
    return localNanoseconds;
  }
  const offsetNanoseconds = BigInt(Number(offsetHour) * 3_600 + Number(offsetMinute) * 60)
    * 1_000_000_000n;
  return offsetSign === "+"
    ? localNanoseconds - offsetNanoseconds
    : localNanoseconds + offsetNanoseconds;
}

function lowercaseSha256(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 64, resource);
  if (!LOWER_HEX_SHA256.test(value)) return invalidResponse(resource);
  return value;
}

function nullableLowercaseSha256(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  return lowercaseSha256(record, key, resource);
}

function parsePositiveBigint(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): bigint {
  if (typeof value !== "string" || !/^[1-9][0-9]*$/.test(value)) {
    return invalidResponse(resource);
  }
  const parsed = BigInt(value);
  if (parsed > 9_223_372_036_854_775_807n) return invalidResponse(resource);
  return parsed;
}

function parseComplianceCase(value: unknown, organizationId: string): ComplianceCase {
  const resource = "compliance_case" as const;
  const record = jsonRecord(value, resource);
  const status = record.status;
  if (status !== "open" && status !== "closed") return invalidResponse(resource);
  const parsed: ComplianceCase = {
    case_id: boundedString(record, "case_id", 200, resource),
    organization_id: boundedString(record, "organization_id", 200, resource),
    external_reference: boundedString(record, "external_reference", 200, resource),
    title: boundedString(record, "title", 200, resource),
    purpose: boundedString(record, "purpose", 2000, resource),
    status,
    created_by: boundedString(record, "created_by", 200, resource),
    created_at: timestamp(record, "created_at", resource),
    closed_by: nullableBoundedString(record, "closed_by", 200, resource),
    closed_at: nullableTimestamp(record, "closed_at", resource),
  };
  if (
    parsed.organization_id !== organizationId
    || (status === "open" && (parsed.closed_by !== null || parsed.closed_at !== null))
    || (status === "closed" && (parsed.closed_by === null || parsed.closed_at === null))
  ) {
    return invalidResponse(resource);
  }
  return parsed;
}

function parseLegalHold(value: unknown, organizationId: string): LegalHold {
  const resource = "legal_hold" as const;
  const record = jsonRecord(value, resource);
  const status = record.status;
  if (status !== "active" && status !== "released") return invalidResponse(resource);
  const parsed: LegalHold = {
    legal_hold_id: boundedString(record, "legal_hold_id", 200, resource),
    organization_id: boundedString(record, "organization_id", 200, resource),
    case_id: boundedString(record, "case_id", 200, resource),
    archive_id: boundedString(record, "archive_id", 200, resource),
    reason: boundedString(record, "reason", 2000, resource),
    status,
    apply_approval_request_id: boundedString(record, "apply_approval_request_id", 200, resource),
    created_by: boundedString(record, "created_by", 200, resource),
    created_at: timestamp(record, "created_at", resource),
    release_approval_request_id: nullableBoundedString(
      record,
      "release_approval_request_id",
      200,
      resource,
    ),
    released_by: nullableBoundedString(record, "released_by", 200, resource),
    released_at: nullableTimestamp(record, "released_at", resource),
  };
  const releaseFields = [
    parsed.release_approval_request_id,
    parsed.released_by,
    parsed.released_at,
  ];
  if (
    parsed.organization_id !== organizationId
    || (status === "active" && releaseFields.some((field) => field !== null))
    || (status === "released" && releaseFields.some((field) => field === null))
  ) {
    return invalidResponse(resource);
  }
  return parsed;
}

function parsePage<T>(
  value: unknown,
  resource: "compliance_case_page" | "legal_hold_page",
  parseItem: (item: unknown) => T,
  keyOf: (item: T) => string,
  maximumItems: number,
): Page<T> {
  const record = jsonRecord(value, resource);
  if (!Array.isArray(record.items) || record.items.length > maximumItems) {
    return invalidResponse(resource);
  }
  const nextCursor = record.next_cursor;
  if (
    nextCursor !== null
    && (
      typeof nextCursor !== "string"
      || nextCursor.length === 0
      || nextCursor.length > 4096
      || nextCursor.trim() !== nextCursor
      || /[\u0000-\u001f\u007f]/.test(nextCursor)
    )
  ) {
    return invalidResponse(resource);
  }
  if (
    typeof record.has_more !== "boolean"
    || record.has_more !== (nextCursor !== null)
  ) {
    return invalidResponse(resource);
  }
  const items = record.items.map(parseItem);
  if (new Set(items.map(keyOf)).size !== items.length) return invalidResponse(resource);
  return { items, next_cursor: nextCursor, has_more: record.has_more };
}

function approvalInteger(
  record: JsonRecord,
  key: string,
  minimum: number,
  maximum: number,
  resource: InvalidApiResponseError["resource"],
): number {
  const value = record[key];
  if (!Number.isInteger(value) || Number(value) < minimum || Number(value) > maximum) {
    return invalidResponse(resource);
  }
  return Number(value);
}

function approvalStatus(
  record: JsonRecord,
  resource: InvalidApiResponseError["resource"],
): ApprovalStatus {
  const status = record.status;
  if (!APPROVAL_STATUSES.includes(status as ApprovalStatus)) {
    return invalidResponse(resource);
  }
  return status as ApprovalStatus;
}

function parseApprovalRequest(
  value: unknown,
  organizationId: string,
): ApprovalRequest {
  const resource = "approval_request" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, APPROVAL_REQUEST_KEYS, resource);
  const action = record.action;
  if (!APPROVAL_ACTIONS.includes(action as ApprovalAction)) {
    return invalidResponse(resource);
  }
  const parsed: ApprovalRequest = {
    approval_request_id: canonicalUuid(record, "approval_request_id", resource),
    organization_id: canonicalUuid(record, "organization_id", resource),
    policy_version_id: canonicalUuid(record, "policy_version_id", resource),
    action: action as ApprovalAction,
    resource_kind: boundedString(record, "resource_kind", 100, resource),
    resource_id: boundedString(record, "resource_id", 500, resource),
    requested_by: canonicalUuid(record, "requested_by", resource),
    reason: boundedString(record, "reason", 1_000, resource),
    ticket_reference: nullableBoundedString(record, "ticket_reference", 500, resource),
    required_approvals: approvalInteger(record, "required_approvals", 1, 5, resource),
    approval_count: approvalInteger(record, "approval_count", 0, 5, resource),
    rejection_count: approvalInteger(record, "rejection_count", 0, 1, resource),
    status: approvalStatus(record, resource),
    created_at: rfc3339Timestamp(record, "created_at", resource),
    expires_at: rfc3339Timestamp(record, "expires_at", resource),
    completed_at: record.completed_at === null
      ? null
      : rfc3339Timestamp(record, "completed_at", resource),
    cancelled_at: record.cancelled_at === null
      ? null
      : rfc3339Timestamp(record, "cancelled_at", resource),
  };
  const createdAt = Date.parse(parsed.created_at);
  const expiresAt = Date.parse(parsed.expires_at);
  const completedAt = parsed.completed_at === null ? null : Date.parse(parsed.completed_at);
  const cancelledAt = parsed.cancelled_at === null ? null : Date.parse(parsed.cancelled_at);
  const completedInWindow = completedAt === null
    || (completedAt >= createdAt && completedAt <= expiresAt);
  const cancelledInWindow = cancelledAt === null
    || (cancelledAt >= createdAt && cancelledAt <= expiresAt);
  const belowThreshold = parsed.approval_count < parsed.required_approvals;
  const reachedThreshold = parsed.approval_count >= parsed.required_approvals;
  const lifecycleValid = (
    parsed.status === "pending"
    && belowThreshold
    && parsed.rejection_count === 0
    && completedAt === null
    && cancelledAt === null
  ) || (
    parsed.status === "approved"
    && reachedThreshold
    && parsed.rejection_count === 0
    && completedAt !== null
    && cancelledAt === null
  ) || (
    parsed.status === "rejected"
    && belowThreshold
    && parsed.rejection_count === 1
    && completedAt !== null
    && cancelledAt === null
  ) || (
    parsed.status === "cancelled"
    && belowThreshold
    && parsed.rejection_count === 0
    && completedAt === null
    && cancelledAt !== null
  ) || (
    parsed.status === "expired"
    && parsed.rejection_count === 0
    && cancelledAt === null
    && (
      (belowThreshold && completedAt === null)
      || (reachedThreshold && completedAt !== null)
    )
  );
  if (
    parsed.organization_id !== organizationId
    || expiresAt <= createdAt
    || expiresAt - createdAt > 30 * 24 * 60 * 60 * 1_000
    || parsed.approval_count + parsed.rejection_count > 5
    || !completedInWindow
    || !cancelledInWindow
    || !lifecycleValid
  ) return invalidResponse(resource);
  return parsed;
}

function parseApprovalStep(value: unknown): ApprovalStep {
  const resource = "approval_step" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "approval_step_id",
    "approver_principal_id",
    "decision",
    "reason",
    "decided_at",
  ], resource);
  const decision = record.decision;
  if (decision !== "approved" && decision !== "rejected") {
    return invalidResponse(resource);
  }
  return {
    approval_step_id: canonicalUuid(record, "approval_step_id", resource),
    approver_principal_id: canonicalUuid(record, "approver_principal_id", resource),
    decision,
    reason: boundedString(record, "reason", 1_000, resource),
    decided_at: rfc3339Timestamp(record, "decided_at", resource),
  };
}

function approvalCursorPrefix(
  organizationId: string,
  view: ApprovalView,
  status?: ApprovalStatus,
): string {
  return `v1.approval.${view}.${status ?? "all"}.${organizationId}.`;
}

function approvalCursorRequestId(
  cursor: string,
  organizationId: string,
  view: ApprovalView,
  status: ApprovalStatus | undefined,
  resource: "approval_request_page",
): string {
  const prefix = approvalCursorPrefix(organizationId, view, status);
  const requestId = cursor.startsWith(prefix) ? cursor.slice(prefix.length) : "";
  if (!CANONICAL_UUID.test(requestId) || cursor.length !== prefix.length + 36) {
    return invalidResponse(resource);
  }
  return requestId;
}

function parseApprovalRequestPage(
  value: unknown,
  organizationId: string,
  input: {
    view: ApprovalView;
    status?: ApprovalStatus;
    cursor?: string;
    limit?: number;
  },
  anchor?: ApprovalRequest,
): Page<ApprovalRequest> {
  const resource = "approval_request_page" as const;
  const limit = input.limit ?? 50;
  if (
    !CANONICAL_UUID.test(organizationId)
    || !Number.isInteger(limit)
    || limit < 1
    || limit > 100
    || (input.view === "actionable" && input.status !== undefined && input.status !== "pending")
  ) return invalidResponse(resource);
  if (input.cursor !== undefined) {
    const anchorId = approvalCursorRequestId(
      input.cursor,
      organizationId,
      input.view,
      input.status,
      resource,
    );
    if (!anchor || anchor.approval_request_id !== anchorId) {
      return invalidResponse(resource);
    }
  }
  const record = jsonRecord(value, resource);
  exactKeys(record, ["items", "next_cursor", "has_more"], resource);
  if (!Array.isArray(record.items) || record.items.length > limit) {
    return invalidResponse(resource);
  }
  const items = record.items.map((item) => parseApprovalRequest(item, organizationId));
  if (new Set(items.map((item) => item.approval_request_id)).size !== items.length) {
    return invalidResponse(resource);
  }
  for (const [index, item] of items.entries()) {
    if (
      (input.view === "actionable" && item.status !== "pending")
      || (input.status !== undefined && item.status !== input.status)
    ) return invalidResponse(resource);
    if (anchor !== undefined) {
      const anchorCreatedAt = Date.parse(anchor.created_at);
      const createdAt = Date.parse(item.created_at);
      if (
        createdAt > anchorCreatedAt
        || (createdAt === anchorCreatedAt
          && item.approval_request_id >= anchor.approval_request_id)
      ) return invalidResponse(resource);
    }
    const previous = items[index - 1];
    if (previous !== undefined) {
      const previousCreatedAt = Date.parse(previous.created_at);
      const createdAt = Date.parse(item.created_at);
      if (
        previousCreatedAt < createdAt
        || (previousCreatedAt === createdAt
          && previous.approval_request_id <= item.approval_request_id)
      ) return invalidResponse(resource);
    }
  }
  if (typeof record.has_more !== "boolean") return invalidResponse(resource);
  const nextCursor = record.next_cursor;
  const expectedCursor = items.length === 0
    ? null
    : `${approvalCursorPrefix(organizationId, input.view, input.status)}${items.at(-1)!.approval_request_id}`;
  if (
    (record.has_more && (items.length !== limit || nextCursor !== expectedCursor))
    || (!record.has_more && nextCursor !== null)
  ) return invalidResponse(resource);
  return { items, next_cursor: nextCursor as string | null, has_more: record.has_more };
}

function parseApprovalRequestCreated(value: unknown): ApprovalRequestCreated {
  const resource = "approval_request_created" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, ["approval_request_id", "status"], resource);
  if (record.status !== "pending") return invalidResponse(resource);
  return {
    approval_request_id: canonicalUuid(record, "approval_request_id", resource),
    status: record.status,
  };
}

function parseApprovalRequestDetail(
  value: unknown,
  organizationId: string,
  requestId: string,
): ApprovalRequestDetail {
  const resource = "approval_request_detail" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, ["request", "steps"], resource);
  if (!Array.isArray(record.steps) || record.steps.length > 5) {
    return invalidResponse(resource);
  }
  const request = parseApprovalRequest(record.request, organizationId);
  const steps = record.steps.map(parseApprovalStep);
  if (
    request.approval_request_id !== requestId
    || new Set(steps.map((step) => step.approval_step_id)).size !== steps.length
    || new Set(steps.map((step) => step.approver_principal_id)).size !== steps.length
    || steps.filter((step) => step.decision === "approved").length !== request.approval_count
    || steps.filter((step) => step.decision === "rejected").length !== request.rejection_count
  ) return invalidResponse(resource);
  const createdAt = Date.parse(request.created_at);
  const expiresAt = Date.parse(request.expires_at);
  for (const [index, step] of steps.entries()) {
    const decidedAt = Date.parse(step.decided_at);
    const previous = steps[index - 1];
    if (
      decidedAt < createdAt
      || decidedAt > expiresAt
      || (request.completed_at !== null && decidedAt > Date.parse(request.completed_at))
      || (previous !== undefined && (
        Date.parse(previous.decided_at) > decidedAt
        || (Date.parse(previous.decided_at) === decidedAt
          && previous.approval_step_id >= step.approval_step_id)
      ))
    ) return invalidResponse(resource);
  }
  return { request, steps };
}

function parseApprovalDecision(
  value: unknown,
  requestId: string,
  expected: "approved" | "rejected" | "cancelled",
  expectation?: ApprovalDecisionExpectation,
): ApprovalDecisionResult {
  const resource = "approval_decision" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, ["approval_request_id", "status", "approval_count"], resource);
  const parsed: ApprovalDecisionResult = {
    approval_request_id: canonicalUuid(record, "approval_request_id", resource),
    status: approvalStatus(record, resource),
    approval_count: approvalInteger(record, "approval_count", 0, 5, resource),
  };
  let statusMatches = parsed.status === expected;
  let countMatches = true;
  if (expected === "approved") {
    if (!validApprovalDecisionExpectation(expectation)) {
      return invalidResponse(resource);
    }
    const resultingCount = expectation.approval_count + 1;
    statusMatches = parsed.status === (
      resultingCount >= expectation.required_approvals ? "approved" : "pending"
    );
    countMatches = parsed.approval_count === resultingCount;
  } else if (expected === "rejected") {
    if (!validApprovalDecisionExpectation(expectation)) {
      return invalidResponse(resource);
    }
    countMatches = parsed.approval_count === expectation.approval_count;
  }
  if (
    parsed.approval_request_id !== requestId
    || !statusMatches
    || !countMatches
  ) {
    return invalidResponse(resource);
  }
  return parsed;
}

function validApprovalDecisionExpectation(
  expectation: ApprovalDecisionExpectation | undefined,
): expectation is ApprovalDecisionExpectation {
  return expectation !== undefined
    && Number.isInteger(expectation.approval_count)
    && Number.isInteger(expectation.rejection_count)
    && Number.isInteger(expectation.required_approvals)
    && expectation.approval_count >= 0
    && expectation.approval_count <= 4
    && expectation.rejection_count === 0
    && expectation.required_approvals >= 1
    && expectation.required_approvals <= 5
    && expectation.approval_count < expectation.required_approvals;
}

function parseAuditEvent(value: unknown, organizationId: string): AuditEvent {
  const resource = "audit_event" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, AUDIT_EVENT_KEYS, resource);
  const actorKind = record.actor_kind;
  const outcome = record.outcome;
  if (
    (actorKind !== "human" && actorKind !== "agent" && actorKind !== "service")
    || (outcome !== "success" && outcome !== "failure" && outcome !== "denied")
  ) return invalidResponse(resource);
  const action = boundedString(record, "action", 100, resource);
  const resourceType = boundedString(record, "resource_type", 100, resource);
  if (!/^[a-z][a-z0-9_.]+$/.test(action) || !/^[a-z][a-z0-9_.]+$/.test(resourceType)) {
    return invalidResponse(resource);
  }
  const details = jsonRecord(record.details, resource);
  let detailsSize: number;
  try {
    detailsSize = new TextEncoder().encode(JSON.stringify(details)).byteLength;
  } catch {
    return invalidResponse(resource);
  }
  if (detailsSize > MAX_AUDIT_DETAILS_BYTES) return invalidResponse(resource);
  const chainSequence = parsePositiveBigint(record.chain_sequence, resource);
  const hashVersion = record.hash_version;
  if (!Number.isInteger(hashVersion) || Number(hashVersion) < 1 || Number(hashVersion) > 32_767) {
    return invalidResponse(resource);
  }
  const parsed: AuditEvent = {
    event_id: canonicalUuid(record, "event_id", resource),
    organization_id: canonicalUuid(record, "organization_id", resource),
    actor_principal_id: canonicalUuid(record, "actor_principal_id", resource),
    actor_kind: actorKind,
    on_behalf_of_principal_id: nullableCanonicalUuid(record, "on_behalf_of_principal_id", resource),
    credential_id: nullableBoundedString(record, "credential_id", 500, resource),
    subject_principal_id: nullableCanonicalUuid(record, "subject_principal_id", resource),
    action,
    resource_type: resourceType,
    resource_id: boundedString(record, "resource_id", 512, resource),
    outcome,
    trace_id: boundedString(record, "trace_id", 200, resource),
    request_id: boundedString(record, "request_id", 200, resource),
    source_ip: nullableBoundedString(record, "source_ip", 64, resource),
    client_id: nullableBoundedString(record, "client_id", 500, resource),
    session_id: nullableBoundedString(record, "session_id", 500, resource),
    token_jti: nullableBoundedString(record, "token_jti", 500, resource),
    policy_id: nullableCanonicalUuid(record, "policy_id", resource),
    policy_version_id: nullableCanonicalUuid(record, "policy_version_id", resource),
    approval_request_id: nullableCanonicalUuid(record, "approval_request_id", resource),
    ticket_reference: nullableBoundedString(record, "ticket_reference", 500, resource),
    environment: nullableBoundedString(record, "environment", 100, resource),
    region: nullableBoundedString(record, "region", 100, resource),
    details,
    occurred_at: rfc3339Timestamp(record, "occurred_at", resource),
    received_at: rfc3339Timestamp(record, "received_at", resource),
    chain_sequence: chainSequence.toString(),
    hash_version: Number(hashVersion),
    previous_hash: nullableLowercaseSha256(record, "previous_hash", resource),
    event_hash: lowercaseSha256(record, "event_hash", resource),
    signature_batch_id: nullableCanonicalUuid(record, "signature_batch_id", resource),
  };
  if (
    parsed.organization_id !== organizationId
    || Date.parse(parsed.received_at) < Date.parse(parsed.occurred_at)
  ) return invalidResponse(resource);
  return parsed;
}

function auditCursorSequence(
  value: unknown,
  organizationId: string,
  resource: "audit_event_page",
): bigint {
  if (
    typeof value !== "string"
    || value.length > 64
    || !value.startsWith(`v1.${organizationId}.`)
  ) return invalidResponse(resource);
  return parsePositiveBigint(value.slice(`v1.${organizationId}.`.length), resource);
}

function parseAuditEventPage(
  value: unknown,
  organizationId: string,
  filters: AuditEventFilters,
  input: { cursor?: string; limit?: number },
): AuditEventSearchPage {
  const resource = "audit_event_page" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "query_id",
    "occurred_from",
    "occurred_to",
    "items",
    "next_cursor",
    "has_more",
  ], resource);
  const maximumItems = input.limit ?? 50;
  if (!Number.isInteger(maximumItems) || maximumItems < 1 || maximumItems > 100) {
    return invalidResponse(resource);
  }
  if (!Array.isArray(record.items) || record.items.length > maximumItems) {
    return invalidResponse(resource);
  }
  const envelope = {
    query_id: canonicalUuid(record, "query_id", resource),
    occurred_from: rfc3339Timestamp(record, "occurred_from", resource),
    occurred_to: rfc3339Timestamp(record, "occurred_to", resource),
  };
  const from = Date.parse(envelope.occurred_from);
  const to = Date.parse(envelope.occurred_to);
  if (
    from !== Date.parse(filters.occurred_from)
    || to !== Date.parse(filters.occurred_to)
    || from > to
    || to - from > 31 * 24 * 60 * 60 * 1000
    || typeof record.has_more !== "boolean"
  ) return invalidResponse(resource);
  const items = record.items.map((item) => parseAuditEvent(item, organizationId));
  if (new Set(items.map((item) => item.event_id)).size !== items.length) {
    return invalidResponse(resource);
  }
  for (const [index, item] of items.entries()) {
    const occurredAt = Date.parse(item.occurred_at);
    const previous = items[index - 1];
    if (
      occurredAt < from
      || occurredAt > to
      || (filters.action !== undefined && item.action !== filters.action)
      || (filters.actor_principal_id !== undefined
        && item.actor_principal_id !== filters.actor_principal_id)
      || (filters.resource_type !== undefined && item.resource_type !== filters.resource_type)
      || (filters.resource_id !== undefined && item.resource_id !== filters.resource_id)
      || (filters.outcome !== undefined && item.outcome !== filters.outcome)
      || (filters.request_id !== undefined && item.request_id !== filters.request_id)
      || (previous !== undefined
        && BigInt(previous.chain_sequence) <= BigInt(item.chain_sequence))
    ) return invalidResponse(resource);
  }
  const requestedCursorSequence = input.cursor === undefined
    ? undefined
    : auditCursorSequence(input.cursor, organizationId, resource);
  if (
    requestedCursorSequence !== undefined
    && items.some((item) => BigInt(item.chain_sequence) >= requestedCursorSequence)
  ) return invalidResponse(resource);
  const nextCursor = record.next_cursor;
  const nextCursorSequence = nextCursor === null
    ? undefined
    : auditCursorSequence(nextCursor, organizationId, resource);
  const expectedNextSequence = items.length
    ? BigInt(items.at(-1)!.chain_sequence)
    : requestedCursorSequence;
  if (
    (record.has_more && (items.length !== maximumItems || nextCursorSequence === undefined))
    || nextCursorSequence !== expectedNextSequence
  ) return invalidResponse(resource);
  return {
    ...envelope,
    items,
    next_cursor: nextCursor as string | null,
    has_more: record.has_more,
  };
}

function parseAuditExportGrant(
  value: unknown,
  organizationId: string,
): AuditExportGrant {
  const resource = "audit_export_grant" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "audit_export_grant_id",
    "organization_id",
    "archive_id",
    "approval_request_id",
    "requested_by",
    "created_at",
    "expires_at",
  ], resource);
  const parsed: AuditExportGrant = {
    audit_export_grant_id: canonicalUuid(record, "audit_export_grant_id", resource),
    organization_id: canonicalUuid(record, "organization_id", resource),
    archive_id: canonicalUuid(record, "archive_id", resource),
    approval_request_id: canonicalUuid(record, "approval_request_id", resource),
    requested_by: canonicalUuid(record, "requested_by", resource),
    created_at: rfc3339Timestamp(record, "created_at", resource),
    expires_at: rfc3339Timestamp(record, "expires_at", resource),
  };
  const lifetime = Date.parse(parsed.expires_at) - Date.parse(parsed.created_at);
  if (
    parsed.organization_id !== organizationId
    || lifetime <= 0
    || lifetime > 30 * 60 * 1000
  ) return invalidResponse(resource);
  return parsed;
}

function nullableRfc3339Timestamp(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  return rfc3339Timestamp(record, key, resource);
}

function safePositiveVersion(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): number {
  if (!Number.isSafeInteger(value) || Number(value) < 1 || Number(value) > MAX_SAFE_VERSION) {
    return invalidResponse(resource);
  }
  return Number(value);
}

function versionFromRecord(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): number {
  return safePositiveVersion(record[key], resource);
}

function ownershipToken(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 512, resource);
  if (!OWNERSHIP_TOKEN.test(value)) return invalidResponse(resource);
  return value;
}

function validOwnershipToken(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string {
  if (typeof value !== "string" || !OWNERSHIP_TOKEN.test(value)) {
    return invalidResponse(resource);
  }
  return value;
}

function isIpv4Address(value: string): boolean {
  const labels = value.split(".");
  return labels.length === 4
    && labels.every((label) => /^\d{1,3}$/.test(label) && Number(label) <= 255);
}

/**
 * Mirrors the server's DNS-name normalization closely enough for the browser
 * boundary: IDNs become lower-case ASCII/Punycode, one terminal root label is
 * removed, and only RFC-compatible DNS labels remain. The server remains the
 * authoritative validator, but the browser never sends an ambiguous cursor or
 * treats a non-canonical read-model name as trusted state.
 */
function canonicalizeMailDomainName(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string {
  if (
    typeof value !== "string"
    || value.length === 0
    || /[\u0000-\u001f\u007f]/.test(value)
  ) return invalidResponse(resource);
  const trimmed = value.trim();
  if (
    trimmed.length === 0
    || new TextEncoder().encode(trimmed).byteLength > 253
    || trimmed.endsWith("..")
  ) return invalidResponse(resource);
  const candidate = trimmed.endsWith(".") ? trimmed.slice(0, -1) : trimmed;
  if (
    candidate.length === 0
    || /[\\/:@?#\[\]%]/.test(candidate)
    || /\s/.test(candidate)
  ) return invalidResponse(resource);
  let hostname: string;
  try {
    const parsed = new URL(`https://${candidate}`);
    if (
      parsed.protocol !== "https:"
      || parsed.username !== ""
      || parsed.password !== ""
      || parsed.port !== ""
      || parsed.pathname !== "/"
      || parsed.search !== ""
      || parsed.hash !== ""
    ) throw new Error("invalid mail domain");
    hostname = parsed.hostname;
  } catch {
    return invalidResponse(resource);
  }
  if (
    hostname.length === 0
    || hostname.length > 253
    || isIpv4Address(hostname)
    || !hostname.includes(".")
    || hostname !== hostname.toLowerCase()
  ) return invalidResponse(resource);
  const labels = hostname.split(".");
  if (labels.some((label) => (
    label.length === 0
    || label.length > 63
    || label.startsWith("-")
    || label.endsWith("-")
    || !/^[a-z0-9-]+$/.test(label)
  ))) return invalidResponse(resource);
  return hostname;
}

function canonicalMailDomainName(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string {
  const value = boundedString(record, key, 253, resource);
  const canonical = canonicalizeMailDomainName(value, resource);
  if (canonical !== value) return invalidResponse(resource);
  return canonical;
}

function canonicalMailDomainCursor(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string {
  if (typeof value !== "string") return invalidResponse(resource);
  const canonical = canonicalizeMailDomainName(value, resource);
  if (canonical !== value) return invalidResponse(resource);
  return canonical;
}

function nullableMailDomainReference(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  return nullableBoundedString(record, key, 512, resource);
}

function nullableFailureCode(
  record: JsonRecord,
  key: string,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (record[key] === null) return null;
  const value = boundedString(record, key, 100, resource);
  if (!DNS_FAILURE_CODE.test(value)) return invalidResponse(resource);
  return value;
}

function mailDomainFailureCodes(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string[] {
  if (!Array.isArray(value) || value.length > MAX_MAIL_DOMAIN_FAILURE_CODES) {
    return invalidResponse(resource);
  }
  return value.map((code) => {
    if (
      typeof code !== "string"
      || code.length === 0
      || code.length > 100
      || code.trim() !== code
      || !DNS_FAILURE_CODE.test(code)
    ) return invalidResponse(resource);
    return code;
  });
}

function parseMailDomain(
  value: unknown,
  organizationId: string,
): MailDomain {
  const resource = "mail_domain" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "mail_domain_id",
    "organization_id",
    "name",
    "status",
    "route_mode",
    "ownership_challenge_version",
    "route_version",
    "inbound_enabled",
    "outbound_enabled",
    "smart_host_ref",
    "dkim_key_ref",
    "mail_engine_domain_ref",
    "mail_engine_projection_status",
    "mail_engine_projected_enabled",
    "mail_engine_projection_last_attempt_at",
    "mail_engine_projection_succeeded_at",
    "mail_engine_projection_error_code",
    "latest_dns_observed_at",
    "latest_dns_valid_until",
    "latest_dns_activation_ready",
    "latest_dns_failure_codes",
  ], resource);
  const status = record.status;
  const routeMode = record.route_mode;
  const projectionStatus = record.mail_engine_projection_status;
  if (
    !MAIL_DOMAIN_STATUSES.includes(status as MailDomainStatus)
    || !MAIL_DOMAIN_ROUTE_MODES.includes(routeMode as MailDomainRouteMode)
    || !MAIL_ENGINE_PROJECTION_STATUSES.includes(
      projectionStatus as MailEngineProjectionStatus,
    )
    || typeof record.inbound_enabled !== "boolean"
    || typeof record.outbound_enabled !== "boolean"
    || typeof record.mail_engine_projected_enabled !== "boolean"
      && record.mail_engine_projected_enabled !== null
    || typeof record.latest_dns_activation_ready !== "boolean"
  ) return invalidResponse(resource);
  const parsed: MailDomain = {
    mail_domain_id: canonicalUuid(record, "mail_domain_id", resource),
    organization_id: canonicalUuid(record, "organization_id", resource),
    name: canonicalMailDomainName(record, "name", resource),
    status: status as MailDomainStatus,
    route_mode: routeMode as MailDomainRouteMode,
    ownership_challenge_version: versionFromRecord(
      record,
      "ownership_challenge_version",
      resource,
    ),
    route_version: versionFromRecord(record, "route_version", resource),
    inbound_enabled: record.inbound_enabled,
    outbound_enabled: record.outbound_enabled,
    smart_host_ref: nullableMailDomainReference(record, "smart_host_ref", resource),
    dkim_key_ref: nullableMailDomainReference(record, "dkim_key_ref", resource),
    mail_engine_domain_ref: nullableMailDomainReference(record, "mail_engine_domain_ref", resource),
    mail_engine_projection_status: projectionStatus as MailEngineProjectionStatus,
    mail_engine_projected_enabled: record.mail_engine_projected_enabled,
    mail_engine_projection_last_attempt_at: nullableRfc3339Timestamp(
      record,
      "mail_engine_projection_last_attempt_at",
      resource,
    ),
    mail_engine_projection_succeeded_at: nullableRfc3339Timestamp(
      record,
      "mail_engine_projection_succeeded_at",
      resource,
    ),
    mail_engine_projection_error_code: nullableFailureCode(
      record,
      "mail_engine_projection_error_code",
      resource,
    ),
    latest_dns_observed_at: nullableRfc3339Timestamp(
      record,
      "latest_dns_observed_at",
      resource,
    ),
    latest_dns_valid_until: nullableRfc3339Timestamp(
      record,
      "latest_dns_valid_until",
      resource,
    ),
    latest_dns_activation_ready: record.latest_dns_activation_ready,
    latest_dns_failure_codes: mailDomainFailureCodes(
      record.latest_dns_failure_codes,
      resource,
    ),
  };
  const hasLatestDnsObservation = parsed.latest_dns_observed_at !== null;
  const projectionReady = parsed.mail_engine_projection_status === "ready";
  const projectionFailed = parsed.mail_engine_projection_status === "failed";
  if (
    parsed.organization_id !== organizationId
    || (parsed.route_mode === "direct_mx" && parsed.smart_host_ref !== null)
    || ((parsed.inbound_enabled || parsed.outbound_enabled) && parsed.status !== "active")
    || (parsed.outbound_enabled && parsed.dkim_key_ref === null)
    || (
      parsed.route_mode === "smart_host"
      && parsed.outbound_enabled
      && parsed.smart_host_ref === null
    )
    || (hasLatestDnsObservation !== (parsed.latest_dns_valid_until !== null))
    || (
      hasLatestDnsObservation
      && Date.parse(parsed.latest_dns_valid_until!) <= Date.parse(parsed.latest_dns_observed_at!)
    )
    || (!hasLatestDnsObservation && (
      parsed.latest_dns_activation_ready || parsed.latest_dns_failure_codes.length !== 0
    ))
    || (projectionReady && (
      parsed.mail_engine_domain_ref === null
      || parsed.mail_engine_projected_enabled === null
      || parsed.mail_engine_projection_succeeded_at === null
      || parsed.mail_engine_projection_error_code !== null
    ))
    || (projectionFailed && (
      parsed.mail_engine_projection_last_attempt_at === null
      || parsed.mail_engine_projection_error_code === null
    ))
  ) return invalidResponse(resource);
  return parsed;
}

function parseMailDomainPage(
  value: unknown,
  organizationId: string,
  input: { after?: string; limit?: number },
): MailDomainPage {
  const resource = "mail_domain_page" as const;
  const limit = input.limit ?? 50;
  if (
    !CANONICAL_UUID.test(organizationId)
    || !Number.isInteger(limit)
    || limit < 1
    || limit > MAX_MAIL_DOMAIN_PAGE_SIZE
  ) return invalidResponse(resource);
  const after = input.after === undefined
    ? undefined
    : canonicalMailDomainCursor(input.after, resource);
  const record = jsonRecord(value, resource);
  exactKeys(record, ["items", "next_after"], resource);
  if (!Array.isArray(record.items) || record.items.length > limit) {
    return invalidResponse(resource);
  }
  const items = record.items.map((item) => parseMailDomain(item, organizationId));
  if (
    new Set(items.map((item) => item.mail_domain_id)).size !== items.length
    || new Set(items.map((item) => item.name)).size !== items.length
  ) return invalidResponse(resource);
  for (const [index, item] of items.entries()) {
    const previous = items[index - 1];
    if (
      (after !== undefined && item.name <= after)
      || (previous !== undefined && item.name <= previous.name)
    ) return invalidResponse(resource);
  }
  const nextAfter = record.next_after === null
    ? null
    : canonicalMailDomainCursor(record.next_after, resource);
  if (
    nextAfter !== null
    && (
      items.length !== limit
      || items.length === 0
      || nextAfter !== items.at(-1)!.name
    )
  ) return invalidResponse(resource);
  return { items, next_after: nextAfter };
}

function parseMailDomainCreateResponse(
  value: unknown,
  organizationId: string,
  expectedName: string,
): MailDomainCreateResponse {
  const resource = "mail_domain_created" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "mail_domain_id",
    "organization_id",
    "name",
    "status",
    "ownership_challenge_version",
    "route_version",
    "ownership_record_name",
    "ownership_token",
  ], resource);
  if (record.status !== "pending_dns") return invalidResponse(resource);
  const parsed: MailDomainCreateResponse = {
    mail_domain_id: canonicalUuid(record, "mail_domain_id", resource),
    organization_id: canonicalUuid(record, "organization_id", resource),
    name: canonicalMailDomainName(record, "name", resource),
    status: "pending_dns",
    ownership_challenge_version: versionFromRecord(
      record,
      "ownership_challenge_version",
      resource,
    ),
    route_version: versionFromRecord(record, "route_version", resource),
    ownership_record_name: boundedString(record, "ownership_record_name", 512, resource),
    ownership_token: ownershipToken(record, "ownership_token", resource),
  };
  if (
    parsed.organization_id !== organizationId
    || parsed.name !== expectedName
    || parsed.ownership_challenge_version !== 1
    || parsed.route_version !== 1
    || parsed.ownership_record_name !== `_copilotmail-verification.${expectedName}`
  ) return invalidResponse(resource);
  return parsed;
}

function parseMailDomainChallengeRotationResponse(
  value: unknown,
  mailDomainId: string,
  expectedName: string,
  expectedVersion: number,
): MailDomainChallengeRotationResponse {
  const resource = "mail_domain_challenge" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "mail_domain_id",
    "status",
    "ownership_challenge_version",
    "ownership_record_name",
    "ownership_token",
  ], resource);
  if (record.status !== "pending_dns") return invalidResponse(resource);
  const parsed: MailDomainChallengeRotationResponse = {
    mail_domain_id: canonicalUuid(record, "mail_domain_id", resource),
    status: "pending_dns",
    ownership_challenge_version: versionFromRecord(
      record,
      "ownership_challenge_version",
      resource,
    ),
    ownership_record_name: boundedString(record, "ownership_record_name", 512, resource),
    ownership_token: ownershipToken(record, "ownership_token", resource),
  };
  if (
    parsed.mail_domain_id !== mailDomainId
    || parsed.ownership_challenge_version !== expectedVersion + 1
    || parsed.ownership_record_name !== `_copilotmail-verification.${expectedName}`
  ) return invalidResponse(resource);
  return parsed;
}

function parseMailDomainDnsVerificationResponse(
  value: unknown,
  mailDomainId: string,
): MailDomainDnsVerificationResponse {
  const resource = "mail_domain_dns_verification" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, [
    "mail_domain_id",
    "status",
    "activation_ready",
    "valid_until",
    "failure_codes",
  ], resource);
  const status = record.status;
  if (
    status !== "pending_dns"
    && status !== "verified"
    && status !== "active"
    || typeof record.activation_ready !== "boolean"
  ) return invalidResponse(resource);
  const parsed: MailDomainDnsVerificationResponse = {
    mail_domain_id: canonicalUuid(record, "mail_domain_id", resource),
    status,
    activation_ready: record.activation_ready,
    valid_until: rfc3339Timestamp(record, "valid_until", resource),
    failure_codes: mailDomainFailureCodes(record.failure_codes, resource),
  };
  if (
    parsed.mail_domain_id !== mailDomainId
    || (parsed.status === "verified" && !parsed.activation_ready)
  ) return invalidResponse(resource);
  return parsed;
}

function parseMailDomainActionResponse(
  value: unknown,
  mailDomainId: string,
  expectedStatus: MailDomainActionResponse["status"],
): MailDomainActionResponse {
  const resource = "mail_domain_action" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, ["mail_domain_id", "status"], resource);
  if (record.status !== expectedStatus) return invalidResponse(resource);
  const parsed: MailDomainActionResponse = {
    mail_domain_id: canonicalUuid(record, "mail_domain_id", resource),
    status: expectedStatus,
  };
  if (parsed.mail_domain_id !== mailDomainId) return invalidResponse(resource);
  return parsed;
}

function parseMailDomainRouteResponse(
  value: unknown,
  mailDomainId: string,
  input: Pick<MailDomainRouteUpdateInput, "expected_version" | "inbound_enabled" | "outbound_enabled">,
): MailDomainRouteResponse {
  const resource = "mail_domain_route" as const;
  const record = jsonRecord(value, resource);
  exactKeys(record, ["mail_domain_id", "version", "inbound_enabled", "outbound_enabled"], resource);
  if (
    typeof record.inbound_enabled !== "boolean"
    || typeof record.outbound_enabled !== "boolean"
  ) return invalidResponse(resource);
  const parsed: MailDomainRouteResponse = {
    mail_domain_id: canonicalUuid(record, "mail_domain_id", resource),
    version: versionFromRecord(record, "version", resource),
    inbound_enabled: record.inbound_enabled,
    outbound_enabled: record.outbound_enabled,
  };
  if (
    parsed.mail_domain_id !== mailDomainId
    || parsed.version !== input.expected_version + 1
    || parsed.inbound_enabled !== input.inbound_enabled
    || parsed.outbound_enabled !== input.outbound_enabled
  ) return invalidResponse(resource);
  return parsed;
}

/**
 * The Inspector is a privileged, bounded control-plane observation. Treat a
 * success body as untrusted until every documented field and derived state is
 * verified. This prevents a proxy, stale implementation, or accidental API
 * expansion from becoming a browser-visible Agent runtime/credential view.
 */
function parseAgentInspector(value: unknown, requestedAgentId: string): AgentInspector {
  const resource = "agent_inspector" as const;
  const record = jsonRecord(value, resource);
  exactKeyCount(record, 8, resource);

  const owner = jsonRecord(record.owner, resource);
  exactKeyCount(owner, 3, resource);
  const profile = jsonRecord(record.profile, resource);
  exactKeyCount(profile, 5, resource);
  const tokenAccess = jsonRecord(record.token_access, resource);
  exactKeyCount(tokenAccess, 5, resource);
  const controlPlane = jsonRecord(record.control_plane, resource);
  exactKeyCount(controlPlane, 5, resource);

  const effectiveScopes = tokenAccess.effective_scopes;
  if (
    !Array.isArray(effectiveScopes)
    || effectiveScopes.length > AGENT_INSPECTOR_SCOPES.length
  ) return invalidResponse(resource);
  const parsedScopes = effectiveScopes.map((scope) => {
    if (
      typeof scope !== "string"
      || !AGENT_INSPECTOR_SCOPES.includes(
        scope as (typeof AGENT_INSPECTOR_SCOPES)[number],
      )
    ) return invalidResponse(resource);
    return scope as AgentInspector["token_access"]["effective_scopes"][number];
  });
  if (new Set(parsedScopes).size !== parsedScopes.length) return invalidResponse(resource);

  const parsed: AgentInspector = {
    agent_id: nonNilCanonicalUuid(record, "agent_id", resource),
    display_name: inspectorDisplayName(record, "display_name", resource),
    principal_status: enumValue(record, "principal_status", PRINCIPAL_STATUSES, resource),
    owner: {
      principal_id: nonNilCanonicalUuid(owner, "principal_id", resource),
      display_name: inspectorDisplayName(owner, "display_name", resource),
      principal_status: enumValue(owner, "principal_status", PRINCIPAL_STATUSES, resource),
    },
    profile: {
      environment: enumValue(profile, "environment", AGENT_ENVIRONMENTS, resource),
      risk_level: enumValue(profile, "risk_level", AGENT_RISK_LEVELS, resource),
      purpose: inspectorPurpose(profile, "purpose", resource),
      review_due_at: rfc3339Timestamp(profile, "review_due_at", resource),
      review_state: enumValue(profile, "review_state", AGENT_REVIEW_STATES, resource),
    },
    token_access: {
      state: enumValue(tokenAccess, "state", AGENT_TOKEN_ACCESS_STATES, resource),
      effective_scopes: parsedScopes,
      active_credential_count: nonNegativeU32(tokenAccess, "active_credential_count", resource),
      next_credential_expiry_at: nullableRfc3339Timestamp(
        tokenAccess,
        "next_credential_expiry_at",
        resource,
      ),
      last_authenticated_at: nullableRfc3339Timestamp(
        tokenAccess,
        "last_authenticated_at",
        resource,
      ),
    },
    control_plane: {
      mail_admission_state: enumValue(
        controlPlane,
        "mail_admission_state",
        AGENT_MAIL_ADMISSION_STATES,
        resource,
      ),
      active_processing_subscription_count: nonNegativeU32(
        controlPlane,
        "active_processing_subscription_count",
        resource,
      ),
      paused_processing_subscription_count: nonNegativeU32(
        controlPlane,
        "paused_processing_subscription_count",
        resource,
      ),
      retired_processing_subscription_count: nonNegativeU32(
        controlPlane,
        "retired_processing_subscription_count",
        resource,
      ),
      last_processing_activity_at: nullableRfc3339Timestamp(
        controlPlane,
        "last_processing_activity_at",
        resource,
      ),
    },
    observed_at: rfc3339Timestamp(record, "observed_at", resource),
  };

  const observedAt = Date.parse(parsed.observed_at);
  const reviewDueAt = Date.parse(parsed.profile.review_due_at);
  const expectedReviewState: AgentInspectorReviewState = reviewDueAt <= observedAt
    ? "due"
    : "scheduled";
  const expectedTokenState: AgentInspectorTokenAccessState = parsed.principal_status !== "active"
    ? "identity_inactive"
    : parsed.token_access.active_credential_count === 0
      ? "no_active_credential"
      : parsed.token_access.effective_scopes.length === 0
        ? "no_effective_scope"
        : "effective";
  const credentialsAreActive = parsed.token_access.active_credential_count > 0;
  const expiryAt = parsed.token_access.next_credential_expiry_at === null
    ? undefined
    : Date.parse(parsed.token_access.next_credential_expiry_at);
  const lastAuthenticatedAt = parsed.token_access.last_authenticated_at === null
    ? undefined
    : Date.parse(parsed.token_access.last_authenticated_at);
  const processingActivityAt = parsed.control_plane.last_processing_activity_at === null
    ? undefined
    : Date.parse(parsed.control_plane.last_processing_activity_at);
  const productionAdmission = parsed.control_plane.mail_admission_state === "active"
    || parsed.control_plane.mail_admission_state === "blocked_no_active_policy";

  if (
    parsed.agent_id !== requestedAgentId
    || parsed.owner.principal_id === parsed.agent_id
    || parsed.profile.review_state !== expectedReviewState
    || parsed.token_access.state !== expectedTokenState
    || (parsed.token_access.next_credential_expiry_at !== null) !== credentialsAreActive
    || (expiryAt !== undefined && expiryAt <= observedAt)
    || (lastAuthenticatedAt !== undefined && lastAuthenticatedAt > observedAt)
    || (processingActivityAt !== undefined && processingActivityAt > observedAt)
    || (!credentialsAreActive && parsed.token_access.effective_scopes.length !== 0)
    || (parsed.principal_status !== "active" && parsed.token_access.effective_scopes.length !== 0)
    || (parsed.profile.environment === "production" && !productionAdmission)
    || (parsed.profile.environment !== "production"
      && parsed.control_plane.mail_admission_state !== "not_required")
  ) return invalidResponse(resource);

  return parsed;
}

/**
 * Verifies the deliberately narrow, Human-only evidence response for one
 * exact production-Agent submission. Unknown fields are rejected so a server
 * or proxy cannot turn this browser view into a recipient, SMTP, runtime,
 * execution, delegation, mailbox, or content disclosure by accident.
 */
function parseAgentMailSubmissionEvidence(
  value: unknown,
  requestedAgentId: string,
  requestedSubmissionId: string,
): AgentMailSubmissionEvidence {
  const resource = "agent_mail_submission_evidence" as const;
  const record = jsonRecord(value, resource);
  exactKeys(
    record,
    ["agent_id", "submission", "admission", "delivery", "audit_references", "observed_at"],
    resource,
  );

  const submission = jsonRecord(record.submission, resource);
  exactKeys(submission, ["submission_id", "status", "created_at", "updated_at"], resource);
  const admission = jsonRecord(record.admission, resource);
  exactKeys(
    admission,
    [
      "admission_id",
      "admitted_at",
      "policy",
      "recipient_count",
      "first_contact_count",
      "first_contact_approval_request_id",
    ],
    resource,
  );
  const policy = jsonRecord(admission.policy, resource);
  exactKeys(
    policy,
    ["activation_id", "activated_at", "version_id", "version", "approval_request_id"],
    resource,
  );

  const auditReferences = record.audit_references;
  if (!Array.isArray(auditReferences) || auditReferences.length < 2 || auditReferences.length > 3) {
    return invalidResponse(resource);
  }

  const parsedAuditReferences = auditReferences.map((reference) => {
    const item = jsonRecord(reference, resource);
    exactKeys(item, ["event_id", "action", "occurred_at"], resource);
    return {
      event_id: nonNilCanonicalUuid(item, "event_id", resource),
      action: enumValue(item, "action", AGENT_MAIL_SUBMISSION_AUDIT_ACTIONS, resource),
      occurred_at: rfc3339Timestamp(item, "occurred_at", resource),
    };
  });

  let delivery: AgentMailSubmissionEvidence["delivery"] = null;
  if (record.delivery !== null) {
    const deliveryRecord = jsonRecord(record.delivery, resource);
    exactKeys(
      deliveryRecord,
      [
        "observed_at",
        "terminal",
        "recipient_count",
        "delivered_count",
        "failed_count",
        "queued_count",
        "unknown_count",
      ],
      resource,
    );
    if (typeof deliveryRecord.terminal !== "boolean") return invalidResponse(resource);
    delivery = {
      observed_at: rfc3339Timestamp(deliveryRecord, "observed_at", resource),
      terminal: deliveryRecord.terminal,
      recipient_count: nonNegativeU32(deliveryRecord, "recipient_count", resource),
      delivered_count: nonNegativeU32(deliveryRecord, "delivered_count", resource),
      failed_count: nonNegativeU32(deliveryRecord, "failed_count", resource),
      queued_count: nonNegativeU32(deliveryRecord, "queued_count", resource),
      unknown_count: nonNegativeU32(deliveryRecord, "unknown_count", resource),
    };
  }

  const parsed: AgentMailSubmissionEvidence = {
    agent_id: nonNilCanonicalUuid(record, "agent_id", resource),
    submission: {
      submission_id: nonNilCanonicalUuid(submission, "submission_id", resource),
      status: enumValue(submission, "status", AGENT_MAIL_SUBMISSION_STATUSES, resource),
      created_at: rfc3339Timestamp(submission, "created_at", resource),
      updated_at: rfc3339Timestamp(submission, "updated_at", resource),
    },
    admission: {
      admission_id: nonNilCanonicalUuid(admission, "admission_id", resource),
      admitted_at: rfc3339Timestamp(admission, "admitted_at", resource),
      policy: {
        activation_id: nonNilCanonicalUuid(policy, "activation_id", resource),
        activated_at: rfc3339Timestamp(policy, "activated_at", resource),
        version_id: nonNilCanonicalUuid(policy, "version_id", resource),
        version: nonNegativeU32(policy, "version", resource),
        approval_request_id: nonNilCanonicalUuid(policy, "approval_request_id", resource),
      },
      recipient_count: nonNegativeU32(admission, "recipient_count", resource),
      first_contact_count: nonNegativeU32(admission, "first_contact_count", resource),
      first_contact_approval_request_id: nullableNonNilCanonicalUuid(
        admission,
        "first_contact_approval_request_id",
        resource,
      ),
    },
    delivery,
    audit_references: parsedAuditReferences,
    observed_at: rfc3339Timestamp(record, "observed_at", resource),
  };

  const observedAt = rfc3339EpochNanoseconds(parsed.observed_at, resource);
  const submissionCreatedAt = rfc3339EpochNanoseconds(parsed.submission.created_at, resource);
  const submissionUpdatedAt = rfc3339EpochNanoseconds(parsed.submission.updated_at, resource);
  const admittedAt = rfc3339EpochNanoseconds(parsed.admission.admitted_at, resource);
  const policyActivatedAt = rfc3339EpochNanoseconds(
    parsed.admission.policy.activated_at,
    resource,
  );
  const auditOccurredAt = parsed.audit_references.map((reference) => (
    rfc3339EpochNanoseconds(reference.occurred_at, resource)
  ));
  const expectedTerminalAction = parsed.submission.status === "accepted"
    ? "mail.submission.accepted"
    : parsed.submission.status === "rejected"
      ? "mail.submission.rejected"
      : parsed.submission.status === "manual_review"
        ? "mail.submission.review_required"
        : undefined;
  const expectedAuditActions = expectedTerminalAction === undefined
    ? ["mail.submission.requested", "agent.mail.admitted"]
    : ["mail.submission.requested", "agent.mail.admitted", expectedTerminalAction];
  const terminalAuditAt = expectedTerminalAction === undefined
    ? null
    : auditOccurredAt[2] ?? null;

  if (
    parsed.agent_id !== requestedAgentId
    || parsed.submission.submission_id !== requestedSubmissionId
    || parsed.submission.status !== "accepted" && parsed.delivery !== null
    || submissionCreatedAt > submissionUpdatedAt
    || submissionUpdatedAt > observedAt
    || policyActivatedAt > admittedAt
    || admittedAt < submissionCreatedAt
    || admittedAt > observedAt
    || policyActivatedAt > observedAt
    || parsed.admission.policy.version === 0
    || parsed.admission.recipient_count < 1
    || parsed.admission.recipient_count > 100
    || parsed.admission.first_contact_count > parsed.admission.recipient_count
    || parsed.admission.first_contact_approval_request_id !== null
      && parsed.admission.first_contact_count === 0
    || parsed.admission.first_contact_approval_request_id === null
      && parsed.admission.first_contact_count > 0
    || parsed.audit_references.length !== expectedAuditActions.length
    || auditOccurredAt[0] !== submissionCreatedAt
    || auditOccurredAt[1] !== admittedAt
    || terminalAuditAt !== null && (
      terminalAuditAt < admittedAt || terminalAuditAt > submissionUpdatedAt
    )
    || parsed.audit_references.some((reference, index) => (
      reference.action !== expectedAuditActions[index]
      || auditOccurredAt[index] === undefined
      || auditOccurredAt[index] > observedAt
      || parsed.audit_references.some((other, otherIndex) => (
        otherIndex > index && other.event_id === reference.event_id
      ))
    ))
  ) return invalidResponse(resource);

  if (parsed.delivery !== null) {
    const deliveryObservedAt = rfc3339EpochNanoseconds(parsed.delivery.observed_at, resource);
    const deliveryCount = parsed.delivery.delivered_count
      + parsed.delivery.failed_count
      + parsed.delivery.queued_count
      + parsed.delivery.unknown_count;
    if (
      parsed.delivery.recipient_count < 1
      || parsed.delivery.recipient_count > 100
      || parsed.delivery.recipient_count !== parsed.admission.recipient_count
      || parsed.delivery.delivered_count > 100
      || parsed.delivery.failed_count > 100
      || parsed.delivery.queued_count > 100
      || parsed.delivery.unknown_count > 100
      || deliveryCount !== parsed.delivery.recipient_count
      || parsed.delivery.terminal !== (
        parsed.delivery.queued_count === 0 && parsed.delivery.unknown_count === 0
      )
      || terminalAuditAt === null
      || deliveryObservedAt < terminalAuditAt
      || deliveryObservedAt > observedAt
    ) return invalidResponse(resource);
  }

  return parsed;
}

function normalizeMailDomainReference(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string | null {
  if (value === null) return null;
  if (typeof value !== "string" || /[\u0000-\u001f\u007f]/.test(value)) {
    return invalidResponse(resource);
  }
  const normalized = value.trim();
  if (normalized.length === 0 || normalized.length > 512) return invalidResponse(resource);
  return normalized;
}

function validDkimSelector(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string {
  if (typeof value !== "string" || !DKIM_SELECTOR.test(value)) {
    return invalidResponse(resource);
  }
  return value;
}

function validReasonCode(
  value: unknown,
  resource: InvalidApiResponseError["resource"],
): string {
  if (
    typeof value !== "string"
    || value.length === 0
    || value.length > 100
    || !DNS_FAILURE_CODE.test(value)
  ) return invalidResponse(resource);
  return value;
}

async function sha256Hex(blob: Blob): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function isUnknownWriteFailure(error: unknown): boolean {
  return error instanceof InvalidApiResponseError
    || !(error instanceof ApiError)
    || error.status >= 500;
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
    private readonly onUnauthorized?: () => void,
  ) {
    this.baseUrl = sameOriginApiBaseUrl(baseUrl);
  }

  /** Lists a tenant's domains strictly after an exact canonical-name cursor. */
  async listMailDomains(
    input: { after?: string; limit?: number } = {},
  ): Promise<MailDomainPage> {
    this.ensureMailDomainOrganization();
    const limit = input.limit ?? 50;
    if (!Number.isInteger(limit) || limit < 1 || limit > MAX_MAIL_DOMAIN_PAGE_SIZE) {
      return invalidResponse("mail_domain_page");
    }
    const after = input.after === undefined
      ? undefined
      : canonicalMailDomainCursor(input.after, "mail_domain_page");
    const response = await this.request<unknown>(
      `/api/v2/organizations/${this.organizationId}/mail-domains${buildQuery({ after, limit })}`,
    );
    return parseMailDomainPage(response, this.organizationId, { after, limit });
  }

  /** Reads one domain only after validating that its path ID is canonical. */
  async getMailDomain(mailDomainId: string): Promise<MailDomain> {
    this.ensureMailDomainOrganization();
    this.ensureMailDomainId(mailDomainId, "mail_domain");
    const result = parseMailDomain(
      await this.request<unknown>(`/api/v2/mail-domains/${mailDomainId}`),
      this.organizationId,
    );
    if (result.mail_domain_id !== mailDomainId) return invalidResponse("mail_domain");
    return result;
  }

  /**
   * Registers a domain and returns its token exactly once. The client keeps no
   * token field and performs a token-free read-back before disclosing it to the
   * caller, so it cannot bleed into durable read-model state.
   */
  async createMailDomain(
    input: MailDomainCreateInput,
  ): Promise<MailDomainCreateResponse> {
    this.ensureMailDomainOrganization();
    const name = canonicalizeMailDomainName(input.name, "mail_domain_created");
    if (!MAIL_DOMAIN_ROUTE_MODES.includes(input.route_mode)) {
      return invalidResponse("mail_domain_created");
    }
    const body: MailDomainCreateInput = {
      name,
      route_mode: input.route_mode,
    };
    const disclosure = parseMailDomainCreateResponse(
      await this.request<unknown>(
        `/api/v2/organizations/${this.organizationId}/mail-domains`,
        { method: "POST", body: JSON.stringify(body) },
      ),
      this.organizationId,
      name,
    );
    const reflected = await this.getMailDomain(disclosure.mail_domain_id);
    if (
      reflected.name !== body.name
      || reflected.status !== disclosure.status
      || reflected.route_mode !== body.route_mode
      || reflected.ownership_challenge_version !== disclosure.ownership_challenge_version
      || reflected.route_version !== disclosure.route_version
      || reflected.inbound_enabled
      || reflected.outbound_enabled
    ) return invalidResponse("mail_domain_created");
    return disclosure;
  }

  /** Replaces a challenge using its current version and returns a new token once. */
  async rotateMailDomainOwnershipChallenge(
    mailDomainId: string,
    expectedVersionInput: number,
  ): Promise<MailDomainChallengeRotationResponse> {
    this.ensureMailDomainOrganization();
    this.ensureMailDomainId(mailDomainId, "mail_domain_challenge");
    const expectedVersion = safePositiveVersion(
      expectedVersionInput,
      "mail_domain_challenge",
    );
    if (expectedVersion >= MAX_SAFE_VERSION) return invalidResponse("mail_domain_challenge");
    const before = await this.getMailDomain(mailDomainId);
    if (before.ownership_challenge_version !== expectedVersion) {
      return invalidResponse("mail_domain_challenge");
    }
    const body: MailDomainChallengeRotationInput = { expected_version: expectedVersion };
    const disclosure = parseMailDomainChallengeRotationResponse(
      await this.request<unknown>(
        `/api/v2/mail-domains/${mailDomainId}/ownership-challenge`,
        { method: "POST", body: JSON.stringify(body) },
      ),
      mailDomainId,
      before.name,
      expectedVersion,
    );
    const reflected = await this.getMailDomain(mailDomainId);
    if (
      reflected.status !== disclosure.status
      || reflected.ownership_challenge_version !== disclosure.ownership_challenge_version
      || reflected.name !== before.name
    ) return invalidResponse("mail_domain_challenge");
    return disclosure;
  }

  /**
   * Records DNS evidence for the exact canonical domain addressed by the path.
   * The ownership token is copied only into this whitelisted network body and
   * is never retained by the client after the awaited request resolves.
   */
  async verifyMailDomainDns(
    mailDomainId: string,
    input: MailDomainDnsVerificationInput,
  ): Promise<MailDomainDnsVerificationResponse> {
    this.ensureMailDomainOrganization();
    this.ensureMailDomainId(mailDomainId, "mail_domain_dns_verification");
    const before = await this.getMailDomain(mailDomainId);
    const domain = canonicalizeMailDomainName(input.domain, "mail_domain_dns_verification");
    const ownershipTokenValue = validOwnershipToken(
      input.ownership_token,
      "mail_domain_dns_verification",
    );
    const dkimSelector = validDkimSelector(input.dkim_selector, "mail_domain_dns_verification");
    if (domain !== before.name) return invalidResponse("mail_domain_dns_verification");
    const body: MailDomainDnsVerificationInput = {
      domain,
      ownership_token: ownershipTokenValue,
      dkim_selector: dkimSelector,
    };
    const result = parseMailDomainDnsVerificationResponse(
      await this.request<unknown>(
        `/api/v2/mail-domains/${mailDomainId}/dns-verifications`,
        { method: "POST", body: JSON.stringify(body) },
      ),
      mailDomainId,
    );
    const reflected = await this.getMailDomain(mailDomainId);
    if (
      reflected.status !== result.status
      || reflected.latest_dns_valid_until === null
      || Date.parse(reflected.latest_dns_valid_until) !== Date.parse(result.valid_until)
      || reflected.latest_dns_activation_ready !== result.activation_ready
      || reflected.latest_dns_failure_codes.length !== result.failure_codes.length
      || reflected.latest_dns_failure_codes.some((value, index) => value !== result.failure_codes[index])
    ) return invalidResponse("mail_domain_dns_verification");
    return result;
  }

  /** Activates only the exact verified domain selected by its canonical path ID. */
  async activateMailDomain(mailDomainId: string): Promise<MailDomainActionResponse> {
    this.ensureMailDomainOrganization();
    this.ensureMailDomainId(mailDomainId, "mail_domain_action");
    const before = await this.getMailDomain(mailDomainId);
    if (before.status !== "verified" || !before.latest_dns_activation_ready) {
      return invalidResponse("mail_domain_action");
    }
    const result = parseMailDomainActionResponse(
      await this.request<unknown>(`/api/v2/mail-domains/${mailDomainId}/activation`, {
        method: "POST",
      }),
      mailDomainId,
      "active",
    );
    const reflected = await this.getMailDomain(mailDomainId);
    if (
      reflected.status !== result.status
      || reflected.ownership_challenge_version !== before.ownership_challenge_version
    ) return invalidResponse("mail_domain_action");
    return result;
  }

  /** Suspends an exact active domain and verifies the mandatory route cut-off. */
  async suspendMailDomain(
    mailDomainId: string,
    input: MailDomainSuspendInput,
  ): Promise<MailDomainActionResponse> {
    this.ensureMailDomainOrganization();
    this.ensureMailDomainId(mailDomainId, "mail_domain_action");
    const reasonCode = validReasonCode(input.reason_code, "mail_domain_action");
    const before = await this.getMailDomain(mailDomainId);
    if (before.status !== "active" || before.route_version >= MAX_SAFE_VERSION) {
      return invalidResponse("mail_domain_action");
    }
    const body: MailDomainSuspendInput = { reason_code: reasonCode };
    const result = parseMailDomainActionResponse(
      await this.request<unknown>(`/api/v2/mail-domains/${mailDomainId}/suspension`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
      mailDomainId,
      "suspended",
    );
    const reflected = await this.getMailDomain(mailDomainId);
    if (
      reflected.status !== result.status
      || reflected.route_version !== before.route_version + 1
      || reflected.inbound_enabled
      || reflected.outbound_enabled
      || reflected.smart_host_ref !== before.smart_host_ref
      || reflected.dkim_key_ref !== before.dkim_key_ref
    ) return invalidResponse("mail_domain_action");
    return result;
  }

  /** Performs an optimistic, read-back-bound route CAS update. */
  async updateMailDomainRoute(
    mailDomainId: string,
    input: MailDomainRouteUpdateInput,
  ): Promise<MailDomainRouteResponse> {
    this.ensureMailDomainOrganization();
    this.ensureMailDomainId(mailDomainId, "mail_domain_route");
    const expectedVersion = safePositiveVersion(input.expected_version, "mail_domain_route");
    if (
      expectedVersion >= MAX_SAFE_VERSION
      || typeof input.inbound_enabled !== "boolean"
      || typeof input.outbound_enabled !== "boolean"
    ) return invalidResponse("mail_domain_route");
    const smartHostRef = normalizeMailDomainReference(input.smart_host_ref, "mail_domain_route");
    const dkimKeyRef = normalizeMailDomainReference(input.dkim_key_ref, "mail_domain_route");
    const before = await this.getMailDomain(mailDomainId);
    if (
      before.route_version !== expectedVersion
      || ((input.inbound_enabled || input.outbound_enabled) && before.status !== "active")
      || (input.outbound_enabled && dkimKeyRef === null)
      || (
        before.route_mode === "smart_host"
        && input.outbound_enabled
        && smartHostRef === null
      )
      || (before.route_mode === "direct_mx" && smartHostRef !== null)
    ) return invalidResponse("mail_domain_route");
    const body: MailDomainRouteUpdateInput = {
      expected_version: expectedVersion,
      inbound_enabled: input.inbound_enabled,
      outbound_enabled: input.outbound_enabled,
      smart_host_ref: smartHostRef,
      dkim_key_ref: dkimKeyRef,
    };
    const result = parseMailDomainRouteResponse(
      await this.request<unknown>(`/api/v2/mail-domains/${mailDomainId}/route`, {
        method: "PUT",
        body: JSON.stringify(body),
      }),
      mailDomainId,
      body,
    );
    const reflected = await this.getMailDomain(mailDomainId);
    if (
      reflected.route_version !== result.version
      || reflected.inbound_enabled !== body.inbound_enabled
      || reflected.outbound_enabled !== body.outbound_enabled
      || reflected.smart_host_ref !== body.smart_host_ref
      || reflected.dkim_key_ref !== body.dkim_key_ref
    ) return invalidResponse("mail_domain_route");
    return result;
  }

  async listCases(input: {
    status?: CaseStatus;
    cursor?: string;
    limit?: number;
  }): Promise<Page<ComplianceCase>> {
    const maximumItems = input.limit ?? 50;
    const response = await this.request<unknown>(
      `/api/v2/organizations/${this.organizationId}/compliance-cases${buildQuery(input)}`,
    );
    const page = parsePage(
      response,
      "compliance_case_page",
      (item) => parseComplianceCase(item, this.organizationId),
      (item) => item.case_id,
      maximumItems,
    );
    if (input.status && page.items.some((item) => item.status !== input.status)) {
      return invalidResponse("compliance_case_page");
    }
    return page;
  }

  async listApprovalRequests(input: {
    view: ApprovalView;
    status?: ApprovalStatus;
    cursor?: string;
    limit?: number;
  }): Promise<Page<ApprovalRequest>> {
    const limit = input.limit ?? 50;
    if (
      !CANONICAL_UUID.test(this.organizationId)
      || !Number.isInteger(limit)
      || limit < 1
      || limit > 100
      || (input.view === "actionable" && input.status !== undefined && input.status !== "pending")
    ) return invalidResponse("approval_request_page");
    let anchor: ApprovalRequest | undefined;
    if (input.cursor !== undefined) {
      const anchorId = approvalCursorRequestId(
        input.cursor,
        this.organizationId,
        input.view,
        input.status,
        "approval_request_page",
      );
      anchor = (await this.getApprovalRequest(anchorId)).request;
    }
    const response = await this.request<unknown>(
      `/api/v2/organizations/${this.organizationId}/approval-requests${buildQuery(input)}`,
    );
    return parseApprovalRequestPage(response, this.organizationId, input, anchor);
  }

  async createApprovalRequest(
    input: ApprovalRequestCreateInput,
  ): Promise<ApprovalRequestCreated> {
    // Keep this boundary explicit: the Rust request contract rejects unknown
    // fields, and callers must not be able to leak view-only state into it.
    const body: ApprovalRequestCreateInput = {
      policy_key: input.policy_key,
      action: input.action,
      resource_kind: input.resource_kind,
      resource_id: input.resource_id,
      reason: input.reason,
      ticket_reference: input.ticket_reference,
      expires_at: input.expires_at,
    };
    const response = await this.request<unknown>("/api/v2/approval-requests", {
      method: "POST",
      body: JSON.stringify(body),
    });
    const created = parseApprovalRequestCreated(response);
    const detail = await this.getApprovalRequest(created.approval_request_id);
    const request = detail.request;
    if (
      request.status !== "pending"
      || request.action !== body.action
      || request.resource_kind !== body.resource_kind
      || request.resource_id !== body.resource_id
      || request.reason !== body.reason
      || request.ticket_reference !== body.ticket_reference
      || Date.parse(request.expires_at) !== Date.parse(body.expires_at)
      || request.approval_count !== 0
      || request.rejection_count !== 0
      || detail.steps.length !== 0
    ) return invalidResponse("approval_request_created");
    return created;
  }

  async getApprovalRequest(requestId: string): Promise<ApprovalRequestDetail> {
    if (!CANONICAL_UUID.test(requestId) || !CANONICAL_UUID.test(this.organizationId)) {
      return invalidResponse("approval_request_detail");
    }
    const response = await this.request<unknown>(`/api/v2/approval-requests/${requestId}`);
    return parseApprovalRequestDetail(response, this.organizationId, requestId);
  }

  async decideApprovalRequest(
    requestId: string,
    decision: "approved" | "rejected",
    reason: string,
    expectation: ApprovalDecisionExpectation,
  ): Promise<ApprovalDecisionResult> {
    if (
      !CANONICAL_UUID.test(requestId)
      || !validApprovalDecisionExpectation(expectation)
    ) return invalidResponse("approval_decision");
    const response = await this.request<unknown>(
      `/api/v2/approval-requests/${requestId}/decisions`,
      {
        method: "POST",
        body: JSON.stringify({ decision, reason }),
      },
    );
    return parseApprovalDecision(response, requestId, decision, expectation);
  }

  async cancelApprovalRequest(
    requestId: string,
  ): Promise<ApprovalDecisionResult> {
    if (!CANONICAL_UUID.test(requestId)) return invalidResponse("approval_decision");
    const response = await this.request<unknown>(
      `/api/v2/approval-requests/${requestId}/cancellation`,
      {
        method: "POST",
        body: JSON.stringify({}),
      },
    );
    return parseApprovalDecision(response, requestId, "cancelled");
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

  async searchAuditEvents(
    filters: AuditEventFilters,
    input: { cursor?: string; limit?: number } = {},
  ): Promise<AuditEventSearchPage> {
    const response = await this.request<unknown>(
      `/api/v2/organizations/${this.organizationId}/audit-events${buildQuery({
        ...filters,
        ...input,
      })}`,
    );
    return parseAuditEventPage(
      response,
      this.organizationId,
      filters,
      input,
    );
  }

  async authorizeAuditExport(
    archiveId: string,
    approvalRequestId: string,
  ): Promise<AuditExportGrant> {
    if (
      !CANONICAL_UUID.test(this.organizationId)
      || !CANONICAL_UUID.test(archiveId)
      || !CANONICAL_UUID.test(approvalRequestId)
    ) return invalidResponse("audit_export_grant");
    const result = parseAuditExportGrant(await this.request<unknown>(`/api/v2/audit-archives/${archiveId}/exports`, {
      method: "POST",
      body: JSON.stringify({ approval_request_id: approvalRequestId }),
    }), this.organizationId);
    if (
      result.archive_id !== archiveId
      || result.approval_request_id !== approvalRequestId
    ) return invalidResponse("audit_export_grant");
    return result;
  }

  async getAuditExportGrant(grantId: string): Promise<AuditExportGrant> {
    if (
      !CANONICAL_UUID.test(this.organizationId)
      || !CANONICAL_UUID.test(grantId)
    ) return invalidResponse("audit_export_grant");
    const result = parseAuditExportGrant(
      await this.request<unknown>(`/api/v2/audit-export-grants/${grantId}`),
      this.organizationId,
    );
    if (result.audit_export_grant_id !== grantId) {
      return invalidResponse("audit_export_grant");
    }
    return result;
  }

  /**
   * Reconciles one unresolved authorization command by its exact immutable
   * relationship. This is intentionally a read-only lookup: callers use it
   * only after a POST outcome became unknown and must never replay that POST.
   */
  async getAuditExportGrantByApproval(
    archiveId: string,
    approvalRequestId: string,
  ): Promise<AuditExportGrant> {
    if (
      !CANONICAL_UUID.test(this.organizationId)
      || !CANONICAL_UUID.test(archiveId)
      || !CANONICAL_UUID.test(approvalRequestId)
    ) return invalidResponse("audit_export_grant");
    const result = parseAuditExportGrant(
      await this.request<unknown>(
        `/api/v2/audit-archives/${archiveId}/export-grants/${approvalRequestId}`,
        { cache: "no-store" },
      ),
      this.organizationId,
    );
    if (
      result.archive_id !== archiveId
      || result.approval_request_id !== approvalRequestId
    ) return invalidResponse("audit_export_grant");
    return result;
  }

  /**
   * Reads one explicitly requested control-plane snapshot. This endpoint is
   * privileged and auditable, so it is never prefetched or cached by the UI.
   */
  async getAgentInspector(agentId: string): Promise<AgentInspector> {
    if (!CANONICAL_UUID.test(agentId) || agentId === NIL_UUID) {
      return invalidResponse("agent_inspector");
    }
    return parseAgentInspector(
      await this.request<unknown>(
        `/api/v2/agents/${encodeURIComponent(agentId)}/inspector`,
        { cache: "no-store" },
      ),
      agentId,
    );
  }

  /**
   * Reads one Human-requested, auditable evidence chain for an exact Agent and
   * submission pair. The POST is intentional: a successful observation
   * atomically appends immutable audit evidence, so browser-session requests
   * must pass the control plane's existing Origin/CSRF guard. It is neither
   * prefetched nor cached.
   */
  async getAgentMailSubmissionEvidence(
    agentId: string,
    submissionId: string,
  ): Promise<AgentMailSubmissionEvidence> {
    if (
      !CANONICAL_UUID.test(agentId)
      || agentId === NIL_UUID
      || !CANONICAL_UUID.test(submissionId)
      || submissionId === NIL_UUID
    ) return invalidResponse("agent_mail_submission_evidence");
    return parseAgentMailSubmissionEvidence(
      await this.request<unknown>(
        `/api/v2/agents/${encodeURIComponent(agentId)}/mail-submissions/${encodeURIComponent(submissionId)}/evidence`,
        { method: "POST", cache: "no-store" },
      ),
      agentId,
      submissionId,
    );
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

  async getCase(caseId: string): Promise<ComplianceCase> {
    const result = parseComplianceCase(
      await this.request<unknown>(`/api/v2/compliance-cases/${caseId}`),
      this.organizationId,
    );
    if (result.case_id !== caseId) return invalidResponse("compliance_case");
    return result;
  }

  async createCase(input: {
    external_reference: string;
    title: string;
    purpose: string;
  }): Promise<ComplianceCase> {
    const result = parseComplianceCase(await this.request<unknown>(
      `/api/v2/organizations/${this.organizationId}/compliance-cases`,
      { method: "POST", body: JSON.stringify(input) },
    ), this.organizationId);
    if (
      result.status !== "open"
      || result.external_reference !== input.external_reference
      || result.title !== input.title
      || result.purpose !== input.purpose
    ) return invalidResponse("compliance_case");
    return result;
  }

  async closeCase(caseId: string): Promise<ComplianceCase> {
    const result = parseComplianceCase(await this.request<unknown>(`/api/v2/compliance-cases/${caseId}/closure`, {
      method: "POST",
    }), this.organizationId);
    if (result.case_id !== caseId || result.status !== "closed") {
      return invalidResponse("compliance_case");
    }
    return result;
  }

  async listHolds(input: {
    status?: HoldStatus;
    case_id?: string;
    cursor?: string;
    limit?: number;
  }): Promise<Page<LegalHold>> {
    const maximumItems = input.limit ?? 50;
    const response = await this.request<unknown>(
      `/api/v2/organizations/${this.organizationId}/legal-holds${buildQuery(input)}`,
    );
    const page = parsePage(
      response,
      "legal_hold_page",
      (item) => parseLegalHold(item, this.organizationId),
      (item) => item.legal_hold_id,
      maximumItems,
    );
    if (
      (input.status && page.items.some((item) => item.status !== input.status))
      || (input.case_id && page.items.some((item) => item.case_id !== input.case_id))
    ) return invalidResponse("legal_hold_page");
    return page;
  }

  async getHold(holdId: string): Promise<LegalHold> {
    const result = parseLegalHold(
      await this.request<unknown>(`/api/v2/legal-holds/${holdId}`),
      this.organizationId,
    );
    if (result.legal_hold_id !== holdId) return invalidResponse("legal_hold");
    return result;
  }

  async applyHold(
    archiveId: string,
    input: {
      case_id: string;
      approval_request_id: string;
      reason: string;
    },
  ): Promise<LegalHold> {
    const result = parseLegalHold(await this.request<unknown>(`/api/v2/audit-archives/${archiveId}/legal-holds`, {
      method: "POST",
      body: JSON.stringify(input),
    }), this.organizationId);
    if (
      result.status !== "active"
      || result.archive_id !== archiveId
      || result.case_id !== input.case_id
      || result.apply_approval_request_id !== input.approval_request_id
      || result.reason !== input.reason
    ) return invalidResponse("legal_hold");
    return result;
  }

  async releaseHold(
    holdId: string,
    approvalRequestId: string,
  ): Promise<LegalHold> {
    const result = parseLegalHold(await this.request<unknown>(`/api/v2/legal-holds/${holdId}/release`, {
      method: "POST",
      body: JSON.stringify({ approval_request_id: approvalRequestId }),
    }), this.organizationId);
    if (
      result.legal_hold_id !== holdId
      || result.status !== "released"
      || result.release_approval_request_id !== approvalRequestId
    ) return invalidResponse("legal_hold");
    return result;
  }

  /**
   * Retrieves one verified archive artifact. The control plane already
   * reverified the object against the recorded digest. This client also
   * verifies the exact size and SHA-256 before exposing any bytes to the UI.
   */
  async downloadAuditExportArtifact(
    grantId: string,
    artifact: AuditExportArtifact,
  ): Promise<{ blob: Blob; sha256: string; fileName: string }> {
    if (
      !CANONICAL_UUID.test(grantId)
      || (artifact !== "events" && artifact !== "manifest")
    ) return invalidResponse("audit_export_artifact");
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
    if (response.status === 401) this.onUnauthorized?.();
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
    const expectedFileName = `${grantId}-${artifact}.${artifact === "events" ? "jsonl" : "json"}`;
    const expectedDisposition = `attachment; filename="${expectedFileName}"`;
    const contentLength = response.headers.get("content-length");
    const sha256 = response.headers.get("x-copilotmail-artifact-sha256");
    if (
      response.headers.get("content-type") !== "application/octet-stream"
      || response.headers.get("cache-control") !== "no-store"
      || response.headers.get("content-disposition") !== expectedDisposition
      || contentLength === null
      || !/^[1-9][0-9]*$/.test(contentLength)
      || Number(contentLength) > MAX_AUDIT_EXPORT_ARTIFACT_BYTES
      || sha256 === null
      || !LOWER_HEX_SHA256.test(sha256)
    ) return invalidResponse("audit_export_artifact");
    const blob = await response.blob();
    if (
      blob.size !== Number(contentLength)
      || await sha256Hex(blob) !== sha256
    ) return invalidResponse("audit_export_artifact");
    return {
      blob,
      sha256,
      fileName: expectedFileName,
    };
  }

  private ensureMailDomainOrganization(): void {
    if (!CANONICAL_UUID.test(this.organizationId)) return invalidResponse("mail_domain");
  }

  private ensureMailDomainId(
    mailDomainId: string,
    resource: Extract<
      InvalidApiResponseError["resource"],
      | "mail_domain"
      | "mail_domain_action"
      | "mail_domain_challenge"
      | "mail_domain_dns_verification"
      | "mail_domain_route"
    >,
  ): void {
    if (!CANONICAL_UUID.test(mailDomainId)) return invalidResponse(resource);
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
    if (response.status === 401) this.onUnauthorized?.();
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
