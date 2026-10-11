import { expect, test as base, type Page } from "@playwright/test";

/**
 * These fixtures are intentionally independent of the future Playwright
 * configuration. That configuration must serve the built admin application at
 * `baseURL` (not Vite source mode), use `locale: "en-US"` and
 * `timezoneId: "UTC"` for stable visual baselines, and install the browser.
 *
 * Every API response below is local to the browser route layer. No test in
 * this directory may call a real control plane, identity provider, database,
 * or object store.
 */

export const E2E_ORGANIZATION_ID = "019b0000-0000-7000-8000-000000000001";
export const E2E_ACTIVE_HUMAN_ID = "019b0000-0000-7000-8000-000000000002";
export const E2E_AGENT_ID = "019b0000-0000-7000-8000-000000000101";
export const E2E_AGENT_OWNER_ID = "019b0000-0000-7000-8000-000000000102";
export const E2E_AGENT_MAIL_SUBMISSION_ID =
  "019b0000-0000-7000-8000-000000000109";
export const E2E_AGENT_DISPLAY_NAME = "E2E control-plane agent";
export const E2E_AGENT_OWNER_DISPLAY_NAME = "E2E Human owner";
export const E2E_CASE_ID = "019b0000-0000-7000-8000-000000000201";
export const E2E_HOLD_ID = "019b0000-0000-7000-8000-000000000301";
export const E2E_ARCHIVE_ID = "019b0000-0000-7000-8000-000000000401";
export const E2E_APPROVAL_ID = "019b0000-0000-7000-8000-000000000501";
export const E2E_AUDIT_EVENT_ID = "019b0000-0000-7000-8000-000000000601";
export const E2E_AUDIT_QUERY_ID = "019b0000-0000-7000-8000-000000000604";
export const E2E_RETENTION_POLICY_ID = "019b0000-0000-7000-8000-000000000701";
export const E2E_RETENTION_VERSION_ID = "019b0000-0000-7000-8000-000000000702";
export const E2E_APPENDED_RETENTION_VERSION_ID =
  "019b0000-0000-7000-8000-000000000703";
export const E2E_ROLE_ID = "019b0000-0000-7000-8000-000000000901";
export const E2E_HUMAN_ROLE_ASSIGNMENT_ID =
  "019b0000-0000-7000-8000-000000000902";
export const E2E_AGENT_ROLE_ASSIGNMENT_ID =
  "019b0000-0000-7000-8000-000000000903";
export const E2E_MAIL_DOMAIN_ID = "019b0000-0000-7000-8000-000000000801";
export const E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID =
  "019b0000-0000-7000-8000-000000000802";
export const E2E_MAIL_DOMAIN_NAME = "relay.example.test";
export const E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME = "unknown.example.test";
// These are deterministic fake browser-fixture values, never production
// credentials. The application treats them as one-time disclosure material.
export const E2E_INITIAL_MAIL_DOMAIN_TOKEN = `cmv1_${"a".repeat(43)}`;
export const E2E_ROTATED_MAIL_DOMAIN_TOKEN = `cmv1_${"b".repeat(43)}`;
export const E2E_FIXED_NOW = "2026-08-24T12:00:00.000Z";
export const E2E_AUDIT_FROM = "2026-08-23T12:00:00.000Z";
export const E2E_AUDIT_TO = E2E_FIXED_NOW;

const HUMAN_SESSION_COOKIE = "copilotmail_human_session";
const HUMAN_SESSION_VALUE = "e2e-active-human-admin-session";

const EMPTY_PAGE = Object.freeze({
  items: [],
  next_cursor: null,
  has_more: false,
});

interface AdminRetentionVersionFixture {
  retention_policy_version_id: string;
  version: number;
  minimum_retention_days: number;
  archive_after_days: number | null;
  delete_after_days: number | null;
  rule: Record<string, unknown>;
  created_by: string;
  created_at: string;
}

interface AdminRetentionPolicyFixture {
  retention_policy_id: string;
  organization_id: string;
  policy_key: string;
  display_name: string;
  resource_kind: "audit_evidence" | "mail_content";
  active_version: AdminRetentionVersionFixture | null;
  latest_version: AdminRetentionVersionFixture;
  created_by: string;
  created_at: string;
}

/**
 * Browser fixture mirrors the role and assignment DTOs exactly. The two
 * subjects intentionally reuse the existing Agent Inspector owner and Agent
 * identities, so the Roles visual contract never invents an identity
 * relationship that the application cannot actually read.
 */
interface AdminRoleFixture {
  role_id: string;
  organization_id: string;
  system_role: "mail_admin";
  display_name: string;
  permissions: string[];
  created_at: string;
}

interface AdminRoleAssignmentFixture {
  role_assignment_id: string;
  organization_id: string;
  principal_id: string;
  principal_kind: "human" | "agent" | "service";
  principal_status: "active" | "suspended";
  principal_display_name: string;
  role_id: string;
  system_role: "mail_admin";
  role_display_name: string;
  created_by: string;
  approval_request_id: string | null;
  valid_from: string;
  valid_until: string | null;
  revoked_at: string | null;
  created_at: string;
  active: boolean;
}

type MailDomainFixtureStatus =
  "pending_dns" | "verified" | "active" | "suspended";
type MailDomainFixtureRouteMode = "direct_mx" | "smart_host";

/**
 * Stateful local control-plane model used exclusively by browser-contract
 * tests. The private token is intentionally omitted from `mailDomainDto` so
 * it can only leave the model through registration or challenge rotation.
 */
interface AdminMailDomainFixture {
  mail_domain_id: string;
  organization_id: string;
  name: string;
  status: MailDomainFixtureStatus;
  route_mode: MailDomainFixtureRouteMode;
  ownership_challenge_version: number;
  route_version: number;
  ownership_token: string;
  inbound_enabled: boolean;
  outbound_enabled: boolean;
  smart_host_ref: string | null;
  dkim_key_ref: string | null;
  mail_engine_domain_ref: string | null;
  mail_engine_projection_status: "not_projected" | "pending";
  mail_engine_projected_enabled: boolean | null;
  mail_engine_projection_last_attempt_at: string | null;
  mail_engine_projection_succeeded_at: string | null;
  mail_engine_projection_error_code: string | null;
  latest_dns_observed_at: string | null;
  latest_dns_valid_until: string | null;
  latest_dns_activation_ready: boolean;
  latest_dns_failure_codes: string[];
}

function mailDomainDto(domain: AdminMailDomainFixture) {
  return {
    mail_domain_id: domain.mail_domain_id,
    organization_id: domain.organization_id,
    name: domain.name,
    status: domain.status,
    route_mode: domain.route_mode,
    ownership_challenge_version: domain.ownership_challenge_version,
    route_version: domain.route_version,
    inbound_enabled: domain.inbound_enabled,
    outbound_enabled: domain.outbound_enabled,
    smart_host_ref: domain.smart_host_ref,
    dkim_key_ref: domain.dkim_key_ref,
    mail_engine_domain_ref: domain.mail_engine_domain_ref,
    mail_engine_projection_status: domain.mail_engine_projection_status,
    mail_engine_projected_enabled: domain.mail_engine_projected_enabled,
    mail_engine_projection_last_attempt_at:
      domain.mail_engine_projection_last_attempt_at,
    mail_engine_projection_succeeded_at:
      domain.mail_engine_projection_succeeded_at,
    mail_engine_projection_error_code: domain.mail_engine_projection_error_code,
    latest_dns_observed_at: domain.latest_dns_observed_at,
    latest_dns_valid_until: domain.latest_dns_valid_until,
    latest_dns_activation_ready: domain.latest_dns_activation_ready,
    latest_dns_failure_codes: domain.latest_dns_failure_codes,
  };
}

function domainFixture(
  input: Pick<
    AdminMailDomainFixture,
    "mail_domain_id" | "name" | "status" | "route_mode"
  > &
    Partial<AdminMailDomainFixture>,
): AdminMailDomainFixture {
  return {
    organization_id: E2E_ORGANIZATION_ID,
    ownership_challenge_version: 1,
    route_version: 1,
    ownership_token: E2E_INITIAL_MAIL_DOMAIN_TOKEN,
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
    ...input,
  };
}

export const E2E_COMPLIANCE_CASE = Object.freeze({
  case_id: E2E_CASE_ID,
  organization_id: E2E_ORGANIZATION_ID,
  external_reference: "SEC-2026-0842",
  title: "Mailbox domain activation review",
  purpose:
    "Preserve the evidence trail for a controlled production-domain activation.",
  status: "open",
  created_by: E2E_ACTIVE_HUMAN_ID,
  created_at: "2026-08-22T08:30:00Z",
  closed_by: null,
  closed_at: null,
});

export const E2E_LEGAL_HOLD = Object.freeze({
  legal_hold_id: E2E_HOLD_ID,
  organization_id: E2E_ORGANIZATION_ID,
  case_id: E2E_CASE_ID,
  archive_id: E2E_ARCHIVE_ID,
  reason:
    "Preserve domain-activation audit evidence while the review remains open.",
  status: "active",
  apply_approval_request_id: "019b0000-0000-7000-8000-000000000302",
  created_by: E2E_ACTIVE_HUMAN_ID,
  created_at: "2026-08-22T09:00:00Z",
  release_approval_request_id: null,
  released_by: null,
  released_at: null,
});

export const E2E_APPROVAL_REQUEST = Object.freeze({
  approval_request_id: E2E_APPROVAL_ID,
  organization_id: E2E_ORGANIZATION_ID,
  policy_version_id: "019b0000-0000-7000-8000-000000000502",
  action: "audit.export",
  resource_kind: "audit_archive",
  resource_id: E2E_ARCHIVE_ID,
  requested_by: "019b0000-0000-7000-8000-000000000503",
  reason: "Export the signed archive for the domain-activation review.",
  ticket_reference: "SEC-2026-0842",
  required_approvals: 2,
  approval_count: 0,
  rejection_count: 0,
  status: "pending",
  created_at: "2026-08-23T08:00:00Z",
  expires_at: "2026-08-25T08:00:00Z",
  completed_at: null,
  cancelled_at: null,
});

export const E2E_APPROVAL_DETAIL = Object.freeze({
  request: E2E_APPROVAL_REQUEST,
  steps: [],
});

export const E2E_AUDIT_EVENT = Object.freeze({
  event_id: E2E_AUDIT_EVENT_ID,
  organization_id: E2E_ORGANIZATION_ID,
  actor_principal_id: E2E_ACTIVE_HUMAN_ID,
  actor_kind: "human",
  on_behalf_of_principal_id: null,
  credential_id: null,
  subject_principal_id: null,
  action: "mail_domain.activated",
  resource_type: "mail_domain",
  resource_id: "corp.example",
  outcome: "success",
  trace_id: "trace-e2e-domain-activation",
  request_id: "request-e2e-domain-activation",
  source_ip: "192.0.2.24",
  client_id: "admin",
  session_id: "session-e2e-admin",
  token_jti: null,
  policy_id: null,
  policy_version_id: "019b0000-0000-7000-8000-000000000602",
  approval_request_id: E2E_APPROVAL_ID,
  ticket_reference: "SEC-2026-0842",
  environment: "production",
  region: "cn-east-1",
  details: { domain: "corp.example", verification: "dns" },
  occurred_at: "2026-08-24T09:15:00Z",
  received_at: "2026-08-24T09:15:01Z",
  chain_sequence: "1842",
  hash_version: 1,
  previous_hash: "a".repeat(64),
  event_hash: "b".repeat(64),
  signature_batch_id: "019b0000-0000-7000-8000-000000000603",
});

export const E2E_RETENTION_POLICY: Readonly<AdminRetentionPolicyFixture> =
  Object.freeze({
    retention_policy_id: E2E_RETENTION_POLICY_ID,
    organization_id: E2E_ORGANIZATION_ID,
    policy_key: "audit.production",
    display_name: "Production audit evidence",
    resource_kind: "audit_evidence",
    active_version: {
      retention_policy_version_id: E2E_RETENTION_VERSION_ID,
      version: 1,
      minimum_retention_days: 2555,
      archive_after_days: 30,
      delete_after_days: null,
      rule: { environment: "production" },
      created_by: E2E_ACTIVE_HUMAN_ID,
      created_at: "2026-08-20T08:00:00Z",
    },
    latest_version: {
      retention_policy_version_id: E2E_RETENTION_VERSION_ID,
      version: 1,
      minimum_retention_days: 2555,
      archive_after_days: 30,
      delete_after_days: null,
      rule: { environment: "production" },
      created_by: E2E_ACTIVE_HUMAN_ID,
      created_at: "2026-08-20T08:00:00Z",
    },
    created_by: E2E_ACTIVE_HUMAN_ID,
    created_at: "2026-08-20T08:00:00Z",
  });

export const E2E_ROLE: Readonly<AdminRoleFixture> = Object.freeze({
  role_id: E2E_ROLE_ID,
  organization_id: E2E_ORGANIZATION_ID,
  system_role: "mail_admin",
  display_name: "Mail administrator",
  permissions: ["mail.domain.manage", "mail.routing.manage"],
  created_at: "2026-08-20T08:00:00Z",
});

export const E2E_ROLE_ASSIGNMENTS: readonly Readonly<AdminRoleAssignmentFixture>[] =
  Object.freeze([
    Object.freeze({
      role_assignment_id: E2E_HUMAN_ROLE_ASSIGNMENT_ID,
      organization_id: E2E_ORGANIZATION_ID,
      principal_id: E2E_AGENT_OWNER_ID,
      principal_kind: "human",
      principal_status: "active",
      principal_display_name: E2E_AGENT_OWNER_DISPLAY_NAME,
      role_id: E2E_ROLE_ID,
      system_role: "mail_admin",
      role_display_name: "Mail administrator",
      created_by: E2E_ACTIVE_HUMAN_ID,
      approval_request_id: null,
      valid_from: "2026-08-01T08:00:00Z",
      valid_until: null,
      revoked_at: null,
      created_at: "2026-08-01T08:00:00Z",
      active: true,
    }),
    Object.freeze({
      role_assignment_id: E2E_AGENT_ROLE_ASSIGNMENT_ID,
      organization_id: E2E_ORGANIZATION_ID,
      principal_id: E2E_AGENT_ID,
      principal_kind: "agent",
      principal_status: "active",
      principal_display_name: E2E_AGENT_DISPLAY_NAME,
      role_id: E2E_ROLE_ID,
      system_role: "mail_admin",
      role_display_name: "Mail administrator",
      created_by: E2E_ACTIVE_HUMAN_ID,
      approval_request_id: null,
      valid_from: "2026-08-20T08:00:00Z",
      valid_until: "2026-09-20T08:00:00Z",
      revoked_at: null,
      created_at: "2026-08-20T08:00:00Z",
      active: true,
    }),
  ]);

/**
 * This is a bounded Agent Inspector DTO, rather than a synthetic mailbox or
 * runtime model. It deliberately includes no delivery, heartbeat, execution,
 * message, credential-material, or address data.
 */
export const E2E_AGENT_INSPECTOR_DTO = Object.freeze({
  agent_id: E2E_AGENT_ID,
  display_name: E2E_AGENT_DISPLAY_NAME,
  principal_status: "active",
  owner: {
    principal_id: E2E_AGENT_OWNER_ID,
    display_name: E2E_AGENT_OWNER_DISPLAY_NAME,
    principal_status: "active",
  },
  profile: {
    environment: "testing",
    risk_level: "low",
    purpose: "Browser-contract verification",
    review_due_at: "2026-09-01T00:00:00Z",
    review_state: "scheduled",
  },
  token_access: {
    state: "effective",
    effective_scopes: ["organization.read", "agent.operate"],
    active_credential_count: 1,
    next_credential_expiry_at: "2026-12-01T00:00:00Z",
    last_authenticated_at: "2026-08-23T09:01:00Z",
  },
  control_plane: {
    // Non-production Agents never imply a production mail-policy admission.
    mail_admission_state: "not_required",
    active_processing_subscription_count: 1,
    paused_processing_subscription_count: 0,
    retired_processing_subscription_count: 0,
    last_processing_activity_at: "2026-08-23T08:59:00Z",
  },
  observed_at: "2026-08-23T09:02:00Z",
});

/**
 * P3 evidence exists only for a production Agent that has an active mail
 * admission. Keep this separate from the broadly reused testing Inspector
 * fixture so browser tests never imply that a non-production profile can
 * disclose production submission evidence.
 */
export const E2E_AGENT_INSPECTOR_PRODUCTION_DTO = Object.freeze({
  ...E2E_AGENT_INSPECTOR_DTO,
  profile: {
    ...E2E_AGENT_INSPECTOR_DTO.profile,
    environment: "production",
  },
  control_plane: {
    ...E2E_AGENT_INSPECTOR_DTO.control_plane,
    mail_admission_state: "active",
  },
});

/**
 * Exact bounded evidence for one explicitly entered production-Agent
 * submission. It intentionally contains no message, recipient, mailbox,
 * runtime, execution, delegation, provider, or SMTP fields.
 */
export const E2E_AGENT_MAIL_SUBMISSION_EVIDENCE_DTO = Object.freeze({
  agent_id: E2E_AGENT_ID,
  submission: {
    submission_id: E2E_AGENT_MAIL_SUBMISSION_ID,
    status: "accepted",
    created_at: "2026-08-24T09:00:00Z",
    updated_at: "2026-08-24T09:01:00Z",
  },
  admission: {
    admission_id: "019b0000-0000-7000-8000-000000000110",
    admitted_at: "2026-08-24T09:00:05Z",
    policy: {
      activation_id: "019b0000-0000-7000-8000-000000000111",
      activated_at: "2026-08-24T08:59:00Z",
      version_id: "019b0000-0000-7000-8000-000000000112",
      version: 4,
      approval_request_id: "019b0000-0000-7000-8000-000000000113",
    },
    recipient_count: 2,
    first_contact_count: 0,
    first_contact_approval_request_id: null,
  },
  delivery: null,
  audit_references: [
    {
      event_id: "019b0000-0000-7000-8000-000000000114",
      action: "mail.submission.requested",
      occurred_at: "2026-08-24T09:00:00Z",
    },
    {
      event_id: "019b0000-0000-7000-8000-000000000115",
      action: "agent.mail.admitted",
      occurred_at: "2026-08-24T09:00:05Z",
    },
    {
      event_id: "019b0000-0000-7000-8000-000000000116",
      action: "mail.submission.accepted",
      occurred_at: "2026-08-24T09:01:00Z",
    },
  ],
  observed_at: "2026-08-24T09:02:00Z",
});

export interface AdminApiRequestLogEntry {
  /** The log deliberately excludes headers, cookies, and request bodies. */
  readonly method: string;
  readonly path: string;
}

export interface AdminInspectorFixtureResponse {
  readonly status: 200 | 403 | 404 | 503;
  readonly body: unknown;
}

export interface AdminMailSubmissionEvidenceFixtureResponse {
  readonly status: 200 | 403 | 404 | 503;
  readonly body: unknown;
}

export interface AdminMailSubmissionEvidenceRead {
  readonly agentId: string;
  readonly submissionId: string;
}

export interface AdminAppFixture {
  /** Calls made to the explicitly auditable inspector endpoint, in order. */
  readonly inspectorReads: readonly string[];
  /** Calls made to the exact, explicitly requested submission-evidence read. */
  readonly mailSubmissionEvidenceReads: readonly AdminMailSubmissionEvidenceRead[];
  /** Sanitized, ordered API request evidence for browser-contract assertions. */
  readonly requestLog: readonly AdminApiRequestLogEntry[];
  /** Any undeclared `/api` request receives a local 501 response and is kept here. */
  readonly unexpectedApiRequests: readonly string[];
  /** Approval detail identities read through the exact detail endpoint. */
  readonly approvalDetailReads: readonly string[];
  /**
   * Replaces the default successful Inspector response with an exact sequence.
   * Once configured, an extra Inspector request is treated as undeclared.
   */
  setInspectorResponses(
    responses: readonly AdminInspectorFixtureResponse[],
  ): void;
  /**
   * Replaces the default exact-pair submission-evidence response sequence.
   * Once configured, an extra evidence read is treated as undeclared.
   */
  setMailSubmissionEvidenceResponses(
    responses: readonly AdminMailSubmissionEvidenceFixtureResponse[],
  ): void;
  /** Holds the next approval-detail response until explicitly released. */
  delayNextApprovalDetail(): void;
  /** Releases one response held by `delayNextApprovalDetail`. */
  releaseApprovalDetail(): void;
  goto(): Promise<void>;
  openAgentInspectorWithKeyboard(): Promise<void>;
  lookupAgentWithKeyboard(agentId?: string): Promise<void>;
}

interface AdminRouteSelectionOptions {
  /** The translated narrow-screen menu controls for the current locale. */
  drawerName?: string;
  keyboard?: boolean;
  triggerName?: string;
}

/**
 * Selects the same Admin route through the visible navigation surface. At
 * 1023px and below this proves the current-page header and grouped drawer;
 * from 1024px through 1279px it uses the explicit compact-rail trigger and
 * the same drawer; at 1280px and above it uses the persistent full rail.
 */
export async function selectAdminRoute(
  page: Page,
  routeName: string,
  {
    drawerName = "Admin navigation",
    keyboard = false,
    triggerName = "Open navigation",
  }: AdminRouteSelectionOptions = {},
) {
  const railRoute = page
    .locator(".admin-navigation--rail")
    .getByRole("button", {
      name: routeName,
      exact: true,
    });
  const activate = async (control: ReturnType<Page["getByRole"]>) => {
    if (keyboard) {
      await control.focus();
      await page.keyboard.press("Enter");
    } else {
      await control.click();
    }
  };

  if (await railRoute.isVisible()) {
    await activate(railRoute);
    return;
  }

  const trigger = page.getByRole("button", { name: triggerName, exact: true });
  await expect(trigger).toBeVisible();
  await activate(trigger);

  const drawer = page.getByRole("dialog", { name: drawerName, exact: true });
  await expect(drawer).toBeVisible();
  const drawerRoute = drawer.getByRole("button", {
    name: routeName,
    exact: true,
  });
  await activate(drawerRoute);
  await expect(drawer).toBeHidden();
}

interface AdminFixtures {
  adminApp: AdminAppFixture;
}

function jsonHeaders(): Record<string, string> {
  return {
    "cache-control": "no-store",
    "content-type": "application/json; charset=utf-8",
  };
}

function hasExpectedHumanSession(cookie: string | undefined): boolean {
  return (
    cookie
      ?.split(";")
      .some(
        (part) =>
          part.trim() === `${HUMAN_SESSION_COOKIE}=${HUMAN_SESSION_VALUE}`,
      ) ?? false
  );
}

function routeName(method: string, path: string): string {
  return `${method.toUpperCase()} ${path}`;
}

function hasExactQuery(
  url: URL,
  expected: ReadonlyArray<readonly [string, string]>,
): boolean {
  const actual = [...url.searchParams.entries()];
  return (
    actual.length === expected.length &&
    actual.every(([key, value], index) => {
      const pair = expected[index];
      return pair?.[0] === key && pair[1] === value;
    })
  );
}

function assertBaseUrl(baseURL: string | undefined): URL {
  if (!baseURL) {
    throw new Error(
      "Admin browser tests require a Playwright baseURL that serves the built admin application.",
    );
  }
  return new URL(baseURL);
}

async function installAdminRoutes(
  page: Page,
  allowedOrigin: string,
  unexpectedApiRequests: string[],
  inspectorReads: string[],
  mailSubmissionEvidenceReads: AdminMailSubmissionEvidenceRead[],
  requestLog: AdminApiRequestLogEntry[],
  approvalDetailReads: string[],
  nextInspectorResponse: () => AdminInspectorFixtureResponse | undefined,
  hasConfiguredInspectorResponses: () => boolean,
  nextMailSubmissionEvidenceResponse: () =>
    AdminMailSubmissionEvidenceFixtureResponse | undefined,
  hasConfiguredMailSubmissionEvidenceResponses: () => boolean,
  waitForApprovalDetail: () => Promise<void>,
): Promise<void> {
  let retentionPolicy: AdminRetentionPolicyFixture = {
    ...E2E_RETENTION_POLICY,
  };
  let retentionAppendCount = 0;
  const mailDomains = new Map<string, AdminMailDomainFixture>([
    [
      E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID,
      domainFixture({
        mail_domain_id: E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID,
        name: E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME,
        status: "active",
        route_mode: "direct_mx",
        route_version: 7,
        latest_dns_observed_at: "2026-08-24T10:00:00Z",
        latest_dns_valid_until: "2026-08-24T13:00:00Z",
        latest_dns_activation_ready: true,
      }),
    ],
  ]);
  // Register the catch-all first so the more specific handlers below run
  // before it. Static assets may use the one loopback origin; everything else
  // is blocked before a future IdP, CDN, or telemetry integration can escape.
  await page.route("**/*", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin !== allowedOrigin) {
      unexpectedApiRequests.push(
        `${request.method()} blocked external origin ${url.origin}`,
      );
      await route.abort("blockedbyclient");
      return;
    }
    await route.fallback();
  });

  await page.route("**/runtime-config.js", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/javascript; charset=utf-8",
      headers: { "cache-control": "no-store" },
      body: [
        "window.__COPILOTMAIL_CONFIG__ = Object.freeze({",
        "  apiBaseUrl: window.location.origin,",
        `  organizationId: \"${E2E_ORGANIZATION_ID}\",`,
        "});",
      ].join("\n"),
    });
  });

  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const method = request.method();
    requestLog.push(Object.freeze({ method, path }));

    if (path === "/api/v2/auth/admin/session" && method === "GET") {
      if (!hasExpectedHumanSession(request.headers().cookie)) {
        await route.fulfill({
          status: 401,
          headers: jsonHeaders(),
          body: JSON.stringify({ code: "unauthenticated" }),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          principal_id: E2E_ACTIVE_HUMAN_ID,
          organization_id: E2E_ORGANIZATION_ID,
          client: "admin",
          expires_at: "2026-12-31T23:59:59Z",
        }),
      });
      return;
    }

    const casesPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/compliance-cases`;
    const holdsPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/legal-holds`;
    const approvalsPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/approval-requests`;
    const auditPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/audit-events`;
    const retentionPolicyPath = `/api/v2/retention-policies/${E2E_RETENTION_POLICY_ID}`;
    const rolesPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/roles`;
    const roleAssignmentsPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/role-assignments`;
    const mailDomainsPath = `/api/v2/organizations/${E2E_ORGANIZATION_ID}/mail-domains`;

    if (method === "GET" && path === rolesPath && hasExactQuery(url, [])) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({ items: [E2E_ROLE] }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === roleAssignmentsPath &&
      hasExactQuery(url, [
        ["include_inactive", "false"],
        ["limit", "50"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          items: E2E_ROLE_ASSIGNMENTS,
          next_cursor: null,
          has_more: false,
        }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === mailDomainsPath &&
      hasExactQuery(url, [["limit", "50"]])
    ) {
      const items = [...mailDomains.values()]
        .sort((left, right) => left.name.localeCompare(right.name))
        .map(mailDomainDto);
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({ items, next_after: null }),
      });
      return;
    }

    if (
      method === "POST" &&
      path === mailDomainsPath &&
      hasExactQuery(url, [])
    ) {
      const body = request.postDataJSON();
      const input =
        body && typeof body === "object" && !Array.isArray(body)
          ? (body as Record<string, unknown>)
          : undefined;
      if (
        !input ||
        Object.keys(input).length !== 2 ||
        input.name !== E2E_MAIL_DOMAIN_NAME ||
        input.route_mode !== "smart_host" ||
        mailDomains.has(E2E_MAIL_DOMAIN_ID)
      ) {
        await route.fulfill({
          status: 422,
          headers: jsonHeaders(),
          body: JSON.stringify({ code: "invalid_mail_domain_registration" }),
        });
        return;
      }
      const created = domainFixture({
        mail_domain_id: E2E_MAIL_DOMAIN_ID,
        name: E2E_MAIL_DOMAIN_NAME,
        status: "pending_dns",
        route_mode: "smart_host",
        ownership_token: E2E_INITIAL_MAIL_DOMAIN_TOKEN,
      });
      mailDomains.set(created.mail_domain_id, created);
      await route.fulfill({
        status: 201,
        headers: jsonHeaders(),
        body: JSON.stringify({
          mail_domain_id: created.mail_domain_id,
          organization_id: created.organization_id,
          name: created.name,
          status: created.status,
          ownership_challenge_version: created.ownership_challenge_version,
          route_version: created.route_version,
          ownership_record_name: `_copilotmail-verification.${created.name}`,
          ownership_token: created.ownership_token,
        }),
      });
      return;
    }

    const mailDomainRead = path.match(/^\/api\/v2\/mail-domains\/([^/]+)$/);
    if (method === "GET" && mailDomainRead && hasExactQuery(url, [])) {
      const domain = mailDomains.get(decodeURIComponent(mailDomainRead[1]!));
      if (!domain) {
        await route.fulfill({
          status: 404,
          headers: jsonHeaders(),
          body: JSON.stringify({ code: "mail_domain_not_found" }),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify(mailDomainDto(domain)),
      });
      return;
    }

    const mailDomainAction = path.match(
      /^\/api\/v2\/mail-domains\/([^/]+)\/(ownership-challenge|dns-verifications|activation|suspension|route)$/,
    );
    if (mailDomainAction && hasExactQuery(url, [])) {
      const domain = mailDomains.get(decodeURIComponent(mailDomainAction[1]!));
      const action = mailDomainAction[2];
      if (!domain) {
        await route.fulfill({
          status: 404,
          headers: jsonHeaders(),
          body: JSON.stringify({ code: "mail_domain_not_found" }),
        });
        return;
      }

      if (method === "POST" && action === "ownership-challenge") {
        const body = request.postDataJSON();
        const input =
          body && typeof body === "object" && !Array.isArray(body)
            ? (body as Record<string, unknown>)
            : undefined;
        if (
          !input ||
          Object.keys(input).length !== 1 ||
          input.expected_version !== domain.ownership_challenge_version
        ) {
          await route.fulfill({
            status: 409,
            headers: jsonHeaders(),
            body: JSON.stringify({ code: "mail_domain_challenge_conflict" }),
          });
          return;
        }
        domain.ownership_challenge_version += 1;
        domain.ownership_token = E2E_ROTATED_MAIL_DOMAIN_TOKEN;
        domain.status = "pending_dns";
        domain.latest_dns_observed_at = null;
        domain.latest_dns_valid_until = null;
        domain.latest_dns_activation_ready = false;
        domain.latest_dns_failure_codes = [];
        await route.fulfill({
          status: 200,
          headers: jsonHeaders(),
          body: JSON.stringify({
            mail_domain_id: domain.mail_domain_id,
            status: "pending_dns",
            ownership_challenge_version: domain.ownership_challenge_version,
            ownership_record_name: `_copilotmail-verification.${domain.name}`,
            ownership_token: domain.ownership_token,
          }),
        });
        return;
      }

      if (method === "POST" && action === "dns-verifications") {
        const body = request.postDataJSON();
        const input =
          body && typeof body === "object" && !Array.isArray(body)
            ? (body as Record<string, unknown>)
            : undefined;
        if (
          !input ||
          Object.keys(input).length !== 3 ||
          input.domain !== domain.name ||
          input.ownership_token !== domain.ownership_token ||
          input.dkim_selector !== "mail2026" ||
          domain.status !== "pending_dns"
        ) {
          await route.fulfill({
            status: 422,
            headers: jsonHeaders(),
            body: JSON.stringify({ code: "mail_domain_dns_evidence_invalid" }),
          });
          return;
        }
        domain.status = "verified";
        domain.latest_dns_observed_at = "2026-08-24T11:00:00Z";
        domain.latest_dns_valid_until = "2026-08-24T14:00:00Z";
        domain.latest_dns_activation_ready = true;
        domain.latest_dns_failure_codes = [];
        await route.fulfill({
          status: 200,
          headers: jsonHeaders(),
          body: JSON.stringify({
            mail_domain_id: domain.mail_domain_id,
            status: "verified",
            activation_ready: true,
            valid_until: domain.latest_dns_valid_until,
            failure_codes: [],
          }),
        });
        return;
      }

      if (method === "POST" && action === "activation") {
        if (
          domain.status !== "verified" ||
          !domain.latest_dns_activation_ready
        ) {
          await route.fulfill({
            status: 409,
            headers: jsonHeaders(),
            body: JSON.stringify({ code: "mail_domain_not_activation_ready" }),
          });
          return;
        }
        domain.status = "active";
        domain.mail_engine_projection_status = "pending";
        domain.mail_engine_projection_last_attempt_at = "2026-08-24T11:05:00Z";
        await route.fulfill({
          status: 200,
          headers: jsonHeaders(),
          body: JSON.stringify({
            mail_domain_id: domain.mail_domain_id,
            status: "active",
          }),
        });
        return;
      }

      if (method === "PUT" && action === "route") {
        const body = request.postDataJSON();
        const input =
          body && typeof body === "object" && !Array.isArray(body)
            ? (body as Record<string, unknown>)
            : undefined;
        const expectedVersion = input?.expected_version;
        const inbound = input?.inbound_enabled;
        const outbound = input?.outbound_enabled;
        const smartHostRef = input?.smart_host_ref;
        const dkimKeyRef = input?.dkim_key_ref;
        const routeIsValid =
          input &&
          Object.keys(input).length === 5 &&
          expectedVersion === domain.route_version &&
          typeof inbound === "boolean" &&
          typeof outbound === "boolean" &&
          (smartHostRef === null || typeof smartHostRef === "string") &&
          (dkimKeyRef === null || typeof dkimKeyRef === "string") &&
          domain.status === "active" &&
          (!outbound || dkimKeyRef !== null) &&
          (domain.route_mode !== "smart_host" ||
            !outbound ||
            smartHostRef !== null) &&
          (domain.route_mode !== "direct_mx" || smartHostRef === null);
        if (!routeIsValid) {
          await route.fulfill({
            status: 409,
            headers: jsonHeaders(),
            body: JSON.stringify({ code: "mail_domain_route_conflict" }),
          });
          return;
        }
        domain.route_version += 1;
        domain.inbound_enabled = inbound;
        domain.outbound_enabled = outbound;
        domain.smart_host_ref = smartHostRef;
        domain.dkim_key_ref = dkimKeyRef;
        if (domain.mail_domain_id === E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID) {
          // Model a server that committed the change but whose response is not
          // trustworthy. The UI must never retry this write automatically.
          await route.fulfill({
            status: 503,
            headers: jsonHeaders(),
            body: JSON.stringify({
              code: "mail_domain_route_response_unknown",
              message:
                "The local browser fixture intentionally withheld the route result.",
              request_id: "e2e-domain-route-unknown",
              trace_id: "e2e-domain-route-unknown",
              retryable: true,
            }),
          });
          return;
        }
        await route.fulfill({
          status: 200,
          headers: jsonHeaders(),
          body: JSON.stringify({
            mail_domain_id: domain.mail_domain_id,
            version: domain.route_version,
            inbound_enabled: domain.inbound_enabled,
            outbound_enabled: domain.outbound_enabled,
          }),
        });
        return;
      }

      if (method === "POST" && action === "suspension") {
        const body = request.postDataJSON();
        const input =
          body && typeof body === "object" && !Array.isArray(body)
            ? (body as Record<string, unknown>)
            : undefined;
        if (
          !input ||
          Object.keys(input).length !== 1 ||
          input.reason_code !== "maintenance_window" ||
          domain.status !== "active"
        ) {
          await route.fulfill({
            status: 422,
            headers: jsonHeaders(),
            body: JSON.stringify({ code: "mail_domain_suspension_invalid" }),
          });
          return;
        }
        domain.status = "suspended";
        domain.route_version += 1;
        domain.inbound_enabled = false;
        domain.outbound_enabled = false;
        await route.fulfill({
          status: 200,
          headers: jsonHeaders(),
          body: JSON.stringify({
            mail_domain_id: domain.mail_domain_id,
            status: "suspended",
          }),
        });
        return;
      }
    }

    // The application opens the non-privileged overview first. Its bounded
    // aggregate reads stay distinct from the real list screens by query.
    if (
      method === "GET" &&
      path === casesPath &&
      hasExactQuery(url, [
        ["status", "open"],
        ["limit", "100"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify(EMPTY_PAGE),
      });
      return;
    }

    if (
      method === "GET" &&
      path === holdsPath &&
      hasExactQuery(url, [
        ["status", "active"],
        ["limit", "100"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify(EMPTY_PAGE),
      });
      return;
    }

    if (
      method === "GET" &&
      path === casesPath &&
      hasExactQuery(url, [
        ["status", "open"],
        ["limit", "25"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          items: [E2E_COMPLIANCE_CASE],
          next_cursor: null,
          has_more: false,
        }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === `/api/v2/compliance-cases/${E2E_CASE_ID}` &&
      hasExactQuery(url, [])
    ) {
      // A Hold may navigate only through its contract-backed case_id. The
      // Cases workspace must read this exact resource; a cursor page cannot
      // establish that a Case is the Hold's linked record.
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify(E2E_COMPLIANCE_CASE),
      });
      return;
    }

    if (
      method === "GET" &&
      path === holdsPath &&
      hasExactQuery(url, [
        ["status", "active"],
        ["case_id", E2E_CASE_ID],
        ["limit", "25"],
      ])
    ) {
      // Cases exposes exactly one supported relationship: opening the holds
      // queue constrained to the selected case ID. Keep its fixture route as
      // strict as every other privileged browser read.
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          items: [E2E_LEGAL_HOLD],
          next_cursor: null,
          has_more: false,
        }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === holdsPath &&
      hasExactQuery(url, [
        ["status", "active"],
        ["limit", "25"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          items: [E2E_LEGAL_HOLD],
          next_cursor: null,
          has_more: false,
        }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === approvalsPath &&
      hasExactQuery(url, [
        ["view", "actionable"],
        ["limit", "50"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          items: [E2E_APPROVAL_REQUEST],
          next_cursor: null,
          has_more: false,
        }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === `/api/v2/approval-requests/${E2E_APPROVAL_ID}` &&
      hasExactQuery(url, [])
    ) {
      approvalDetailReads.push(E2E_APPROVAL_ID);
      await waitForApprovalDetail();
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify(E2E_APPROVAL_DETAIL),
      });
      return;
    }

    if (
      method === "GET" &&
      path === auditPath &&
      hasExactQuery(url, [
        ["occurred_from", E2E_AUDIT_FROM],
        ["occurred_to", E2E_AUDIT_TO],
        ["limit", "50"],
      ])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify({
          items: [E2E_AUDIT_EVENT],
          next_cursor: `v1.${E2E_ORGANIZATION_ID}.${E2E_AUDIT_EVENT.chain_sequence}`,
          has_more: false,
          query_id: E2E_AUDIT_QUERY_ID,
          occurred_from: E2E_AUDIT_FROM,
          occurred_to: E2E_AUDIT_TO,
        }),
      });
      return;
    }

    if (
      method === "GET" &&
      path === retentionPolicyPath &&
      hasExactQuery(url, [])
    ) {
      await route.fulfill({
        status: 200,
        headers: jsonHeaders(),
        body: JSON.stringify(retentionPolicy),
      });
      return;
    }

    if (
      method === "POST" &&
      path === `${retentionPolicyPath}/versions` &&
      hasExactQuery(url, [])
    ) {
      retentionAppendCount += 1;
      if (retentionAppendCount !== 1) {
        unexpectedApiRequests.push(
          `${routeName(method, path)} repeated an immutable append`,
        );
        await route.fulfill({
          status: 501,
          headers: jsonHeaders(),
          body: JSON.stringify({ code: "e2e_repeated_retention_append" }),
        });
        return;
      }
      const input = request.postDataJSON() as {
        minimum_retention_days: number;
        archive_after_days: number | null;
        delete_after_days: number | null;
        rule: Record<string, unknown>;
      };
      const appended = {
        retention_policy_version_id: E2E_APPENDED_RETENTION_VERSION_ID,
        version: 2,
        minimum_retention_days: input.minimum_retention_days,
        archive_after_days: input.archive_after_days,
        delete_after_days: input.delete_after_days,
        rule: input.rule,
        created_by: E2E_ACTIVE_HUMAN_ID,
        created_at: "2026-08-24T11:30:00Z",
      };
      retentionPolicy = { ...retentionPolicy, latest_version: appended };
      await route.fulfill({
        status: 201,
        headers: jsonHeaders(),
        body: JSON.stringify(appended),
      });
      return;
    }

    const mailSubmissionEvidenceMatch = path.match(
      /^\/api\/v2\/agents\/([^/]+)\/mail-submissions\/([^/]+)\/evidence$/,
    );
    if (
      method === "POST" &&
      mailSubmissionEvidenceMatch &&
      hasExactQuery(url, [])
    ) {
      const agentId = decodeURIComponent(mailSubmissionEvidenceMatch[1]!);
      const submissionId = decodeURIComponent(mailSubmissionEvidenceMatch[2]!);
      mailSubmissionEvidenceReads.push(
        Object.freeze({ agentId, submissionId }),
      );
      const response = nextMailSubmissionEvidenceResponse();
      if (hasConfiguredMailSubmissionEvidenceResponses() && !response) {
        unexpectedApiRequests.push(
          `${routeName(method, path)} exceeded configured response matrix`,
        );
        await route.fulfill({
          status: 501,
          headers: jsonHeaders(),
          body: JSON.stringify({
            code: "e2e_mail_submission_evidence_response_matrix_exhausted",
            message:
              "No further local mail-submission evidence response is configured.",
            request_id: "e2e-matrix-exhausted-request",
            trace_id: "e2e-matrix-exhausted-trace",
            retryable: false,
          }),
        });
        return;
      }
      const selected =
        response ??
        (agentId === E2E_AGENT_ID &&
        submissionId === E2E_AGENT_MAIL_SUBMISSION_ID
          ? {
              status: 200 as const,
              body: E2E_AGENT_MAIL_SUBMISSION_EVIDENCE_DTO,
            }
          : {
              status: 404 as const,
              body: {
                code: "resource_not_found",
                message:
                  "No exact local submission evidence target is available.",
                request_id: "e2e-mail-submission-evidence-not-found",
                trace_id: "e2e-mail-submission-evidence-not-found",
                retryable: false,
              },
            });
      await route.fulfill({
        status: selected.status,
        headers: jsonHeaders(),
        body: JSON.stringify(selected.body),
      });
      return;
    }

    const inspectorMatch = path.match(
      /^\/api\/v2\/agents\/([^/]+)\/inspector$/,
    );
    if (method === "GET" && inspectorMatch) {
      const agentId = decodeURIComponent(inspectorMatch[1]!);
      inspectorReads.push(agentId);
      const response = nextInspectorResponse();
      if (hasConfiguredInspectorResponses() && !response) {
        unexpectedApiRequests.push(
          `${routeName(method, path)} exceeded configured response matrix`,
        );
        await route.fulfill({
          status: 501,
          headers: jsonHeaders(),
          body: JSON.stringify({
            code: "e2e_inspector_response_matrix_exhausted",
            message: "No further local Inspector response is configured.",
            request_id: "e2e-matrix-exhausted-request",
            trace_id: "e2e-matrix-exhausted-trace",
            retryable: false,
          }),
        });
        return;
      }
      const selected = response ?? {
        status: 200 as const,
        body: E2E_AGENT_INSPECTOR_DTO,
      };
      await route.fulfill({
        status: selected.status,
        headers: jsonHeaders(),
        body: JSON.stringify(selected.body),
      });
      return;
    }

    unexpectedApiRequests.push(routeName(method, `${path}${url.search}`));
    await route.fulfill({
      status: 501,
      headers: jsonHeaders(),
      body: JSON.stringify({
        code: "e2e_unhandled_api_request",
        message: "No local browser-test route is declared for this request.",
        request_id: "e2e-unhandled-request",
        trace_id: "e2e-unhandled-trace",
        retryable: false,
      }),
    });
  });
}

export const test = base.extend<AdminFixtures>({
  adminApp: async ({ baseURL, page }, use) => {
    const target = assertBaseUrl(baseURL);
    const unexpectedApiRequests: string[] = [];
    const inspectorReads: string[] = [];
    const mailSubmissionEvidenceReads: AdminMailSubmissionEvidenceRead[] = [];
    const requestLog: AdminApiRequestLogEntry[] = [];
    const approvalDetailReads: string[] = [];
    let configuredInspectorResponses = false;
    let inspectorResponses: AdminInspectorFixtureResponse[] = [];
    let configuredMailSubmissionEvidenceResponses = false;
    let mailSubmissionEvidenceResponses: AdminMailSubmissionEvidenceFixtureResponse[] =
      [];
    let delayNextApprovalDetail = false;
    let releaseApprovalDetail: (() => void) | undefined;

    await page.addInitScript(() => {
      // App locale is selected from navigator.language during initial render.
      // This keeps selector text and visual baselines independent of the host.
      Object.defineProperty(window.navigator, "language", {
        configurable: true,
        value: "en-US",
      });
      Object.defineProperty(window.navigator, "languages", {
        configurable: true,
        value: ["en-US", "en"],
      });
    });
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.context().addCookies([
      {
        name: HUMAN_SESSION_COOKIE,
        value: HUMAN_SESSION_VALUE,
        url: target.origin,
        httpOnly: true,
        secure: target.protocol === "https:",
        sameSite: "Lax",
      },
    ]);
    await installAdminRoutes(
      page,
      target.origin,
      unexpectedApiRequests,
      inspectorReads,
      mailSubmissionEvidenceReads,
      requestLog,
      approvalDetailReads,
      () => inspectorResponses.shift(),
      () => configuredInspectorResponses,
      () => mailSubmissionEvidenceResponses.shift(),
      () => configuredMailSubmissionEvidenceResponses,
      async () => {
        if (!delayNextApprovalDetail) return;
        delayNextApprovalDetail = false;
        await new Promise<void>((resolve) => {
          releaseApprovalDetail = resolve;
        });
        releaseApprovalDetail = undefined;
      },
    );

    await use({
      get inspectorReads() {
        return inspectorReads;
      },
      get mailSubmissionEvidenceReads() {
        return mailSubmissionEvidenceReads;
      },
      get requestLog() {
        return requestLog;
      },
      get unexpectedApiRequests() {
        return unexpectedApiRequests;
      },
      get approvalDetailReads() {
        return approvalDetailReads;
      },
      setInspectorResponses(responses) {
        if (responses.length === 0) {
          throw new Error(
            "The Inspector response matrix must contain at least one response.",
          );
        }
        configuredInspectorResponses = true;
        inspectorResponses = [...responses];
      },
      setMailSubmissionEvidenceResponses(responses) {
        if (responses.length === 0) {
          throw new Error(
            "The mail-submission evidence response matrix must contain at least one response.",
          );
        }
        configuredMailSubmissionEvidenceResponses = true;
        mailSubmissionEvidenceResponses = [...responses];
      },
      delayNextApprovalDetail() {
        if (delayNextApprovalDetail || releaseApprovalDetail) {
          throw new Error(
            "Only one approval-detail response may be delayed at a time.",
          );
        }
        delayNextApprovalDetail = true;
      },
      releaseApprovalDetail() {
        if (!releaseApprovalDetail) {
          throw new Error(
            "No approval-detail response is waiting to be released.",
          );
        }
        releaseApprovalDetail();
      },
      async goto() {
        await page.goto("/", { waitUntil: "domcontentloaded" });
        const usesAdaptiveNavigation =
          (page.viewportSize()?.width ?? Number.POSITIVE_INFINITY) <= 1279;
        if (!usesAdaptiveNavigation) {
          await expect(
            page.getByRole("navigation", { name: "Enterprise admin" }),
          ).toBeVisible();
        } else {
          await expect(
            page.getByRole("button", { name: "Open navigation", exact: true }),
          ).toBeVisible();
        }
      },
      async openAgentInspectorWithKeyboard() {
        await selectAdminRoute(page, "Agent inspector", { keyboard: true });
        await expect(
          page.getByRole("heading", { name: "Agent control-plane inspector" }),
        ).toBeVisible();
      },
      async lookupAgentWithKeyboard(agentId = E2E_AGENT_ID) {
        const field = page.getByRole("textbox", { name: "Agent ID" });
        await field.focus();
        await page.keyboard.type(agentId);
        await page.keyboard.press("Enter");
        await expect(
          page.getByRole("heading", { name: E2E_AGENT_DISPLAY_NAME }),
        ).toBeVisible();
      },
    });

    expect(
      unexpectedApiRequests,
      "An Admin browser test attempted an API request without a declared local route.",
    ).toEqual([]);
  },
});

export { expect };
