import {
  expect,
  test as base,
  type Page,
  type Route,
} from "@playwright/test";
import type {
  DraftResult,
  HumanDraftListItem,
  HumanDraftPage,
  HumanDraftRestore,
  HumanSentSubmissionPage,
  HumanSessionResponse,
  InboxPage,
  MailSubmission,
  MailboxDirectoryItem,
  MailboxDirectoryPage,
  MessageDetail,
  MessageState,
  MessageSummary,
} from "../../mail-app/src/types";

/**
 * These are syntactically valid, deterministic server identifiers. They are
 * deliberately test-only values: browser evidence must never need a live
 * identity provider, mailbox, or database.
 */
export const MAIL_FIXTURE_IDS = {
  organization: "10000000-0000-4000-8000-000000000001",
  principal: "10000000-0000-4000-8000-000000000002",
  mailbox: "20000000-0000-4000-8000-000000000001",
  message: "30000000-0000-4000-8000-000000000001",
  secondMessage: "30000000-0000-4000-8000-000000000002",
  draft: "40000000-0000-4000-8000-000000000001",
  submission: "50000000-0000-4000-8000-000000000001",
  sentAccepted: "50000000-0000-4000-8000-000000000014",
  sentQueued: "50000000-0000-4000-8000-000000000013",
  sentDelivered: "50000000-0000-4000-8000-000000000012",
  sentPartial: "50000000-0000-4000-8000-000000000011",
} as const;

export const MAIL_VISUAL_VIEWPORTS = [
  { name: "1440", width: 1440, height: 1024 },
  { name: "1280", width: 1280, height: 960 },
  { name: "1024", width: 1024, height: 900 },
  { name: "768", width: 768, height: 1024 },
  { name: "390", width: 390, height: 844 },
] as const;

export const AUTHORIZED_HUMAN_MAILBOX: MailboxDirectoryItem = {
  mailbox_id: MAIL_FIXTURE_IDS.mailbox,
  kind: "human",
  primary_address: "alex.chen@copilotmail.test",
  // The directory is the sole source of the browser's sendability fact.
  can_send: true,
};

export const LONG_SUBJECT = [
  "Quarterly risk review: Human and Agent mail-routing evidence, approval boundaries,",
  " retention checkpoints, and delivery reconciliation plan",
].join("");

export const FIXTURE_MESSAGE: MessageSummary = {
  message_id: MAIL_FIXTURE_IDS.message,
  subject: LONG_SUBJECT,
  from: [{ name: "Jordan Li", email: "jordan.li@partner.example" }],
  received_at: "2026-01-18T12:00:00Z",
  preview: "Please review the attached evidence before the weekly operational meeting.",
  is_read: false,
  is_flagged: false,
  has_attachment: true,
};

export const FIXTURE_MESSAGE_DETAIL: MessageDetail = {
  message: FIXTURE_MESSAGE,
  // The current API parser intentionally binds this value to the selected
  // message ID, so the fixture mirrors that exact contract.
  reply_to_message_id: MAIL_FIXTURE_IDS.message,
  to: [
    { name: "Priya Shah", email: "priya.shah@copilotmail.test" },
    { name: "Mateo Ortiz", email: "mateo.ortiz@copilotmail.test" },
    { name: "Nia Brooks", email: "nia.brooks@copilotmail.test" },
  ],
  cc: [{ name: "Operations Review", email: "ops-review@copilotmail.test" }],
  text_body: [
    "The attached report records the approved routing evidence and the current",
    "retention checkpoint. Please add any material corrections before Friday.",
  ].join(" "),
  text_body_truncated: false,
  attachments: [{
    name: "routing-evidence-q1.pdf",
    media_type: "application/pdf",
    size_bytes: 153_600,
  }],
};

export const FIXTURE_DRAFT_V4: HumanDraftListItem = {
  draft_id: MAIL_FIXTURE_IDS.draft,
  version: 4,
  reply_to_message_id: null,
  recipient_count: 3,
  created_at: "2026-01-17T09:00:00Z",
  updated_at: "2026-01-18T09:30:00Z",
};

export const FIXTURE_DRAFT_V5: HumanDraftListItem = {
  ...FIXTURE_DRAFT_V4,
  version: 5,
  updated_at: "2026-01-18T10:15:00Z",
};

export const FIXTURE_DRAFT_RESTORE_V5: HumanDraftRestore = {
  draft_id: MAIL_FIXTURE_IDS.draft,
  version: 5,
  reply_to_message_id: null,
  to: ["review-owner@copilotmail.test"],
  cc: ["operations@copilotmail.test"],
  bcc: ["audit@copilotmail.test"],
  subject: "Recovered retention review",
  text_body: "The server-authoritative version is ready for continued editing.",
};

export const SECOND_FIXTURE_MESSAGE: MessageSummary = {
  message_id: MAIL_FIXTURE_IDS.secondMessage,
  subject: "Incident response handoff and owner confirmation",
  from: [{ name: "Samira Khan", email: "samira.khan@partner.example" }],
  received_at: "2026-01-18T11:30:00Z",
  preview: "The escalation notes are ready for the next on-call owner.",
  is_read: true,
  is_flagged: false,
  has_attachment: false,
};

export const MAIL_SESSION: HumanSessionResponse = {
  principal_id: MAIL_FIXTURE_IDS.principal,
  organization_id: MAIL_FIXTURE_IDS.organization,
  client: "mail",
  expires_at: "2099-01-19T12:00:00Z",
};

/** Deterministic, browser-visible outcomes for the cookie-backed session read. */
export type MailSessionFixtureResponse =
  | { readonly kind: "authenticated"; readonly session?: HumanSessionResponse }
  | { readonly kind: "signed-out" }
  | { readonly kind: "unavailable" };

/** The initial authorized-mailbox directory is either exact or unavailable. */
export type MailboxDirectoryFixtureResponse =
  | { readonly kind: "success"; readonly page: MailboxDirectoryPage }
  | { readonly kind: "unavailable" };

const EMPTY_DRAFT_PAGE: HumanDraftPage = {
  items: [],
  next_cursor: null,
  has_more: false,
};

const EMPTY_SENT_PAGE: HumanSentSubmissionPage = {
  items: [],
  next_cursor: null,
  has_more: false,
};

export const FIXTURE_SENT_STATE_PAGE: HumanSentSubmissionPage = {
  items: [{
    submission_id: MAIL_FIXTURE_IDS.sentAccepted,
    accepted_at: "2026-01-18T12:40:00Z",
    recipient_count: 2,
  }, {
    submission_id: MAIL_FIXTURE_IDS.sentQueued,
    accepted_at: "2026-01-18T12:30:00Z",
    recipient_count: 2,
    delivery: {
      observed_at: "2026-01-18T12:31:00Z",
      terminal: false,
      recipient_count: 2,
      delivered_count: 0,
      failed_count: 0,
      queued_count: 2,
      unknown_count: 0,
    },
  }, {
    submission_id: MAIL_FIXTURE_IDS.sentDelivered,
    accepted_at: "2026-01-18T12:20:00Z",
    recipient_count: 2,
    delivery: {
      observed_at: "2026-01-18T12:22:00Z",
      terminal: true,
      recipient_count: 2,
      delivered_count: 2,
      failed_count: 0,
      queued_count: 0,
      unknown_count: 0,
    },
  }, {
    submission_id: MAIL_FIXTURE_IDS.sentPartial,
    accepted_at: "2026-01-18T12:10:00Z",
    recipient_count: 2,
    delivery: {
      observed_at: "2026-01-18T12:12:00Z",
      terminal: true,
      recipient_count: 2,
      delivered_count: 1,
      failed_count: 1,
      queued_count: 0,
      unknown_count: 0,
    },
  }],
  next_cursor: null,
  has_more: false,
};

const MAIL_APP_LOCALE_STORAGE_KEY = "copilotmail.mail.locale.v1";
const JSON_HEADERS = {
  "Cache-Control": "no-store",
};

interface FixtureMutableState {
  draftVersion: number;
  draftFailuresRemaining: number;
  draftListRequestCount: number;
  draftListSuccessCount: number;
  draftRestoreRequests: ObservedDraftRestoreRequest[];
  draftSaveRequestCount: number;
  detailFailuresRemaining: number;
  detailRequestCount: number;
  inboxFailuresRemaining: number;
  inboxMailboxIds: string[];
  inboxRequestCount: number;
  mailboxDirectoryRequestCount: number;
  messageStateRequestCount: number;
  messageStates: Map<string, Pick<MessageSummary, "is_flagged" | "is_read">>;
  sentListRequestCount: number;
  sentListSuccessCount: number;
  sentFailuresRemaining: number;
  searchRequests: ObservedSearchRequest[];
  submissionRequestCount: number;
  submissionRequests: ObservedSubmissionRequest[];
  submissionTimelineIndex: number;
  sessionRequestCount: number;
}

export interface ObservedDraftRestoreRequest {
  readonly draftId: string;
  readonly expectedVersion: number;
}

export interface ObservedSearchRequest {
  readonly cursor: string | null;
  readonly pageSize: number;
  readonly text: string;
}

export interface ObservedSubmissionRequest {
  readonly draftId: string;
  readonly expectedDraftVersion: number;
  readonly idempotencyKey: string;
}

export interface MailFixtureObservedState {
  readonly draftListRequestCount: number;
  readonly draftRestoreRequests: readonly ObservedDraftRestoreRequest[];
  readonly draftSaveRequestCount: number;
  readonly detailRequestCount: number;
  readonly inboxMailboxIds: readonly string[];
  readonly inboxRequestCount: number;
  readonly mailboxDirectoryRequestCount: number;
  readonly messageStateRequestCount: number;
  readonly sentListRequestCount: number;
  readonly searchRequests: readonly ObservedSearchRequest[];
  readonly submissionRequestCount: number;
  readonly submissionRequests: readonly ObservedSubmissionRequest[];
  readonly sessionRequestCount: number;
}

export interface MailFixtureOptions {
  /** Persist the declared product locale before the application first renders. */
  readonly locale?: "en" | "zh-CN";
  /** Ordered outcomes for exact cookie-session reads; the final item repeats. */
  readonly sessionResponses?: readonly MailSessionFixtureResponse[];
  /** Override the initial authorized directory without inventing a mailbox. */
  readonly mailboxDirectoryResponse?: MailboxDirectoryFixtureResponse;
  /** Replace the server-authoritative active mailbox returned by the directory. */
  readonly mailbox?: MailboxDirectoryItem;
  /** Return an ordered multi-kind directory; every declared mailbox remains API-bound. */
  readonly mailboxes?: readonly MailboxDirectoryItem[];
  /** Replace the inbox page while retaining the same exact-response parser boundary. */
  readonly messages?: readonly MessageSummary[];
  /** Fail this many initial inbox reads before returning the configured page. */
  readonly inboxFailuresBeforeSuccess?: number;
  /** Return an empty or otherwise distinct server page for exact search requests. */
  readonly searchMessages?: readonly MessageSummary[];
  /** Fail this many initial reads of a selected message before returning detail. */
  readonly detailFailuresBeforeSuccess?: number;
  /** Sequence of metadata pages returned by successive initial draft reads. */
  readonly draftPages?: readonly HumanDraftPage[];
  /** Fail this many draft-directory reads before returning the configured pages. */
  readonly draftFailuresBeforeSuccess?: number;
  /** Sequence of exact restore outcomes; an undeclared restore remains fail-closed. */
  readonly draftRestoreOutcomes?: readonly ("conflict" | HumanDraftRestore)[];
  /** Sequence of metadata pages returned by successive sent-directory reads. */
  readonly sentPages?: readonly HumanSentSubmissionPage[];
  /** Fail this many sent-directory reads before returning the configured pages. */
  readonly sentFailuresBeforeSuccess?: number;
  /** Simulate a request whose server outcome cannot be observed by the browser. */
  readonly submissionOutcome?: "accepted" | "network-unknown";
  /** Exact POST response followed by each status-poll response. */
  readonly submissionTimeline?: readonly MailSubmission[];
}

interface ResolvedMailFixtureOptions {
  readonly locale: "en" | "zh-CN";
  readonly sessionResponses: readonly MailSessionFixtureResponse[];
  readonly mailboxDirectoryResponse?: MailboxDirectoryFixtureResponse;
  readonly mailbox: MailboxDirectoryItem;
  readonly mailboxes: readonly MailboxDirectoryItem[];
  readonly messages: readonly MessageSummary[];
  readonly inboxFailuresBeforeSuccess: number;
  readonly searchMessages: readonly MessageSummary[];
  readonly detailFailuresBeforeSuccess: number;
  readonly draftPages: readonly HumanDraftPage[];
  readonly draftFailuresBeforeSuccess: number;
  readonly draftRestoreOutcomes: readonly ("conflict" | HumanDraftRestore)[];
  readonly sentPages: readonly HumanSentSubmissionPage[];
  readonly sentFailuresBeforeSuccess: number;
  readonly submissionOutcome: "accepted" | "network-unknown";
  readonly submissionTimeline: readonly MailSubmission[];
}

export interface MailFixtureState {
  readonly unexpectedApiRequests: string[];
  readonly state: Readonly<MailFixtureObservedState>;
}

export interface MailApp {
  readonly page: Page;
  readonly fixture: MailFixtureState;
  /** Navigate with the local fixtures without assuming a usable workspace. */
  open(): Promise<void>;
  /** Navigate only after routes and the default authenticated fixture are ready. */
  goto(): Promise<void>;
}

interface MailAppFixtures {
  mailApp: MailApp;
  mailFixtureOptions: MailFixtureOptions;
}

function resolveFixtureOptions(options: MailFixtureOptions): ResolvedMailFixtureOptions {
  const cloneDirectoryPage = (page: MailboxDirectoryPage): MailboxDirectoryPage => ({
    items: page.items.map((mailbox) => ({ ...mailbox })),
    next_cursor: page.next_cursor,
    has_more: page.has_more,
  });
  const sessionResponses = options.sessionResponses?.length
    ? options.sessionResponses
    : [{ kind: "authenticated" } satisfies MailSessionFixtureResponse];
  const mailboxDirectoryResponse = options.mailboxDirectoryResponse?.kind === "success"
    ? {
        kind: "success" as const,
        page: cloneDirectoryPage(options.mailboxDirectoryResponse.page),
      }
    : options.mailboxDirectoryResponse;
  const cloneMessages = (messages: readonly MessageSummary[]) => messages.map((message) => ({
    ...message,
    from: message.from.map((address) => ({ ...address })),
  }));
  const mailboxes = (options.mailboxes?.length
    ? options.mailboxes
    : [options.mailbox ?? AUTHORIZED_HUMAN_MAILBOX]).map((mailbox) => ({ ...mailbox }));
  const mailbox = mailboxes.find((candidate) => candidate.kind === "human") ?? mailboxes[0];
  if (!mailbox) throw new Error("Mail fixture requires at least one mailbox.");
  return {
    locale: options.locale ?? "en",
    sessionResponses: sessionResponses.map((response) => response.kind === "authenticated"
      ? { kind: "authenticated", session: { ...(response.session ?? MAIL_SESSION) } }
      : { ...response }),
    mailboxDirectoryResponse,
    mailbox,
    mailboxes,
    messages: cloneMessages(options.messages ?? [FIXTURE_MESSAGE]),
    inboxFailuresBeforeSuccess: Math.max(0, options.inboxFailuresBeforeSuccess ?? 0),
    searchMessages: cloneMessages(options.searchMessages ?? options.messages ?? [FIXTURE_MESSAGE]),
    detailFailuresBeforeSuccess: Math.max(0, options.detailFailuresBeforeSuccess ?? 0),
    draftPages: (options.draftPages ?? [EMPTY_DRAFT_PAGE]).map((page) => ({
      ...page,
      items: page.items.map((item) => ({ ...item })),
    })),
    draftFailuresBeforeSuccess: Math.max(0, options.draftFailuresBeforeSuccess ?? 0),
    draftRestoreOutcomes: (options.draftRestoreOutcomes ?? []).map((outcome) =>
      outcome === "conflict"
        ? outcome
        : {
            ...outcome,
            to: [...outcome.to],
            cc: [...outcome.cc],
            bcc: [...outcome.bcc],
          }),
    sentPages: (options.sentPages ?? [EMPTY_SENT_PAGE]).map((page) => ({
      ...page,
      items: page.items.map((item) => ({
        ...item,
        ...(item.delivery ? { delivery: { ...item.delivery } } : {}),
      })),
    })),
    sentFailuresBeforeSuccess: Math.max(0, options.sentFailuresBeforeSuccess ?? 0),
    submissionOutcome: options.submissionOutcome ?? "accepted",
    submissionTimeline: (options.submissionTimeline ?? []).map((state) => ({
      ...state,
      delivery: state.delivery
        ? {
            ...state.delivery,
            recipients: state.delivery.recipients.map((recipient) => ({ ...recipient })),
          }
        : null,
    })),
  };
}

function currentMessageState(state: FixtureMutableState, messageId: string) {
  const current = state.messageStates.get(messageId);
  if (!current) throw new Error(`fixture_message_state_missing:${messageId}`);
  return current;
}

function cloneSummary(
  state: FixtureMutableState,
  message: MessageSummary,
): MessageSummary {
  const current = currentMessageState(state, message.message_id);
  return {
    ...message,
    from: message.from.map((address) => ({ ...address })),
    is_flagged: current.is_flagged,
    is_read: current.is_read,
  };
}

function inboxPage(
  state: FixtureMutableState,
  options: ResolvedMailFixtureOptions,
): InboxPage {
  return {
    items: options.messages.map((message) => cloneSummary(state, message)),
    next_cursor: null,
    has_more: false,
  };
}

function searchPage(
  state: FixtureMutableState,
  options: ResolvedMailFixtureOptions,
): InboxPage {
  return {
    items: options.searchMessages.map((message) => cloneSummary(state, message)),
    next_cursor: null,
    has_more: false,
  };
}

function messageDetail(
  state: FixtureMutableState,
  message: MessageSummary,
): MessageDetail {
  return {
    ...FIXTURE_MESSAGE_DETAIL,
    message: cloneSummary(state, message),
    reply_to_message_id: message.message_id,
    to: FIXTURE_MESSAGE_DETAIL.to.map((address) => ({ ...address })),
    cc: FIXTURE_MESSAGE_DETAIL.cc.map((address) => ({ ...address })),
    attachments: FIXTURE_MESSAGE_DETAIL.attachments.map((attachment) => ({ ...attachment })),
  };
}

function responseMessageState(
  state: FixtureMutableState,
  messageId: string,
): MessageState {
  const current = currentMessageState(state, messageId);
  return {
    message_id: messageId,
    is_read: current.is_read,
    is_flagged: current.is_flagged,
  };
}

function submission(
  state: FixtureMutableState,
  mailboxId: string,
): MailSubmission {
  const draftVersion = Math.max(state.draftVersion, 1);
  return {
    submission_id: MAIL_FIXTURE_IDS.submission,
    mailbox_id: mailboxId,
    draft_id: MAIL_FIXTURE_IDS.draft,
    draft_version: draftVersion,
    status: "accepted",
    state_version: 1,
    created_at: "2026-01-18T12:10:00Z",
    updated_at: "2026-01-18T12:10:01Z",
    delivery: {
      observed_at: "2026-01-18T12:10:02Z",
      terminal: true,
      recipient_count: 1,
      delivered_count: 1,
      failed_count: 0,
      queued_count: 0,
      unknown_count: 0,
      recipients: [{
        recipient_ref: "4db054bef65528cf7d9df038af553c8211362e2e6d9ba7e7f0689535958081a8",
        status: "delivered",
        smtp_reply: "250 2.0.0 accepted by recipient server",
      }],
    },
  };
}

function nextSubmission(
  mutable: FixtureMutableState,
  options: ResolvedMailFixtureOptions,
  mailboxId: string,
): MailSubmission {
  if (options.submissionTimeline.length === 0) {
    return submission(mutable, mailboxId);
  }
  const configured = options.submissionTimeline[
    Math.min(mutable.submissionTimelineIndex, options.submissionTimeline.length - 1)
  ];
  if (!configured) throw new Error("Mail fixture submission timeline cannot contain gaps.");
  mutable.submissionTimelineIndex += 1;
  return {
    ...configured,
    delivery: configured.delivery
      ? {
          ...configured.delivery,
          recipients: configured.delivery.recipients.map((recipient) => ({ ...recipient })),
        }
      : null,
  };
}

function isExactQuery(url: URL, expected: Record<string, string>): boolean {
  const expectedEntries = Object.entries(expected);
  if (Array.from(url.searchParams.entries()).length !== expectedEntries.length) return false;
  return expectedEntries.every(([key, value]) => {
    const values = url.searchParams.getAll(key);
    return values.length === 1 && values[0] === value;
  });
}

function jsonRecord(value: string | null): Record<string, unknown> | undefined {
  if (!value) return undefined;
  try {
    const parsed: unknown = JSON.parse(value);
    return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : undefined;
  } catch {
    return undefined;
  }
}

async function fulfillJson(route: Route, body: unknown, status = 200) {
  await route.fulfill({
    status,
    contentType: "application/json; charset=utf-8",
    headers: JSON_HEADERS,
    body: JSON.stringify(body),
  });
}

function describeRequest(route: Route) {
  const request = route.request();
  const url = new URL(request.url());
  return `${request.method()} ${url.pathname}${url.search}`;
}

async function rejectUnexpectedApiRequest(route: Route, unexpectedApiRequests: string[]) {
  unexpectedApiRequests.push(describeRequest(route));
  // Aborting prevents a fixture gap from falling through to a real BFF, IdP,
  // database, or network service. Fixture teardown converts the record into a
  // direct test failure with the exact request shape.
  await route.abort("blockedbyclient");
}

async function handleMailApiRequest(
  route: Route,
  mutable: FixtureMutableState,
  unexpectedApiRequests: string[],
  options: ResolvedMailFixtureOptions,
) {
  const request = route.request();
  const url = new URL(request.url());
  const method = request.method();
  const path = url.pathname;
  const mailboxPathMatch = /^\/api\/v2\/mailboxes\/([^/]+)/u.exec(path);
  const requestMailbox = mailboxPathMatch
    ? options.mailboxes.find((mailbox) => mailbox.mailbox_id === mailboxPathMatch[1])
    : undefined;
  const mailboxPrefix = requestMailbox
    ? `/api/v2/mailboxes/${requestMailbox.mailbox_id}`
    : "/api/v2/mailboxes/__undeclared_mailbox__";

  if (method === "GET" && path === "/api/v2/auth/mail/session" && !url.search) {
    const response = options.sessionResponses[Math.min(
      mutable.sessionRequestCount,
      options.sessionResponses.length - 1,
    )];
    mutable.sessionRequestCount += 1;
    if (!response) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    if (response.kind === "signed-out") {
      await fulfillJson(route, { error: "fixture_signed_out" }, 401);
      return;
    }
    if (response.kind === "unavailable") {
      await fulfillJson(route, { error: "fixture_session_unavailable" }, 503);
      return;
    }
    await fulfillJson(route, response.session ?? MAIL_SESSION);
    return;
  }

  if (method === "POST" && path === "/api/v2/auth/mail/logout" && !url.search) {
    await route.fulfill({ status: 204, headers: JSON_HEADERS });
    return;
  }

  if (
    method === "GET"
    && path === "/api/v2/mailboxes"
    && isExactQuery(url, { limit: "50" })
  ) {
    mutable.mailboxDirectoryRequestCount += 1;
    if (options.mailboxDirectoryResponse?.kind === "unavailable") {
      await fulfillJson(route, { error: "fixture_mailbox_directory_unavailable" }, 503);
      return;
    }
    const directory: MailboxDirectoryPage = options.mailboxDirectoryResponse?.kind === "success"
      ? {
          items: options.mailboxDirectoryResponse.page.items.map((mailbox) => ({ ...mailbox })),
          next_cursor: options.mailboxDirectoryResponse.page.next_cursor,
          has_more: options.mailboxDirectoryResponse.page.has_more,
        }
      : {
          items: options.mailboxes.map((mailbox) => ({ ...mailbox })),
          next_cursor: null,
          has_more: false,
        };
    await fulfillJson(route, directory);
    return;
  }

  if (
    method === "GET"
    && path === `${mailboxPrefix}/messages`
    && isExactQuery(url, { page_size: "30" })
  ) {
    if (!requestMailbox) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    mutable.inboxRequestCount += 1;
    mutable.inboxMailboxIds.push(requestMailbox.mailbox_id);
    if (mutable.inboxFailuresRemaining > 0) {
      mutable.inboxFailuresRemaining -= 1;
      await fulfillJson(route, { error: "fixture_inbox_upstream_failure" }, 503);
      return;
    }
    await fulfillJson(route, inboxPage(mutable, options));
    return;
  }

  if (method === "POST" && path === `${mailboxPrefix}/messages/search` && !url.search) {
    const body = jsonRecord(request.postData());
    const bodyKeys = body ? Object.keys(body).sort() : [];
    if (
      !body
      || bodyKeys.join("|") !== "cursor|page_size|text"
      || typeof body.text !== "string"
      || (body.cursor !== null && typeof body.cursor !== "string")
      || body.page_size !== 30
    ) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    mutable.searchRequests.push({
      cursor: body.cursor as string | null,
      pageSize: body.page_size as number,
      text: body.text,
    });
    await fulfillJson(route, searchPage(mutable, options));
    return;
  }

  const messagePathPrefix = `${mailboxPrefix}/messages/`;
  const messageId = path.startsWith(messagePathPrefix)
    ? path.slice(messagePathPrefix.length)
    : undefined;
  const configuredMessage = messageId
    ? options.messages.find((message) => message.message_id === messageId)
    : undefined;

  if (
    method === "GET"
    && configuredMessage
    && !url.search
  ) {
    mutable.detailRequestCount += 1;
    if (mutable.detailFailuresRemaining > 0) {
      mutable.detailFailuresRemaining -= 1;
      await fulfillJson(route, { error: "fixture_message_store_unavailable" }, 503);
      return;
    }
    await fulfillJson(route, messageDetail(mutable, configuredMessage));
    return;
  }

  if (method === "PATCH" && configuredMessage && messageId && !url.search) {
    mutable.messageStateRequestCount += 1;
    const body = jsonRecord(request.postData());
    const entries = body ? Object.entries(body) : [];
    const validUpdate = entries.length === 1
      && ((typeof body?.is_read === "boolean" && entries[0]?.[0] === "is_read")
        || (typeof body?.is_flagged === "boolean" && entries[0]?.[0] === "is_flagged"));
    if (!validUpdate || !body) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    const current = currentMessageState(mutable, messageId);
    if (typeof body.is_read === "boolean") current.is_read = body.is_read;
    if (typeof body.is_flagged === "boolean") current.is_flagged = body.is_flagged;
    await fulfillJson(route, responseMessageState(mutable, messageId));
    return;
  }

  if (
    method === "GET"
    && path === `${mailboxPrefix}/drafts`
    && isExactQuery(url, { limit: "50" })
  ) {
    mutable.draftListRequestCount += 1;
    if (mutable.draftFailuresRemaining > 0) {
      mutable.draftFailuresRemaining -= 1;
      await fulfillJson(route, { error: "fixture_draft_directory_unavailable" }, 503);
      return;
    }
    const pageIndex = Math.min(
      mutable.draftListSuccessCount,
      Math.max(0, options.draftPages.length - 1),
    );
    mutable.draftListSuccessCount += 1;
    await fulfillJson(route, options.draftPages[pageIndex] ?? EMPTY_DRAFT_PAGE);
    return;
  }

  const draftPathPrefix = `${mailboxPrefix}/drafts/`;
  const restoreSuffix = "/restore";
  const restoredDraftId = path.startsWith(draftPathPrefix) && path.endsWith(restoreSuffix)
    ? path.slice(draftPathPrefix.length, -restoreSuffix.length)
    : undefined;
  if (method === "POST" && restoredDraftId && !url.search) {
    const body = jsonRecord(request.postData());
    const bodyEntries = body ? Object.entries(body) : [];
    const expectedVersion = body?.expected_version;
    const pageIndex = Math.min(
      Math.max(0, mutable.draftListSuccessCount - 1),
      Math.max(0, options.draftPages.length - 1),
    );
    const currentDraft = (options.draftPages[pageIndex] ?? EMPTY_DRAFT_PAGE).items
      .find((draft) => draft.draft_id === restoredDraftId);
    const outcome = options.draftRestoreOutcomes[mutable.draftRestoreRequests.length];
    if (
      bodyEntries.length !== 1
      || bodyEntries[0]?.[0] !== "expected_version"
      || !Number.isSafeInteger(expectedVersion)
      || Number(expectedVersion) <= 0
      || !currentDraft
      || expectedVersion !== currentDraft.version
      || outcome === undefined
    ) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    mutable.draftRestoreRequests.push({
      draftId: restoredDraftId,
      expectedVersion: Number(expectedVersion),
    });
    if (outcome === "conflict") {
      await fulfillJson(route, { error: "draft_version_conflict" }, 409);
      return;
    }
    if (outcome.draft_id !== restoredDraftId || outcome.version !== expectedVersion) {
      unexpectedApiRequests.push(
        `fixture restore response mismatch ${restoredDraftId}@${String(expectedVersion)}`,
      );
      await route.abort("blockedbyclient");
      return;
    }
    await fulfillJson(route, outcome);
    return;
  }

  if (method === "POST" && path === `${mailboxPrefix}/drafts` && !url.search) {
    const body = jsonRecord(request.postData());
    const replyTo = body?.reply_to_message_id;
    if (!body || (replyTo !== null && typeof replyTo !== "string")) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    mutable.draftSaveRequestCount += 1;
    mutable.draftVersion += 1;
    const result: DraftResult = {
      draft_id: MAIL_FIXTURE_IDS.draft,
      version: mutable.draftVersion,
      reply_to_message_id: replyTo,
      previous_draft_destroyed: true,
    };
    await fulfillJson(route, result);
    return;
  }

  if (
    method === "GET"
    && path === `${mailboxPrefix}/submissions`
    && isExactQuery(url, { limit: "50" })
  ) {
    mutable.sentListRequestCount += 1;
    if (mutable.sentFailuresRemaining > 0) {
      mutable.sentFailuresRemaining -= 1;
      await fulfillJson(route, { error: "fixture_sent_directory_unavailable" }, 503);
      return;
    }
    const pageIndex = Math.min(
      mutable.sentListSuccessCount,
      Math.max(0, options.sentPages.length - 1),
    );
    mutable.sentListSuccessCount += 1;
    await fulfillJson(route, options.sentPages[pageIndex] ?? EMPTY_SENT_PAGE);
    return;
  }

  if (method === "POST" && path === `${mailboxPrefix}/submissions` && !url.search) {
    const body = jsonRecord(request.postData());
    const idempotencyKey = await request.headerValue("Idempotency-Key");
    if (
      !body
      || body.draft_id !== MAIL_FIXTURE_IDS.draft
      || body.expected_draft_version !== Math.max(mutable.draftVersion, 1)
      || !idempotencyKey
    ) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    mutable.submissionRequestCount += 1;
    mutable.submissionRequests.push({
      draftId: String(body.draft_id),
      expectedDraftVersion: Number(body.expected_draft_version),
      idempotencyKey,
    });
    if (options.submissionOutcome === "network-unknown") {
      // The request matched the exact local contract, but the browser cannot
      // observe whether the server accepted it. This is an expected transport
      // ambiguity, not permission to let the request escape the fixture.
      await route.abort("connectionreset");
      return;
    }
    if (!requestMailbox) {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    await fulfillJson(route, nextSubmission(mutable, options, requestMailbox.mailbox_id));
    return;
  }

  if (
    method === "GET"
    && path === `/api/v2/mail-submissions/${MAIL_FIXTURE_IDS.submission}`
    && !url.search
  ) {
    await fulfillJson(route, nextSubmission(mutable, options, options.mailbox.mailbox_id));
    return;
  }

  await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
}

export async function installMailRoutes(
  page: Page,
  allowedOrigin: string,
  fixtureOptions: MailFixtureOptions = {},
): Promise<MailFixtureState> {
  const options = resolveFixtureOptions(fixtureOptions);
  const mutable: FixtureMutableState = {
    draftVersion: 0,
    draftFailuresRemaining: options.draftFailuresBeforeSuccess,
    draftListRequestCount: 0,
    draftListSuccessCount: 0,
    draftRestoreRequests: [],
    draftSaveRequestCount: 0,
    detailFailuresRemaining: options.detailFailuresBeforeSuccess,
    detailRequestCount: 0,
    inboxFailuresRemaining: options.inboxFailuresBeforeSuccess,
    inboxMailboxIds: [],
    inboxRequestCount: 0,
    mailboxDirectoryRequestCount: 0,
    messageStateRequestCount: 0,
    messageStates: new Map([...options.messages, ...options.searchMessages].map((message) => [
      message.message_id,
      { is_flagged: message.is_flagged, is_read: message.is_read },
    ])),
    searchRequests: [],
    sentListRequestCount: 0,
    sentListSuccessCount: 0,
    sentFailuresRemaining: options.sentFailuresBeforeSuccess,
    submissionRequestCount: 0,
    submissionRequests: [],
    submissionTimelineIndex: 0,
    sessionRequestCount: 0,
  };
  const unexpectedApiRequests: string[] = [];

  await page.addInitScript((args: { locale: string; localeKey: string }) => {
    window.localStorage.setItem(args.localeKey, args.locale);
  }, { localeKey: MAIL_APP_LOCALE_STORAGE_KEY, locale: options.locale });
  // Later, more specific route handlers own declared local APIs. This first
  // catch-all permits only the preview origin and blocks any future IdP, CDN,
  // telemetry, or other network dependency before it can leave the machine.
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
    if (route.request().method() !== "GET") {
      await rejectUnexpectedApiRequest(route, unexpectedApiRequests);
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/javascript; charset=utf-8",
      headers: JSON_HEADERS,
      body: "window.__COPILOTMAIL_MAIL_CONFIG__ = { apiBaseUrl: \"\" };",
    });
  });
  await page.route("**/api/**", (route) =>
    handleMailApiRequest(route, mutable, unexpectedApiRequests, options));

  return {
    unexpectedApiRequests,
    state: mutable,
  };
}

function assertMailBaseUrl(baseURL: string | undefined): URL {
  if (!baseURL) {
    throw new Error(
      "Mail browser tests require a Playwright baseURL that serves the built Mail application.",
    );
  }
  return new URL(baseURL);
}

export const test = base.extend<MailAppFixtures>({
  mailFixtureOptions: [{}, { option: true }],
  mailApp: async ({ baseURL, mailFixtureOptions, page }, use) => {
    const target = assertMailBaseUrl(baseURL);
    const fixture = await installMailRoutes(page, target.origin, mailFixtureOptions);
    const open = async () => {
      await page.goto("/", { waitUntil: "domcontentloaded" });
    };
    const app: MailApp = {
      page,
      fixture,
      open,
      goto: async () => {
        await open();
        await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
        await expect(page.getByRole("textbox", { name: "Search email", exact: true }))
          .toBeVisible();
      },
    };

    await use(app);

    if (fixture.unexpectedApiRequests.length > 0) {
      throw new Error(
        `Unexpected Mail API request(s): ${fixture.unexpectedApiRequests.join(", ")}`,
      );
    }
  },
});

export { expect };
