import { sameOriginApiBaseUrl } from "./config";
import type {
  DraftInput,
  DraftResult,
  HumanDraftPage,
  HumanDraftRestore,
  HumanSentDeliverySummary,
  HumanSentSubmissionListItem,
  HumanSentSubmissionPage,
  InboxPage,
  MailboxDirectoryPage,
  MailboxKind,
  MailboxScopedConfig,
  MessageDetail,
  MessageState,
  MailDeliveryRecipient,
  MailSubmission,
  RuntimeConfig,
} from "./types";

function endpoint(config: RuntimeConfig, path: string): string {
  return `${sameOriginApiBaseUrl(config.apiBaseUrl)}${path}`;
}

const cookieRequest = {
  credentials: "same-origin" as const,
  cache: "no-store" as const,
  headers: { Accept: "application/json" },
};

const uuidPattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const canonicalUuidPattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const mailboxDirectoryCursorPattern =
  /^v1\.[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const humanDraftCursorPattern = mailboxDirectoryCursorPattern;
const humanSentSubmissionCursorPattern = mailboxDirectoryCursorPattern;
const rfc3339Pattern = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|[+-](\d{2}):(\d{2}))$/;
const rfc3339FractionPattern = /\.(\d{1,9})(?:Z|[+-]\d{2}:\d{2})$/;
const utf8 = new TextEncoder();

const inboxPageKeys = ["items", "next_cursor", "has_more"] as const;
const mailboxDirectoryPageKeys = ["items", "next_cursor", "has_more"] as const;
const mailboxDirectoryItemKeys = [
  "mailbox_id",
  "kind",
  "primary_address",
  "can_send",
] as const;
const messageSummaryKeys = [
  "message_id",
  "subject",
  "from",
  "received_at",
  "preview",
  "is_read",
  "is_flagged",
  "has_attachment",
] as const;
const addressKeys = ["name", "email"] as const;
const messageDetailKeys = [
  "message",
  "reply_to_message_id",
  "to",
  "cc",
  "text_body",
  "text_body_truncated",
  "attachments",
] as const;
const attachmentKeys = ["name", "media_type", "size_bytes"] as const;
const messageStateKeys = ["message_id", "is_read", "is_flagged"] as const;
const humanDraftPageKeys = ["items", "next_cursor", "has_more"] as const;
const humanDraftListItemKeys = [
  "draft_id",
  "version",
  "reply_to_message_id",
  "recipient_count",
  "created_at",
  "updated_at",
] as const;
const humanDraftRestoreKeys = [
  "draft_id",
  "version",
  "reply_to_message_id",
  "to",
  "cc",
  "bcc",
  "subject",
  "text_body",
] as const;
const humanSentSubmissionPageKeys = ["items", "next_cursor", "has_more"] as const;
const humanSentSubmissionListItemRequiredKeys = [
  "submission_id",
  "accepted_at",
  "recipient_count",
] as const;
const humanSentSubmissionListItemAllowedKeys = [
  ...humanSentSubmissionListItemRequiredKeys,
  "delivery",
] as const;
const humanSentDeliverySummaryKeys = [
  "observed_at",
  "terminal",
  "recipient_count",
  "delivered_count",
  "failed_count",
  "queued_count",
  "unknown_count",
] as const;
const draftInputKeys = [
  "draft_id",
  "expected_version",
  "reply_to_message_id",
  "to",
  "cc",
  "bcc",
  "subject",
  "text_body",
] as const;
const draftResultKeys = [
  "draft_id",
  "version",
  "reply_to_message_id",
  "previous_draft_destroyed",
] as const;
const mailSubmissionKeys = [
  "submission_id",
  "mailbox_id",
  "draft_id",
  "draft_version",
  "status",
  "state_version",
  "created_at",
  "updated_at",
  "delivery",
] as const;
const mailDeliveryKeys = [
  "observed_at",
  "terminal",
  "recipient_count",
  "delivered_count",
  "failed_count",
  "queued_count",
  "unknown_count",
  "recipients",
] as const;
const mailDeliveryRecipientKeys = ["recipient_ref", "status", "smtp_reply"] as const;
const idempotencyKeyPattern = /^[A-Za-z0-9._-]{22,200}$/;
const recipientReferencePattern = /^[0-9a-f]{64}$/;

function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function exactRecord(
  value: unknown,
  allowedKeys: readonly string[],
  errorCode: string,
): Record<string, unknown> {
  const valueRecord = record(value);
  const allowed = new Set(allowedKeys);
  if (
    !valueRecord
    || Object.keys(valueRecord).length !== allowedKeys.length
    || Object.keys(valueRecord).some((key) => !allowed.has(key))
  ) {
    throw new Error(errorCode);
  }
  return valueRecord;
}

function recordWithRequiredKeys(
  value: unknown,
  requiredKeys: readonly string[],
  allowedKeys: readonly string[],
  errorCode: string,
): Record<string, unknown> {
  const valueRecord = record(value);
  const allowed = new Set(allowedKeys);
  if (
    !valueRecord
    || Object.keys(valueRecord).some((key) => !allowed.has(key))
    || requiredKeys.some((key) => !Object.prototype.hasOwnProperty.call(valueRecord, key))
  ) {
    throw new Error(errorCode);
  }
  return valueRecord;
}

function boundedString(
  value: unknown,
  minimumBytes: number,
  maximumBytes: number,
  errorCode: string,
): string {
  if (typeof value !== "string") throw new Error(errorCode);
  const length = utf8.encode(value).byteLength;
  if (length < minimumBytes || length > maximumBytes) throw new Error(errorCode);
  return value;
}

function booleanValue(value: unknown, errorCode: string): boolean {
  if (typeof value !== "boolean") throw new Error(errorCode);
  return value;
}

function positiveSafeInteger(
  value: unknown,
  maximum: number,
  errorCode: string,
): number {
  if (
    typeof value !== "number"
    || !Number.isSafeInteger(value)
    || value < 1
    || value > maximum
  ) throw new Error(errorCode);
  return value;
}

function nonNegativeSafeInteger(
  value: unknown,
  maximum: number,
  errorCode: string,
): number {
  if (
    typeof value !== "number"
    || !Number.isSafeInteger(value)
    || value < 0
    || value > maximum
  ) throw new Error(errorCode);
  return value;
}

function boundedArray(
  value: unknown,
  maximumItems: number,
  errorCode: string,
): unknown[] {
  if (!Array.isArray(value) || value.length > maximumItems) {
    throw new Error(errorCode);
  }
  return value;
}

function messageUuid(value: unknown, errorCode: string): string {
  if (typeof value !== "string" || !uuidPattern.test(value)) {
    throw new Error(errorCode);
  }
  return value;
}

function canonicalUuid(value: unknown, errorCode: string): string {
  if (typeof value !== "string" || !canonicalUuidPattern.test(value)) {
    throw new Error(errorCode);
  }
  return value;
}

function mailboxKind(value: unknown, errorCode: string): MailboxKind {
  if (value === "human" || value === "agent" || value === "shared" || value === "group") {
    return value;
  }
  throw new Error(errorCode);
}

function mailboxAddress(value: unknown, errorCode: string): string {
  const address = boundedString(value, 3, 254, errorCode);
  if (address !== address.toLowerCase() || address.split("@").length !== 2) {
    throw new Error(errorCode);
  }
  const [local, domain] = address.split("@");
  const validLocal = local !== undefined
    && local.length > 0
    && local.length <= 64
    && !local.startsWith(".")
    && !local.endsWith(".")
    && !local.includes("..")
    && /^[A-Za-z0-9.!#$%&'*+\-/=?^_`{|}~]+$/.test(local);
  const labels = domain?.split(".") ?? [];
  const validDomain = domain !== undefined
    && domain.length <= 253
    && labels.length >= 2
    && labels.every((label) => label.length > 0
      && label.length <= 63
      && !label.startsWith("-")
      && !label.endsWith("-")
      && /^[A-Za-z0-9-]+$/.test(label));
  if (
    !validLocal
    || !validDomain
  ) throw new Error(errorCode);
  return address;
}

function parseMailboxDirectoryPage(
  value: unknown,
  requestCursor?: string,
): MailboxDirectoryPage {
  const errorCode = "mailbox_directory_response_invalid";
  const page = exactRecord(value, mailboxDirectoryPageKeys, errorCode);
  const items = boundedArray(page.items, 50, errorCode).map((value) => {
    const item = exactRecord(value, mailboxDirectoryItemKeys, errorCode);
    return {
      mailbox_id: canonicalUuid(item.mailbox_id, errorCode),
      kind: mailboxKind(item.kind, errorCode),
      primary_address: mailboxAddress(item.primary_address, errorCode),
      can_send: booleanValue(item.can_send, errorCode),
    };
  });
  for (let index = 1; index < items.length; index += 1) {
    if (items[index - 1].mailbox_id >= items[index].mailbox_id) {
      throw new Error(errorCode);
    }
  }
  const requestAnchor = requestCursor?.slice(3);
  if (requestAnchor && items.some((item) => item.mailbox_id <= requestAnchor)) {
    throw new Error(errorCode);
  }
  const nextCursor = page.next_cursor === null
    ? null
    : boundedString(page.next_cursor, 1, 2048, errorCode);
  if (nextCursor !== null && !mailboxDirectoryCursorPattern.test(nextCursor)) {
    throw new Error(errorCode);
  }
  const hasMore = booleanValue(page.has_more, errorCode);
  if (hasMore !== (nextCursor !== null)) throw new Error(errorCode);
  if (
    hasMore
    && (items.length === 0 || nextCursor !== `v1.${items.at(-1)?.mailbox_id}`)
  ) throw new Error(errorCode);
  if (requestCursor && nextCursor !== null && nextCursor <= requestCursor) {
    throw new Error(errorCode);
  }
  return { items, next_cursor: nextCursor, has_more: hasMore };
}

function dateTime(value: unknown, errorCode: string): string {
  const result = boundedString(value, 1, 64, errorCode);
  if (!Number.isFinite(Date.parse(result))) {
    throw new Error(errorCode);
  }
  return result;
}

function parseAddress(value: unknown, errorCode: string) {
  const address = exactRecord(value, addressKeys, errorCode);
  const name = address.name === null
    ? null
    : boundedString(address.name, 0, 512, errorCode);
  return {
    name,
    email: boundedString(address.email, 1, 320, errorCode),
  };
}

function parseAddresses(
  value: unknown,
  maximumItems: number,
  errorCode: string,
) {
  return boundedArray(value, maximumItems, errorCode)
    .map((address) => parseAddress(address, errorCode));
}

function parseMessageSummary(
  value: unknown,
  errorCode: string,
) {
  const summary = exactRecord(value, messageSummaryKeys, errorCode);
  return {
    message_id: messageUuid(summary.message_id, errorCode),
    subject: boundedString(summary.subject, 0, 998, errorCode),
    from: parseAddresses(summary.from, 128, errorCode),
    received_at: dateTime(summary.received_at, errorCode),
    preview: boundedString(summary.preview, 0, 4096, errorCode),
    is_read: booleanValue(summary.is_read, errorCode),
    is_flagged: booleanValue(summary.is_flagged, errorCode),
    has_attachment: booleanValue(summary.has_attachment, errorCode),
  };
}

function parseInboxPage(value: unknown): InboxPage {
  const errorCode = "inbox_response_invalid";
  const page = exactRecord(value, inboxPageKeys, errorCode);
  const items = boundedArray(page.items, 50, errorCode)
    .map((item) => parseMessageSummary(item, "inbox_message_id_invalid"));
  const uniqueMessageIds = new Set(items.map((item) => item.message_id));
  if (uniqueMessageIds.size !== items.length) throw new Error(errorCode);
  const nextCursor = page.next_cursor === null
    ? null
    : boundedString(page.next_cursor, 1, 2048, errorCode);
  const hasMore = booleanValue(page.has_more, errorCode);
  if (hasMore !== (nextCursor !== null)) {
    throw new Error(errorCode);
  }
  return {
    items,
    next_cursor: nextCursor,
    has_more: hasMore,
  };
}

function parseAttachment(value: unknown, errorCode: string) {
  const attachment = exactRecord(value, attachmentKeys, errorCode);
  const name = attachment.name === null
    ? null
    : boundedString(attachment.name, 0, 1024, errorCode);
  if (
    typeof attachment.size_bytes !== "number"
    || !Number.isSafeInteger(attachment.size_bytes)
    || attachment.size_bytes < 0
  ) {
    throw new Error(errorCode);
  }
  return {
    name,
    media_type: boundedString(attachment.media_type, 1, 255, errorCode),
    size_bytes: attachment.size_bytes,
  };
}

function parseMessageDetail(value: unknown, expectedMessageId: string): MessageDetail {
  const errorCode = "message_response_invalid";
  const detail = exactRecord(value, messageDetailKeys, errorCode);
  const message = parseMessageSummary(
    detail.message,
    "message_response_id_invalid",
  );
  if (message.message_id !== expectedMessageId) {
    throw new Error("message_response_id_mismatch");
  }
  let replyMessageId: string | null = null;
  if (detail.reply_to_message_id !== null) {
    replyMessageId = messageUuid(
      detail.reply_to_message_id,
      "message_reply_id_invalid",
    );
    if (replyMessageId !== expectedMessageId) {
      throw new Error("message_reply_id_mismatch");
    }
  }
  return {
    message,
    reply_to_message_id: replyMessageId,
    to: parseAddresses(detail.to, 256, errorCode),
    cc: parseAddresses(detail.cc, 256, errorCode),
    text_body: boundedString(detail.text_body, 0, 256 * 1024, errorCode),
    text_body_truncated: booleanValue(detail.text_body_truncated, errorCode),
    attachments: boundedArray(detail.attachments, 256, errorCode)
      .map((attachment) => parseAttachment(attachment, errorCode)),
  };
}

function parseMessageState(value: unknown, expectedMessageId: string): MessageState {
  const errorCode = "message_state_response_invalid";
  const state = exactRecord(value, messageStateKeys, errorCode);
  const responseMessageId = messageUuid(
    state.message_id,
    "message_state_response_id_invalid",
  );
  if (responseMessageId !== expectedMessageId) {
    throw new Error("message_state_response_id_mismatch");
  }
  return {
    message_id: responseMessageId,
    is_read: booleanValue(state.is_read, errorCode),
    is_flagged: booleanValue(state.is_flagged, errorCode),
  };
}

function optionalCanonicalUuid(value: unknown, errorCode: string): string | null {
  return value === null ? null : canonicalUuid(value, errorCode);
}

function draftDateTime(value: unknown, errorCode: string): string {
  const result = boundedString(value, 20, 35, errorCode);
  const match = rfc3339Pattern.exec(result);
  if (!match || !Number.isFinite(Date.parse(result))) {
    throw new Error(errorCode);
  }
  const [year, month, day, hour, minute, second, offsetHour, offsetMinute] =
    match.slice(1).map((part) => part === undefined ? 0 : Number.parseInt(part, 10));
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const daysInMonth = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (
    month < 1
    || month > 12
    || day < 1
    || day > (daysInMonth[month - 1] ?? 0)
    || hour > 23
    || minute > 59
    || second > 59
    || offsetHour > 23
    || offsetMinute > 59
  ) throw new Error(errorCode);
  return result;
}

function dateTimeNanoseconds(value: string): bigint {
  const milliseconds = Date.parse(value);
  if (!Number.isSafeInteger(milliseconds)) {
    throw new Error("sent_list_response_invalid");
  }
  const fraction = rfc3339FractionPattern.exec(value)?.[1] ?? "";
  const fractionalNanoseconds = Number.parseInt(
    fraction.padEnd(9, "0"),
    10,
  );
  return BigInt(milliseconds) * 1_000_000n
    + BigInt(fractionalNanoseconds % 1_000_000);
}

/**
 * The service orders accepted submissions by accepted_at DESC then UUID DESC.
 * Retaining RFC3339 precision avoids treating two distinct micro/nanosecond
 * acceptance timestamps as a UUID-only ordering problem.
 */
export function compareHumanSentSubmissionItems(
  left: Pick<HumanSentSubmissionListItem, "submission_id" | "accepted_at">,
  right: Pick<HumanSentSubmissionListItem, "submission_id" | "accepted_at">,
): number {
  const leftTimestamp = dateTimeNanoseconds(left.accepted_at);
  const rightTimestamp = dateTimeNanoseconds(right.accepted_at);
  if (leftTimestamp !== rightTimestamp) {
    return leftTimestamp > rightTimestamp ? -1 : 1;
  }
  if (left.submission_id === right.submission_id) return 0;
  return left.submission_id > right.submission_id ? -1 : 1;
}

function parseHumanDraftPage(
  value: unknown,
  requestCursor?: string,
): HumanDraftPage {
  const errorCode = "draft_list_response_invalid";
  const page = exactRecord(value, humanDraftPageKeys, errorCode);
  const items = boundedArray(page.items, 50, errorCode).map((value) => {
    const item = exactRecord(value, humanDraftListItemKeys, errorCode);
    const createdAt = draftDateTime(item.created_at, errorCode);
    const updatedAt = draftDateTime(item.updated_at, errorCode);
    if (Date.parse(updatedAt) < Date.parse(createdAt)) throw new Error(errorCode);
    return {
      draft_id: canonicalUuid(item.draft_id, errorCode),
      version: positiveSafeInteger(item.version, Number.MAX_SAFE_INTEGER, errorCode),
      reply_to_message_id: optionalCanonicalUuid(item.reply_to_message_id, errorCode),
      recipient_count: positiveSafeInteger(item.recipient_count, 100, errorCode),
      created_at: createdAt,
      updated_at: updatedAt,
    };
  });
  for (let index = 1; index < items.length; index += 1) {
    if (items[index - 1].draft_id <= items[index].draft_id) throw new Error(errorCode);
  }
  const requestAnchor = requestCursor?.slice(3);
  if (requestAnchor && items.some((item) => item.draft_id >= requestAnchor)) {
    throw new Error(errorCode);
  }
  const nextCursor = page.next_cursor === null
    ? null
    : boundedString(page.next_cursor, 39, 39, errorCode);
  if (nextCursor !== null && !humanDraftCursorPattern.test(nextCursor)) {
    throw new Error(errorCode);
  }
  const hasMore = booleanValue(page.has_more, errorCode);
  if (hasMore !== (nextCursor !== null)) throw new Error(errorCode);
  if (
    hasMore
    && (items.length === 0 || nextCursor !== `v1.${items.at(-1)?.draft_id}`)
  ) throw new Error(errorCode);
  if (requestCursor && nextCursor !== null && nextCursor >= requestCursor) {
    throw new Error(errorCode);
  }
  return { items, next_cursor: nextCursor, has_more: hasMore };
}

function parseHumanSentDeliverySummary(
  value: unknown,
  acceptedAt: string,
): HumanSentDeliverySummary {
  const errorCode = "sent_list_response_invalid";
  const delivery = exactRecord(value, humanSentDeliverySummaryKeys, errorCode);
  const observedAt = draftDateTime(delivery.observed_at, errorCode);
  if (Date.parse(observedAt) < Date.parse(acceptedAt)) throw new Error(errorCode);
  const recipientCount = positiveSafeInteger(delivery.recipient_count, 100, errorCode);
  const deliveredCount = nonNegativeSafeInteger(delivery.delivered_count, 100, errorCode);
  const failedCount = nonNegativeSafeInteger(delivery.failed_count, 100, errorCode);
  const queuedCount = nonNegativeSafeInteger(delivery.queued_count, 100, errorCode);
  const unknownCount = nonNegativeSafeInteger(delivery.unknown_count, 100, errorCode);
  const terminal = booleanValue(delivery.terminal, errorCode);
  if (
    deliveredCount + failedCount + queuedCount + unknownCount !== recipientCount
    || terminal !== (queuedCount === 0 && unknownCount === 0)
  ) throw new Error(errorCode);
  return {
    observed_at: observedAt,
    terminal,
    recipient_count: recipientCount,
    delivered_count: deliveredCount,
    failed_count: failedCount,
    queued_count: queuedCount,
    unknown_count: unknownCount,
  };
}

function parseHumanSentSubmissionPage(
  value: unknown,
  requestCursor?: string,
): HumanSentSubmissionPage {
  const errorCode = "sent_list_response_invalid";
  const page = exactRecord(value, humanSentSubmissionPageKeys, errorCode);
  const items = boundedArray(page.items, 50, errorCode).map((value) => {
    const item = recordWithRequiredKeys(
      value,
      humanSentSubmissionListItemRequiredKeys,
      humanSentSubmissionListItemAllowedKeys,
      errorCode,
    );
    const acceptedAt = draftDateTime(item.accepted_at, errorCode);
    const recipientCount = positiveSafeInteger(item.recipient_count, 100, errorCode);
    const delivery = item.delivery === undefined
      ? undefined
      : parseHumanSentDeliverySummary(item.delivery, acceptedAt);
    if (delivery && delivery.recipient_count !== recipientCount) throw new Error(errorCode);
    return {
      submission_id: canonicalUuid(item.submission_id, errorCode),
      accepted_at: acceptedAt,
      recipient_count: recipientCount,
      ...(delivery === undefined ? {} : { delivery }),
    };
  });
  for (let index = 1; index < items.length; index += 1) {
    if (compareHumanSentSubmissionItems(items[index - 1], items[index]) !== -1) {
      throw new Error(errorCode);
    }
  }
  const requestAnchor = requestCursor?.slice(3);
  if (requestAnchor && items.some((item) => item.submission_id === requestAnchor)) {
    throw new Error(errorCode);
  }
  const nextCursor = page.next_cursor === null
    ? null
    : boundedString(page.next_cursor, 39, 39, errorCode);
  if (nextCursor !== null && !humanSentSubmissionCursorPattern.test(nextCursor)) {
    throw new Error(errorCode);
  }
  const hasMore = booleanValue(page.has_more, errorCode);
  if (hasMore !== (nextCursor !== null)) throw new Error(errorCode);
  if (
    hasMore
    && (items.length === 0 || nextCursor !== `v1.${items.at(-1)?.submission_id}`)
  ) throw new Error(errorCode);
  if (requestCursor && nextCursor === requestCursor) {
    throw new Error(errorCode);
  }
  return { items, next_cursor: nextCursor, has_more: hasMore };
}

function draftAddresses(value: unknown, errorCode: string): string[] {
  return boundedArray(value, 100, errorCode)
    .map((address) => mailboxAddress(address, errorCode));
}

function parseHumanDraftRestore(
  value: unknown,
  expectedDraft: HumanDraftPage["items"][number],
): HumanDraftRestore {
  const errorCode = "draft_restore_response_invalid";
  const restored = exactRecord(value, humanDraftRestoreKeys, errorCode);
  const draftId = canonicalUuid(restored.draft_id, errorCode);
  const version = positiveSafeInteger(
    restored.version,
    Number.MAX_SAFE_INTEGER,
    errorCode,
  );
  if (draftId !== expectedDraft.draft_id || version !== expectedDraft.version) {
    throw new Error(errorCode);
  }
  const to = draftAddresses(restored.to, errorCode);
  const cc = draftAddresses(restored.cc, errorCode);
  const bcc = draftAddresses(restored.bcc, errorCode);
  const allRecipients = [...to, ...cc, ...bcc];
  if (
    allRecipients.length < 1
    || allRecipients.length > 100
    || new Set(allRecipients).size !== allRecipients.length
  ) throw new Error(errorCode);
  const replyToMessageId = optionalCanonicalUuid(restored.reply_to_message_id, errorCode);
  if (
    replyToMessageId !== expectedDraft.reply_to_message_id
    || allRecipients.length !== expectedDraft.recipient_count
  ) throw new Error(errorCode);
  return {
    draft_id: draftId,
    version,
    reply_to_message_id: replyToMessageId,
    to,
    cc,
    bcc,
    subject: boundedString(restored.subject, 0, 998, errorCode),
    text_body: boundedString(restored.text_body, 0, 256 * 1024, errorCode),
  };
}

function validatedDraftInput(draft: DraftInput): DraftInput {
  const errorCode = "draft_request_invalid";
  const request = exactRecord(draft, draftInputKeys, errorCode);
  if ((request.draft_id === null) !== (request.expected_version === null)) {
    throw new Error(errorCode);
  }
  const draftId = request.draft_id === null
    ? null
    : canonicalUuid(request.draft_id, errorCode);
  const expectedVersion = request.expected_version === null
    ? null
    : positiveSafeInteger(request.expected_version, Number.MAX_SAFE_INTEGER, errorCode);
  const replyToMessageId = optionalCanonicalUuid(request.reply_to_message_id, errorCode);
  const to = draftAddresses(request.to, errorCode);
  const cc = draftAddresses(request.cc, errorCode);
  const bcc = draftAddresses(request.bcc, errorCode);
  const allRecipients = [...to, ...cc, ...bcc];
  if (
    allRecipients.length < 1
    || allRecipients.length > 100
    || new Set(allRecipients).size !== allRecipients.length
    || typeof request.subject !== "string"
    || utf8.encode(request.subject).byteLength > 998
    || /[\u0000-\u001f\u007f-\u009f]/u.test(request.subject)
    || typeof request.text_body !== "string"
    || utf8.encode(request.text_body).byteLength > 256 * 1024
    || request.text_body.includes("\0")
  ) throw new Error(errorCode);
  return {
    draft_id: draftId,
    expected_version: expectedVersion,
    reply_to_message_id: replyToMessageId,
    to,
    cc,
    bcc,
    subject: request.subject,
    text_body: request.text_body,
  };
}

function parseDraftResult(value: unknown, request: DraftInput): DraftResult {
  const errorCode = "draft_response_invalid";
  const result = exactRecord(value, draftResultKeys, errorCode);
  const draftId = canonicalUuid(result.draft_id, errorCode);
  const version = positiveSafeInteger(result.version, Number.MAX_SAFE_INTEGER, errorCode);
  const replyToMessageId = optionalCanonicalUuid(result.reply_to_message_id, errorCode);
  if (
    replyToMessageId !== request.reply_to_message_id
    || (request.draft_id === null
      ? version !== 1
      : draftId !== request.draft_id
        || request.expected_version === null
        || version < request.expected_version
        || version > request.expected_version + 1)
  ) throw new Error(errorCode);
  return {
    draft_id: draftId,
    version,
    reply_to_message_id: replyToMessageId,
    previous_draft_destroyed: booleanValue(result.previous_draft_destroyed, errorCode),
  };
}

function submissionStatus(value: unknown, errorCode: string): MailSubmission["status"] {
  if (
    value === "pending"
    || value === "reconciling"
    || value === "accepted"
    || value === "rejected"
    || value === "manual_review"
  ) return value;
  throw new Error(errorCode);
}

function deliveryRecipientStatus(
  value: unknown,
  errorCode: string,
): MailDeliveryRecipient["status"] {
  if (
    value === "queued"
    || value === "delivered"
    || value === "permanent_failure"
    || value === "unknown"
  ) return value;
  throw new Error(errorCode);
}

function parseMailDelivery(value: unknown, submissionUpdatedAt: string) {
  const errorCode = "mail_submission_response_invalid";
  const delivery = exactRecord(value, mailDeliveryKeys, errorCode);
  const observedAt = draftDateTime(delivery.observed_at, errorCode);
  if (Date.parse(observedAt) < Date.parse(submissionUpdatedAt)) throw new Error(errorCode);
  const recipientCount = positiveSafeInteger(delivery.recipient_count, 100, errorCode);
  const counts = {
    delivered: nonNegativeSafeInteger(delivery.delivered_count, 100, errorCode),
    permanent_failure: nonNegativeSafeInteger(delivery.failed_count, 100, errorCode),
    queued: nonNegativeSafeInteger(delivery.queued_count, 100, errorCode),
    unknown: nonNegativeSafeInteger(delivery.unknown_count, 100, errorCode),
  };
  const recipients = boundedArray(delivery.recipients, 100, errorCode).map((value) => {
    const recipient = exactRecord(value, mailDeliveryRecipientKeys, errorCode);
    const recipientRef = boundedString(recipient.recipient_ref, 64, 64, errorCode);
    const smtpReply = boundedString(recipient.smtp_reply, 1, 1024, errorCode);
    if (
      !recipientReferencePattern.test(recipientRef)
      || /[\u0000-\u001f\u007f-\u009f]/u.test(smtpReply)
    ) throw new Error(errorCode);
    return {
      recipient_ref: recipientRef,
      status: deliveryRecipientStatus(recipient.status, errorCode),
      smtp_reply: smtpReply,
    };
  });
  const actualCounts = {
    delivered: recipients.filter((recipient) => recipient.status === "delivered").length,
    permanent_failure: recipients.filter(
      (recipient) => recipient.status === "permanent_failure",
    ).length,
    queued: recipients.filter((recipient) => recipient.status === "queued").length,
    unknown: recipients.filter((recipient) => recipient.status === "unknown").length,
  };
  const terminal = booleanValue(delivery.terminal, errorCode);
  if (
    recipients.length !== recipientCount
    || new Set(recipients.map((recipient) => recipient.recipient_ref)).size !== recipientCount
    || Object.entries(counts).some(([status, count]) =>
      actualCounts[status as keyof typeof actualCounts] !== count)
    || Object.values(counts).reduce((total, count) => total + count, 0) !== recipientCount
    || terminal !== (counts.queued === 0 && counts.unknown === 0)
  ) throw new Error(errorCode);
  return {
    observed_at: observedAt,
    terminal,
    recipient_count: recipientCount,
    delivered_count: counts.delivered,
    failed_count: counts.permanent_failure,
    queued_count: counts.queued,
    unknown_count: counts.unknown,
    recipients,
  };
}

function validSubmissionTransition(
  previous: MailSubmission["status"],
  next: MailSubmission["status"],
) {
  if (previous === next) return true;
  if (previous === "pending") return next !== "pending";
  if (previous === "reconciling") {
    return next === "accepted" || next === "rejected" || next === "manual_review";
  }
  return false;
}

function validDeliveryTransition(
  previous: NonNullable<MailSubmission["delivery"]>,
  next: NonNullable<MailSubmission["delivery"]>,
) {
  if (
    next.delivered_count < previous.delivered_count
    || next.failed_count < previous.failed_count
  ) return false;
  const previousRecipients = new Map(
    previous.recipients.map((recipient) => [recipient.recipient_ref, recipient]),
  );
  for (const recipient of next.recipients) {
    const prior = previousRecipients.get(recipient.recipient_ref);
    if (!prior) return false;
    if (
      (prior.status === "delivered" || prior.status === "permanent_failure")
      && (recipient.status !== prior.status || recipient.smtp_reply !== prior.smtp_reply)
    ) return false;
  }
  if (!previous.terminal) return true;
  return next.observed_at === previous.observed_at
    && next.delivered_count === previous.delivered_count
    && next.failed_count === previous.failed_count
    && next.queued_count === previous.queued_count
    && next.unknown_count === previous.unknown_count
    && next.recipients.every((recipient) => {
      const prior = previousRecipients.get(recipient.recipient_ref);
      return prior?.status === recipient.status && prior.smtp_reply === recipient.smtp_reply;
    });
}

function parseMailSubmission(
  value: unknown,
  expectedMailboxId: string,
  expectedDraftId: string,
  expectedDraftVersion: number,
  previous?: MailSubmission,
): MailSubmission {
  const errorCode = "mail_submission_response_invalid";
  const submission = exactRecord(value, mailSubmissionKeys, errorCode);
  const parsed: MailSubmission = {
    submission_id: canonicalUuid(submission.submission_id, errorCode),
    mailbox_id: canonicalUuid(submission.mailbox_id, errorCode),
    draft_id: canonicalUuid(submission.draft_id, errorCode),
    draft_version: positiveSafeInteger(
      submission.draft_version,
      Number.MAX_SAFE_INTEGER,
      errorCode,
    ),
    status: submissionStatus(submission.status, errorCode),
    state_version: positiveSafeInteger(
      submission.state_version,
      Number.MAX_SAFE_INTEGER,
      errorCode,
    ),
    created_at: draftDateTime(submission.created_at, errorCode),
    updated_at: draftDateTime(submission.updated_at, errorCode),
    delivery: null,
  };
  if (
    parsed.mailbox_id !== expectedMailboxId
    || parsed.draft_id !== expectedDraftId
    || parsed.draft_version !== expectedDraftVersion
    || Date.parse(parsed.updated_at) < Date.parse(parsed.created_at)
  ) throw new Error(errorCode);
  parsed.delivery = submission.delivery === null
    ? null
    : parseMailDelivery(submission.delivery, parsed.updated_at);
  if (parsed.delivery !== null && parsed.status !== "accepted") throw new Error(errorCode);
  if (!previous) return parsed;
  if (
    parsed.submission_id !== previous.submission_id
    || parsed.created_at !== previous.created_at
    || parsed.state_version < previous.state_version
    || Date.parse(parsed.updated_at) < Date.parse(previous.updated_at)
    || !validSubmissionTransition(previous.status, parsed.status)
    || (parsed.status === previous.status && (
      parsed.state_version !== previous.state_version
      || parsed.updated_at !== previous.updated_at
    ))
    || (parsed.status !== previous.status && parsed.state_version === previous.state_version)
    || (previous.delivery !== null && parsed.delivery === null)
    || (previous.delivery !== null && parsed.delivery !== null && (
      Date.parse(parsed.delivery.observed_at) < Date.parse(previous.delivery.observed_at)
      || parsed.delivery.recipient_count !== previous.delivery.recipient_count
      || previous.delivery.terminal && !parsed.delivery.terminal
      || [...parsed.delivery.recipients].map((recipient) => recipient.recipient_ref).sort().join("|")
        !== [...previous.delivery.recipients]
          .map((recipient) => recipient.recipient_ref)
          .sort()
          .join("|")
      || !validDeliveryTransition(previous.delivery, parsed.delivery)
    ))
  ) throw new Error(errorCode);
  return parsed;
}

export async function listMailboxes(
  config: RuntimeConfig,
  cursor?: string,
): Promise<MailboxDirectoryPage> {
  if (cursor !== undefined && !mailboxDirectoryCursorPattern.test(cursor)) {
    throw new Error("mailbox_directory_cursor_invalid");
  }
  const query = new URLSearchParams({ limit: "50" });
  if (cursor) query.set("cursor", cursor);
  const response = await fetch(
    endpoint(config, `/api/v2/mailboxes?${query}`),
    cookieRequest,
  );
  if (!response.ok) throw new Error(`mailbox_directory_${response.status}`);
  return parseMailboxDirectoryPage(await response.json(), cursor);
}

export async function listDrafts(
  config: MailboxScopedConfig,
  cursor?: string,
  signal?: AbortSignal,
): Promise<HumanDraftPage> {
  const mailboxId = canonicalUuid(config.mailboxId, "mailbox_id_invalid");
  if (cursor !== undefined && !humanDraftCursorPattern.test(cursor)) {
    throw new Error("draft_list_cursor_invalid");
  }
  const query = new URLSearchParams({ limit: "50" });
  if (cursor) query.set("cursor", cursor);
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(mailboxId)}/drafts?${query}`,
    ),
    { ...cookieRequest, signal },
  );
  if (!response.ok) throw new Error(`draft_list_${response.status}`);
  return parseHumanDraftPage(await response.json(), cursor);
}

/**
 * Lists only metadata for accepted submissions created by the current human.
 * This directory intentionally never requests a single submission or its
 * recipient/provider evidence.
 */
export async function listSentSubmissions(
  config: MailboxScopedConfig,
  cursor?: string,
  signal?: AbortSignal,
): Promise<HumanSentSubmissionPage> {
  const mailboxId = canonicalUuid(config.mailboxId, "mailbox_id_invalid");
  if (cursor !== undefined && !humanSentSubmissionCursorPattern.test(cursor)) {
    throw new Error("sent_list_cursor_invalid");
  }
  const query = new URLSearchParams({ limit: "50" });
  if (cursor) query.set("cursor", cursor);
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(mailboxId)}/submissions?${query}`,
    ),
    { ...cookieRequest, signal },
  );
  if (!response.ok) throw new Error(`sent_list_${response.status}`);
  return parseHumanSentSubmissionPage(await response.json(), cursor);
}

export async function restoreDraft(
  config: MailboxScopedConfig,
  draft: HumanDraftPage["items"][number],
  signal?: AbortSignal,
): Promise<HumanDraftRestore> {
  const mailboxId = canonicalUuid(config.mailboxId, "mailbox_id_invalid");
  const safeDraftId = canonicalUuid(draft.draft_id, "draft_id_invalid");
  const safeVersion = positiveSafeInteger(
    draft.version,
    Number.MAX_SAFE_INTEGER,
    "draft_version_invalid",
  );
  const expectedDraft = {
    ...draft,
    draft_id: safeDraftId,
    version: safeVersion,
    reply_to_message_id: optionalCanonicalUuid(
      draft.reply_to_message_id,
      "draft_reply_to_message_id_invalid",
    ),
    recipient_count: positiveSafeInteger(
      draft.recipient_count,
      100,
      "draft_recipient_count_invalid",
    ),
  };
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(mailboxId)}/drafts/${encodeURIComponent(safeDraftId)}/restore`,
    ),
    {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ expected_version: safeVersion }),
      signal,
    },
  );
  if (!response.ok) throw new Error(`draft_restore_${response.status}`);
  return parseHumanDraftRestore(await response.json(), expectedDraft);
}

export async function listInbox(
  config: MailboxScopedConfig,
  cursor?: string,
): Promise<InboxPage> {
  const query = new URLSearchParams({ page_size: "30" });
  if (cursor) query.set("cursor", cursor);
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(config.mailboxId)}/messages?${query}`,
    ),
    cookieRequest,
  );
  if (!response.ok) throw new Error(`inbox_${response.status}`);
  return parseInboxPage(await response.json());
}

export async function searchInbox(
  config: MailboxScopedConfig,
  text: string,
  cursor?: string,
): Promise<InboxPage> {
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(config.mailboxId)}/messages/search`,
    ),
    {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ text, cursor: cursor ?? null, page_size: 30 }),
    },
  );
  if (!response.ok) throw new Error(`inbox_search_${response.status}`);
  return parseInboxPage(await response.json());
}

export async function getMessage(
  config: MailboxScopedConfig,
  messageId: string,
  signal?: AbortSignal,
): Promise<MessageDetail> {
  const safeMessageId = messageUuid(messageId, "message_id_invalid");
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(config.mailboxId)}/messages/${encodeURIComponent(safeMessageId)}`,
    ),
    { ...cookieRequest, signal },
  );
  if (!response.ok) throw new Error(`message_${response.status}`);
  return parseMessageDetail(await response.json(), safeMessageId);
}

export async function updateMessageState(
  config: MailboxScopedConfig,
  messageId: string,
  update: { is_read?: boolean; is_flagged?: boolean },
): Promise<MessageState> {
  const safeMessageId = messageUuid(messageId, "message_id_invalid");
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(config.mailboxId)}/messages/${encodeURIComponent(safeMessageId)}`,
    ),
    {
      method: "PATCH",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(update),
    },
  );
  if (!response.ok) throw new Error(`message_state_${response.status}`);
  return parseMessageState(await response.json(), safeMessageId);
}

export async function saveDraft(
  config: MailboxScopedConfig,
  draft: DraftInput,
): Promise<DraftResult> {
  const mailboxId = canonicalUuid(config.mailboxId, "mailbox_id_invalid");
  const validatedDraft = validatedDraftInput(draft);
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(mailboxId)}/drafts`,
    ),
    {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(validatedDraft),
    },
  );
  if (!response.ok) throw new Error(`draft_${response.status}`);
  return parseDraftResult(await response.json(), validatedDraft);
}

export async function requestSubmission(
  config: MailboxScopedConfig,
  draftId: string,
  draftVersion: number,
  idempotencyKey: string,
): Promise<MailSubmission> {
  const mailboxId = canonicalUuid(config.mailboxId, "mailbox_id_invalid");
  const safeDraftId = canonicalUuid(draftId, "draft_id_invalid");
  const safeDraftVersion = positiveSafeInteger(
    draftVersion,
    Number.MAX_SAFE_INTEGER,
    "draft_version_invalid",
  );
  if (!idempotencyKeyPattern.test(idempotencyKey)) {
    throw new Error("idempotency_key_invalid");
  }
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mailboxes/${encodeURIComponent(mailboxId)}/submissions`,
    ),
    {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "Idempotency-Key": idempotencyKey,
      },
      body: JSON.stringify({
        draft_id: safeDraftId,
        expected_draft_version: safeDraftVersion,
      }),
    },
  );
  if (!response.ok) throw new Error(`submission_${response.status}`);
  return parseMailSubmission(
    await response.json(),
    mailboxId,
    safeDraftId,
    safeDraftVersion,
  );
}

export async function getSubmission(
  config: MailboxScopedConfig,
  previous: MailSubmission,
): Promise<MailSubmission> {
  const mailboxId = canonicalUuid(config.mailboxId, "mailbox_id_invalid");
  const submissionId = canonicalUuid(previous.submission_id, "submission_id_invalid");
  const draftId = canonicalUuid(previous.draft_id, "draft_id_invalid");
  const draftVersion = positiveSafeInteger(
    previous.draft_version,
    Number.MAX_SAFE_INTEGER,
    "draft_version_invalid",
  );
  if (previous.mailbox_id !== mailboxId) throw new Error("submission_mailbox_mismatch");
  const validatedPrevious = parseMailSubmission(
    previous,
    mailboxId,
    draftId,
    draftVersion,
  );
  const response = await fetch(
    endpoint(
      config,
      `/api/v2/mail-submissions/${encodeURIComponent(submissionId)}`,
    ),
    cookieRequest,
  );
  if (!response.ok) throw new Error(`submission_status_${response.status}`);
  return parseMailSubmission(
    await response.json(),
    mailboxId,
    draftId,
    draftVersion,
    validatedPrevious,
  );
}
