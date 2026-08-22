import { afterEach, describe, expect, it, vi } from "vitest";
import {
  getMessage,
  getSubmission,
  listDrafts,
  listInbox,
  listMailboxes,
  listSentSubmissions,
  requestSubmission,
  restoreDraft,
  saveDraft,
  searchInbox,
  updateMessageState,
} from "./api";
import type { DraftInput, MailboxScopedConfig, MailSubmission } from "./types";

const config: MailboxScopedConfig = {
  apiBaseUrl: "",
  mailboxId: "018f0000-0000-7000-8000-000000000001",
};
const messageId = "018f0000-0000-7000-8000-000000000009";
const otherMessageId = "018f0000-0000-7000-8000-000000000010";
const messageSummary = {
  message_id: messageId,
  subject: "Status",
  from: [{ name: "Sender", email: "sender@example.test" }],
  received_at: "2026-07-27T00:00:00Z",
  preview: "Ready.",
  is_read: false,
  is_flagged: false,
  has_attachment: false,
};

afterEach(() => vi.restoreAllMocks());

describe("cookie-authenticated mail API", () => {
  it("lists the bounded mailbox directory with the exact cursor contract", async () => {
    const cursor = "v1.018f0000-0000-7000-8000-000000000001";
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        items: [{
          mailbox_id: "018f0000-0000-7000-8000-000000000002",
          kind: "shared",
          primary_address: "support@example.test",
          can_send: false,
        }],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );

    await expect(listMailboxes(config, cursor)).resolves.toMatchObject({
      items: [{ kind: "shared", can_send: false }],
    });

    expect(fetchMock).toHaveBeenCalledWith(
      `/api/v2/mailboxes?limit=50&cursor=${cursor}`,
      expect.objectContaining({ credentials: "same-origin", cache: "no-store" }),
    );
  });

  it("fails closed on malformed mailbox directory identities and cursors", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [{
          mailbox_id: "018f0000-0000-7000-8000-000000000002",
          kind: "shared",
          primary_address: "Support@example.test",
          can_send: false,
        }],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [],
        next_cursor: "v1.018F0000-0000-7000-8000-000000000002",
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));

    await expect(listMailboxes(config)).rejects.toThrow(
      "mailbox_directory_response_invalid",
    );
    await expect(listMailboxes(config)).rejects.toThrow(
      "mailbox_directory_response_invalid",
    );
  });

  it("requires canonical ordered mailbox pages with a cursor bound to the last item", async () => {
    const first = "018f0000-0000-7000-8000-000000000002";
    const second = "018f0000-0000-7000-8000-000000000003";
    const item = (mailbox_id: string) => ({
      mailbox_id,
      kind: "human",
      primary_address: `${mailbox_id.slice(-4)}@example.test`,
      can_send: true,
    });
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [item(first.toUpperCase())],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [item(second), item(first)],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [item(first), item(second)],
        next_cursor: `v1.${first}`,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [],
        next_cursor: `v1.${second}`,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));

    for (let attempt = 0; attempt < 4; attempt += 1) {
      await expect(listMailboxes(config)).rejects.toThrow(
        "mailbox_directory_response_invalid",
      );
    }
  });

  it("rejects a non-canonical directory request cursor before network I/O", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");

    await expect(listMailboxes(config, "v1.018F0000-0000-7000-8000-000000000001"))
      .rejects.toThrow("mailbox_directory_cursor_invalid");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects a repeated next-page anchor or an item at or before the request cursor", async () => {
    const anchorId = "018f0000-0000-7000-8000-000000000002";
    const cursor = `v1.${anchorId}`;
    const nextId = "018f0000-0000-7000-8000-000000000003";
    const item = (mailbox_id: string) => ({
      mailbox_id,
      kind: "shared",
      primary_address: `${mailbox_id.slice(-4)}@example.test`,
      can_send: true,
    });
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [item(anchorId)],
        next_cursor: null,
        has_more: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        items: [item(nextId)],
        next_cursor: cursor,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }));

    await expect(listMailboxes(config, cursor)).rejects.toThrow(
      "mailbox_directory_response_invalid",
    );
    await expect(listMailboxes(config, cursor)).rejects.toThrow(
      "mailbox_directory_response_invalid",
    );
  });

  it("bounds inbox pages and sends only same-origin cookie credentials", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ items: [], next_cursor: null, has_more: false }), {
        status: 200, headers: { "Content-Type": "application/json" },
      }),
    );

    await listInbox(config, "opaque-cursor");

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("page_size=30");
    expect(String(url)).toContain("cursor=opaque-cursor");
    expect(init).toMatchObject({
      credentials: "same-origin",
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    expect(init?.headers).not.toHaveProperty("Authorization");
  });

  it("rejects non-success responses", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 403 }));
    await expect(listInbox(config)).rejects.toThrow("inbox_403");
  });

  it("round-trips only the safe message UUID in the detail path", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        message: messageSummary, reply_to_message_id: messageId, to: [], cc: [], text_body: "",
        text_body_truncated: false, attachments: [],
      }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );

    const detail = await getMessage(config, messageId);

    expect(String(fetchMock.mock.calls[0][0])).toBe(
      `/api/v2/mailboxes/${config.mailboxId}/messages/${messageId}`,
    );
    expect(fetchMock.mock.calls[0][1]).toMatchObject({
      credentials: "same-origin",
      cache: "no-store",
    });
    expect(detail.message.message_id).toBe(messageId);
    expect(detail.message).not.toHaveProperty("id");
    expect(detail.message).not.toHaveProperty("thread_id");
  });

  it("rejects a reply identity that does not match the opened message", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        message: messageSummary,
        reply_to_message_id: otherMessageId,
        to: [],
        cc: [],
        text_body: "",
        text_body_truncated: false,
        attachments: [],
      }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );

    await expect(getMessage(config, messageId)).rejects.toThrow(
      "message_reply_id_mismatch",
    );
  });

  it("updates only explicitly requested Human presentation state", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        message_id: messageId, is_read: true, is_flagged: false,
      }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );

    await updateMessageState(config, messageId, { is_read: true });

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toBe(
      `/api/v2/mailboxes/${config.mailboxId}/messages/${messageId}`,
    );
    expect(init).toMatchObject({
      method: "PATCH",
      credentials: "same-origin",
      cache: "no-store",
      body: "{\"is_read\":true}",
    });
    expect(init?.headers).toMatchObject({ "Content-Type": "application/json" });
    expect(init?.headers).not.toHaveProperty("Authorization");
  });

  it("rejects non-UUID message path input before making a request", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");

    await expect(getMessage(config, "provider-jmap-email-id")).rejects.toThrow(
      "message_id_invalid",
    );
    await expect(
      updateMessageState(config, "provider-jmap-email-id", { is_read: true }),
    ).rejects.toThrow("message_id_invalid");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("fails closed on provider identifiers or mismatched UUIDs in inbox responses", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          items: [{
            ...messageSummary,
            message_id: undefined,
            id: "jmap-email-id",
            thread_id: "jmap-thread-id",
          }],
          next_cursor: null,
          has_more: false,
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message: { ...messageSummary, message_id: otherMessageId },
          reply_to_message_id: otherMessageId,
          to: [],
          cc: [],
          text_body: "",
          text_body_truncated: false,
          attachments: [],
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message_id: otherMessageId,
          is_read: true,
          is_flagged: false,
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      );

    await expect(listInbox(config)).rejects.toThrow("inbox_message_id_invalid");
    await expect(getMessage(config, messageId)).rejects.toThrow(
      "message_response_id_mismatch",
    );
    await expect(
      updateMessageState(config, messageId, { is_read: true }),
    ).rejects.toThrow("message_state_response_id_mismatch");
  });

  it("rejects unknown provider fields at every Human inbox response boundary", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          items: [],
          next_cursor: null,
          has_more: false,
          accountId: "42",
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          items: [{ ...messageSummary, threadId: "jmap-thread" }],
          next_cursor: null,
          has_more: false,
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message: messageSummary,
          reply_to_message_id: messageId,
          to: [],
          cc: [],
          text_body: "",
          text_body_truncated: false,
          attachments: [],
          engine_document_ref: "jmap-email",
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message: messageSummary,
          reply_to_message_id: messageId,
          to: [{
            name: null,
            email: "reader@example.test",
            accountId: "42",
          }],
          cc: [],
          text_body: "",
          text_body_truncated: false,
          attachments: [],
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message: messageSummary,
          reply_to_message_id: messageId,
          to: [],
          cc: [],
          text_body: "",
          text_body_truncated: false,
          attachments: [{
            name: "report.txt",
            media_type: "text/plain",
            size_bytes: 12,
            blobId: "jmap-blob",
          }],
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message_id: messageId,
          is_read: true,
          is_flagged: false,
          engine_document_ref: "jmap-email",
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      );

    await expect(listInbox(config)).rejects.toThrow("inbox_response_invalid");
    await expect(listInbox(config)).rejects.toThrow("inbox_message_id_invalid");
    await expect(getMessage(config, messageId)).rejects.toThrow("message_response_invalid");
    await expect(getMessage(config, messageId)).rejects.toThrow("message_response_invalid");
    await expect(getMessage(config, messageId)).rejects.toThrow("message_response_invalid");
    await expect(
      updateMessageState(config, messageId, { is_read: true }),
    ).rejects.toThrow("message_state_response_invalid");
  });

  it("rejects malformed types and values beyond the published bounds", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          items: [{ ...messageSummary, preview: "x".repeat(4097) }],
          next_cursor: null,
          has_more: false,
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message: messageSummary,
          reply_to_message_id: messageId,
          to: [],
          cc: [],
          text_body: "",
          text_body_truncated: false,
          attachments: [{
            name: "report.txt",
            media_type: "text/plain",
            size_bytes: -1,
          }],
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          message_id: messageId,
          is_read: "true",
          is_flagged: false,
        }), { status: 200, headers: { "Content-Type": "application/json" } }),
      );

    await expect(listInbox(config)).rejects.toThrow("inbox_message_id_invalid");
    await expect(getMessage(config, messageId)).rejects.toThrow("message_response_invalid");
    await expect(
      updateMessageState(config, messageId, { is_read: true }),
    ).rejects.toThrow("message_state_response_invalid");
  });

  it("saves a draft without accepting a caller-selected From address", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        draft_id: "018f0000-0000-7000-8000-000000000002",
        version: 2,
        reply_to_message_id: "018f0000-0000-7000-8000-000000000009",
        previous_draft_destroyed: true,
      }), { status: 201, headers: { "Content-Type": "application/json" } }),
    );

    await saveDraft(config, {
      draft_id: "018f0000-0000-7000-8000-000000000002",
      expected_version: 1,
      reply_to_message_id: "018f0000-0000-7000-8000-000000000009",
      to: ["recipient@example.test"],
      cc: [],
      bcc: [],
      subject: "Status",
      text_body: "Ready.",
    });

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/drafts");
    expect(init).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
    });
    expect(JSON.parse(String(init?.body))).toEqual({
      draft_id: "018f0000-0000-7000-8000-000000000002",
      expected_version: 1,
      reply_to_message_id: "018f0000-0000-7000-8000-000000000009",
      to: ["recipient@example.test"],
      cc: [],
      bcc: [],
      subject: "Status",
      text_body: "Ready.",
    });
    expect(String(init?.body)).not.toContain("\"from\"");
  });

  it("requests submission with a header-only idempotency key", async () => {
    const submission = {
      submission_id: "018f0000-0000-7000-8000-000000000003",
      mailbox_id: config.mailboxId,
      draft_id: "018f0000-0000-7000-8000-000000000002",
      draft_version: 2,
      status: "pending",
      state_version: 1,
      created_at: "2026-07-27T12:00:00Z",
      updated_at: "2026-07-27T12:00:00Z",
      delivery: null,
    };
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify(submission), {
        status: 202,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await requestSubmission(
      config,
      "018f0000-0000-7000-8000-000000000002",
      2,
      "stable-idempotency-key-000001",
    );

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/submissions");
    expect(init).toMatchObject({
      credentials: "same-origin",
      cache: "no-store",
    });
    expect(init?.headers).toMatchObject({
      "Idempotency-Key": "stable-idempotency-key-000001",
    });
    expect(init?.headers).not.toHaveProperty("Authorization");
    expect(JSON.parse(String(init?.body))).toEqual({
      draft_id: "018f0000-0000-7000-8000-000000000002",
      expected_draft_version: 2,
    });
    expect(String(init?.body)).not.toContain("idempotency");
  });

  it("binds a polled submission to its exact prior identity and transition", async () => {
    const previous = {
      submission_id: "018f0000-0000-7000-8000-000000000003",
      mailbox_id: config.mailboxId,
      draft_id: "018f0000-0000-7000-8000-000000000002",
      draft_version: 2,
      status: "pending" as const,
      state_version: 1,
      created_at: "2026-07-27T12:00:00Z",
      updated_at: "2026-07-27T12:00:00Z",
      delivery: null,
    };
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        ...previous,
        status: "reconciling",
        state_version: 2,
        updated_at: "2026-07-27T12:00:01Z",
      }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await expect(getSubmission(config, previous)).resolves.toMatchObject({
      submission_id: previous.submission_id,
      status: "reconciling",
      state_version: 2,
    });

    expect(String(fetchMock.mock.calls[0][0])).toContain(
      `/mail-submissions/${previous.submission_id}`,
    );
    expect(fetchMock.mock.calls[0][1]).toMatchObject({ cache: "no-store" });
  });
});

describe("strict draft and submission API boundaries", () => {
  const draftId = "018f0000-0000-7000-8000-000000000002";
  const otherDraftId = "018f0000-0000-7000-8000-000000000012";
  const replyId = "018f0000-0000-7000-8000-000000000009";
  const submissionId = "018f0000-0000-7000-8000-000000000003";
  const otherSubmissionId = "018f0000-0000-7000-8000-000000000013";
  const draft: DraftInput = {
    draft_id: draftId,
    expected_version: 2,
    reply_to_message_id: replyId,
    to: ["recipient@example.test"],
    cc: [],
    bcc: [],
    subject: "Status",
    text_body: "Ready.",
  };
  const pendingSubmission: MailSubmission = {
    submission_id: submissionId,
    mailbox_id: config.mailboxId,
    draft_id: draftId,
    draft_version: 2,
    status: "pending",
    state_version: 1,
    created_at: "2026-07-27T12:00:00Z",
    updated_at: "2026-07-27T12:00:00Z",
    delivery: null,
  };
  const delivery = {
    observed_at: "2026-07-27T12:00:01Z",
    terminal: true,
    recipient_count: 1,
    delivered_count: 1,
    failed_count: 0,
    queued_count: 0,
    unknown_count: 0,
    recipients: [{
      recipient_ref: "a".repeat(64),
      status: "delivered" as const,
      smtp_reply: "250 delivered",
    }],
  };
  const jsonResponse = (value: unknown, status = 200) => new Response(
    JSON.stringify(value),
    { status, headers: { "Content-Type": "application/json" } },
  );

  it("rejects malformed draft requests before network I/O", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const malformed: DraftInput[] = [
      { ...draft, draft_id: null, expected_version: 2 },
      { ...draft, draft_id: draftId.toUpperCase() },
      { ...draft, expected_version: 0 },
      { ...draft, to: ["Recipient@example.test"] },
      { ...draft, cc: ["recipient@example.test"] },
      { ...draft, subject: "bad\nsubject" },
      { ...draft, text_body: "bad\0body" },
      { ...draft, from: "caller@example.test" } as DraftInput,
    ];

    for (const request of malformed) {
      await expect(saveDraft(config, request)).rejects.toThrow("draft_request_invalid");
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("binds an exact draft save response to the requested identity and monotonic version", async () => {
    const valid = {
      draft_id: draftId,
      version: 3,
      reply_to_message_id: replyId,
      previous_draft_destroyed: true,
    };
    const malformed = [
      { ...valid, provider_id: "leak" },
      { ...valid, draft_id: otherDraftId },
      { ...valid, draft_id: draftId.toUpperCase() },
      { ...valid, version: 1 },
      { ...valid, version: 4 },
      { ...valid, reply_to_message_id: null },
      { ...valid, previous_draft_destroyed: "true" },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const response of malformed) {
      fetchMock.mockResolvedValueOnce(jsonResponse(response, 201));
      await expect(saveDraft(config, draft)).rejects.toThrow("draft_response_invalid");
    }
    fetchMock.mockResolvedValueOnce(jsonResponse({ ...valid, version: 2 }, 201));
    await expect(saveDraft(config, draft)).resolves.toMatchObject({ version: 2 });
    fetchMock.mockResolvedValueOnce(jsonResponse({ ...valid, version: 2 }, 201));
    await expect(saveDraft(config, {
      ...draft,
      draft_id: null,
      expected_version: null,
    })).rejects.toThrow("draft_response_invalid");
  });

  it("rejects malformed submission request identities and idempotency keys before fetch", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    await expect(requestSubmission(
      { ...config, mailboxId: config.mailboxId.toUpperCase() },
      draftId,
      2,
      "stable-idempotency-key-000001",
    )).rejects.toThrow("mailbox_id_invalid");
    await expect(requestSubmission(
      config,
      draftId.toUpperCase(),
      2,
      "stable-idempotency-key-000001",
    )).rejects.toThrow("draft_id_invalid");
    await expect(requestSubmission(
      config,
      draftId,
      0,
      "stable-idempotency-key-000001",
    )).rejects.toThrow("draft_version_invalid");
    for (const key of ["too-short", "invalid/idempotency/key"] as const) {
      await expect(requestSubmission(config, draftId, 2, key)).rejects.toThrow(
        "idempotency_key_invalid",
      );
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects exact-record and identity substitutions in submission creation responses", async () => {
    const malformed = [
      { ...pendingSubmission, provider_id: "leak" },
      { ...pendingSubmission, submission_id: submissionId.toUpperCase() },
      { ...pendingSubmission, mailbox_id: "018f0000-0000-7000-8000-000000000011" },
      { ...pendingSubmission, draft_id: otherDraftId },
      { ...pendingSubmission, draft_version: 3 },
      { ...pendingSubmission, status: "sending" },
      { ...pendingSubmission, state_version: 0 },
      { ...pendingSubmission, updated_at: "2026-07-27T11:59:59Z" },
      { ...pendingSubmission, status: "rejected", delivery },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const response of malformed) {
      fetchMock.mockResolvedValueOnce(jsonResponse(response, 202));
      await expect(requestSubmission(
        config,
        draftId,
        2,
        "stable-idempotency-key-000001",
      )).rejects.toThrow("mail_submission_response_invalid");
    }
  });

  it("validates the prior poll snapshot before using it as request identity", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    await expect(getSubmission(config, {
      ...pendingSubmission,
      created_at: "not-a-date",
    })).rejects.toThrow("mail_submission_response_invalid");
    await expect(getSubmission(config, {
      ...pendingSubmission,
      unexpected: true,
    } as MailSubmission)).rejects.toThrow("mail_submission_response_invalid");
    await expect(getSubmission(config, {
      ...pendingSubmission,
      mailbox_id: "018f0000-0000-7000-8000-000000000011",
    })).rejects.toThrow("submission_mailbox_mismatch");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("binds polls to one submission and rejects state rollback or illegal transitions", async () => {
    const malformed = [
      { ...pendingSubmission, submission_id: otherSubmissionId },
      { ...pendingSubmission, mailbox_id: "018f0000-0000-7000-8000-000000000011" },
      { ...pendingSubmission, draft_id: otherDraftId },
      { ...pendingSubmission, draft_version: 3 },
      { ...pendingSubmission, created_at: "2026-07-27T12:00:01Z" },
      { ...pendingSubmission, status: "reconciling", state_version: 1 },
      {
        ...pendingSubmission,
        state_version: 2,
        updated_at: "2026-07-27T12:00:01Z",
      },
      { ...pendingSubmission, state_version: 0 },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const response of malformed) {
      fetchMock.mockResolvedValueOnce(jsonResponse(response));
      await expect(getSubmission(config, pendingSubmission)).rejects.toThrow(
        "mail_submission_response_invalid",
      );
    }

    const reconciling: MailSubmission = {
      ...pendingSubmission,
      status: "reconciling",
      state_version: 2,
      updated_at: "2026-07-27T12:00:01Z",
    };
    fetchMock.mockResolvedValueOnce(jsonResponse({
      ...reconciling,
      status: "pending",
      state_version: 3,
      updated_at: "2026-07-27T12:00:02Z",
    }));
    await expect(getSubmission(config, reconciling)).rejects.toThrow(
      "mail_submission_response_invalid",
    );
    const rejected: MailSubmission = {
      ...pendingSubmission,
      status: "rejected",
      state_version: 2,
      updated_at: "2026-07-27T12:00:01Z",
    };
    fetchMock.mockResolvedValueOnce(jsonResponse({
      ...rejected,
      status: "reconciling",
      state_version: 3,
      updated_at: "2026-07-27T12:00:02Z",
    }));
    await expect(getSubmission(config, rejected)).rejects.toThrow(
      "mail_submission_response_invalid",
    );
  });

  it("enforces delivery recipient, count, terminal, and timestamp bounds", async () => {
    const accepted = {
      ...pendingSubmission,
      status: "accepted",
      state_version: 2,
      updated_at: "2026-07-27T12:00:01Z",
      delivery,
    } satisfies MailSubmission;
    const malformed = [
      { ...delivery, provider_id: "leak" },
      { ...delivery, recipient_count: 2 },
      { ...delivery, delivered_count: 0, failed_count: 1 },
      { ...delivery, terminal: false },
      { ...delivery, observed_at: "2026-07-27T12:00:00Z" },
      { ...delivery, recipients: [{ ...delivery.recipients[0], recipient_ref: "A".repeat(64) }] },
      { ...delivery, recipients: [{ ...delivery.recipients[0], status: "deferred" }] },
      { ...delivery, recipients: [{ ...delivery.recipients[0], smtp_reply: "250\nleak" }] },
      { ...delivery, recipients: [{ ...delivery.recipients[0], provider_id: "leak" }] },
      {
        ...delivery,
        recipient_count: 2,
        delivered_count: 2,
        recipients: [delivery.recipients[0], delivery.recipients[0]],
      },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const invalidDelivery of malformed) {
      fetchMock.mockResolvedValueOnce(jsonResponse({ ...accepted, delivery: invalidDelivery }, 202));
      await expect(requestSubmission(
        config,
        draftId,
        2,
        "stable-idempotency-key-000001",
      )).rejects.toThrow("mail_submission_response_invalid");
    }
    fetchMock.mockResolvedValueOnce(jsonResponse(accepted, 202));
    await expect(requestSubmission(
      config,
      draftId,
      2,
      "stable-idempotency-key-000001",
    )).resolves.toEqual(accepted);
  });

  it("allows delivery progress but rejects terminal-recipient or terminal-snapshot rewrites", async () => {
    const previous: MailSubmission = {
      ...pendingSubmission,
      status: "accepted",
      state_version: 2,
      updated_at: "2026-07-27T12:00:01Z",
      delivery: {
        observed_at: "2026-07-27T12:00:01Z",
        terminal: false,
        recipient_count: 2,
        delivered_count: 1,
        failed_count: 0,
        queued_count: 1,
        unknown_count: 0,
        recipients: [
          delivery.recipients[0],
          { recipient_ref: "b".repeat(64), status: "queued", smtp_reply: "250 queued" },
        ],
      },
    };
    const completed: MailSubmission = {
      ...previous,
      delivery: {
        observed_at: "2026-07-27T12:00:02Z",
        terminal: true,
        recipient_count: 2,
        delivered_count: 1,
        failed_count: 1,
        queued_count: 0,
        unknown_count: 0,
        recipients: [
          delivery.recipients[0],
          {
            recipient_ref: "b".repeat(64),
            status: "permanent_failure",
            smtp_reply: "550 rejected",
          },
        ],
      },
    };
    const fetchMock = vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(jsonResponse(completed))
      .mockResolvedValueOnce(jsonResponse({
        ...completed,
        delivery: {
          ...completed.delivery,
          delivered_count: 2,
          failed_count: 0,
          recipients: [
            delivery.recipients[0],
            { recipient_ref: "b".repeat(64), status: "delivered", smtp_reply: "250 changed" },
          ],
        },
      }));

    await expect(getSubmission(config, previous)).resolves.toEqual(completed);
    await expect(getSubmission(config, completed)).rejects.toThrow(
      "mail_submission_response_invalid",
    );
  });
});

describe("searchInbox", () => {
  it("keeps sensitive search text in the POST body and preserves a bound cursor", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ items: [], next_cursor: null, has_more: false }), {
        status: 200, headers: { "Content-Type": "application/json" },
      }),
    );

    await searchInbox(config, "quarterly report", "bound-cursor");

    const [url, request] = fetchMock.mock.calls[0]!;
    expect(url).toBe(
      "/api/v2/mailboxes/018f0000-0000-7000-8000-000000000001/messages/search",
    );
    expect(String(url)).not.toContain("quarterly");
    expect(request).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
    });
    expect(JSON.parse(String(request?.body))).toEqual({
      text: "quarterly report",
      cursor: "bound-cursor",
      page_size: 30,
    });
    expect(request?.headers).not.toHaveProperty("Authorization");
  });

  it("surfaces a stable search-specific error", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 400 }));
    await expect(searchInbox(config, "x")).rejects.toThrow("inbox_search_400");
  });
});

describe("human draft recovery API", () => {
  const newestDraftId = "018f0000-0000-7000-8000-000000000010";
  const olderDraftId = "018f0000-0000-7000-8000-000000000009";
  const replyId = "018f0000-0000-7000-8000-000000000008";
  const draftItem = (draft_id = newestDraftId) => ({
    draft_id,
    version: 2,
    reply_to_message_id: null,
    recipient_count: 1,
    created_at: "2026-07-27T12:00:00.000Z",
    updated_at: "2026-07-27T12:01:00.000Z",
  });

  it("lists only metadata with a bounded opaque cursor and abort signal", async () => {
    const cursor = "v1.018f0000-0000-7000-8000-000000000011";
    const controller = new AbortController();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        items: [draftItem(newestDraftId), draftItem(olderDraftId)],
        next_cursor: `v1.${olderDraftId}`,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );

    await expect(listDrafts(config, cursor, controller.signal)).resolves.toMatchObject({
      items: [{ draft_id: newestDraftId }, { draft_id: olderDraftId }],
      has_more: true,
    });
    expect(fetchMock).toHaveBeenCalledWith(
      `/api/v2/mailboxes/${config.mailboxId}/drafts?limit=50&cursor=${cursor}`,
      expect.objectContaining({
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
      }),
    );
    expect(JSON.stringify(fetchMock.mock.calls[0])).not.toContain("text_body");
  });

  it("fails closed on malformed draft identities, versions, dates, counts, and cursors", async () => {
    const validPage = {
      items: [draftItem(newestDraftId), draftItem(olderDraftId)],
      next_cursor: null,
      has_more: false,
    };
    const malformed = [
      { ...validPage, unexpected: true },
      { ...validPage, items: [{ ...draftItem(), draft_id: newestDraftId.toUpperCase() }] },
      { ...validPage, items: [{ ...draftItem(), version: 0 }] },
      { ...validPage, items: [{ ...draftItem(), recipient_count: 101 }] },
      { ...validPage, items: [{ ...draftItem(), created_at: "July 27, 2026" }] },
      { ...validPage, items: [{ ...draftItem(), created_at: "2026-02-30T12:00:00Z" }] },
      {
        ...validPage,
        items: [{
          ...draftItem(),
          created_at: "2026-07-27T12:02:00Z",
          updated_at: "2026-07-27T12:01:00Z",
        }],
      },
      { ...validPage, items: [...validPage.items].reverse() },
      { ...validPage, next_cursor: `v1.${olderDraftId}`, has_more: false },
      { ...validPage, next_cursor: `v1.${newestDraftId}`, has_more: true },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const response of malformed) {
      fetchMock.mockResolvedValueOnce(new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }));
      await expect(listDrafts(config)).rejects.toThrow("draft_list_response_invalid");
    }

    await expect(listDrafts(config, "opaque-but-not-canonical")).rejects.toThrow(
      "draft_list_cursor_invalid",
    );
    expect(fetchMock).toHaveBeenCalledTimes(malformed.length);
  });

  it("restores one exact version without placing content in the URL", async () => {
    const controller = new AbortController();
    const expectedDraft = {
      ...draftItem(newestDraftId),
      version: 2,
      reply_to_message_id: replyId,
      recipient_count: 2,
    };
    const response = {
      draft_id: newestDraftId,
      version: 2,
      reply_to_message_id: replyId,
      to: ["alice@example.test"],
      cc: ["bob@example.test"],
      bcc: [],
      subject: "Private plan",
      text_body: "Only returned after explicit restore.",
    };
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await expect(restoreDraft(config, expectedDraft, controller.signal))
      .resolves.toEqual(response);
    const [url, request] = fetchMock.mock.calls[0];
    expect(url).toBe(
      `/api/v2/mailboxes/${config.mailboxId}/drafts/${newestDraftId}/restore`,
    );
    expect(request).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      signal: controller.signal,
    });
    expect(JSON.parse(String(request?.body))).toEqual({ expected_version: 2 });
    expect(String(url)).not.toContain("Private");
  });

  it("rejects restore response substitution and malformed recipient content", async () => {
    const expectedDraft = { ...draftItem(newestDraftId), version: 2 };
    const valid = {
      draft_id: newestDraftId,
      version: 2,
      reply_to_message_id: null,
      to: ["alice@example.test"],
      cc: [],
      bcc: [],
      subject: "Subject",
      text_body: "Body",
    };
    const malformed = [
      { ...valid, draft_id: olderDraftId },
      { ...valid, version: 3 },
      { ...valid, reply_to_message_id: replyId },
      { ...valid, cc: ["bob@example.test"] },
      { ...valid, extra: "field" },
      { ...valid, to: ["Alice@example.test"] },
      { ...valid, cc: ["alice@example.test"] },
      { ...valid, to: [] },
      { ...valid, subject: "界".repeat(333) },
      { ...valid, text_body: "界".repeat(87_382) },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const response of malformed) {
      fetchMock.mockResolvedValueOnce(new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }));
      await expect(restoreDraft(config, expectedDraft)).rejects.toThrow(
        "draft_restore_response_invalid",
      );
    }
    await expect(restoreDraft(config, {
      ...expectedDraft,
      draft_id: newestDraftId.toUpperCase(),
    })).rejects.toThrow(
      "draft_id_invalid",
    );
    await expect(restoreDraft(config, { ...expectedDraft, version: 0 })).rejects.toThrow(
      "draft_version_invalid",
    );
    expect(fetchMock).toHaveBeenCalledTimes(malformed.length);
  });
});

describe("human sent submission directory API", () => {
  const newestSubmissionId = "018f0000-0000-7000-8000-000000000010";
  const olderSubmissionId = "018f0000-0000-7000-8000-000000000009";
  const sentItem = (submission_id = newestSubmissionId) => ({
    submission_id,
    accepted_at: "2026-07-27T12:00:00.000Z",
    recipient_count: 2,
    delivery: {
      observed_at: "2026-07-27T12:01:00.000Z",
      terminal: false,
      recipient_count: 2,
      delivered_count: 1,
      failed_count: 0,
      queued_count: 1,
      unknown_count: 0,
    },
  });

  it("lists only accepted-send metadata with a bounded opaque cursor and abort signal", async () => {
    const cursor = "v1.018f0000-0000-7000-8000-000000000011";
    const controller = new AbortController();
    const newerAcceptanceWithOlderId = {
      ...sentItem(olderSubmissionId),
      accepted_at: "2026-07-27T12:00:00.000000001+00:00",
    };
    const olderAcceptanceWithNewerId = sentItem(newestSubmissionId);
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        items: [newerAcceptanceWithOlderId, olderAcceptanceWithNewerId],
        next_cursor: `v1.${newestSubmissionId}`,
        has_more: true,
      }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );

    await expect(listSentSubmissions(config, cursor, controller.signal)).resolves.toMatchObject({
      items: [{ submission_id: olderSubmissionId }, { submission_id: newestSubmissionId }],
      has_more: true,
    });
    expect(fetchMock).toHaveBeenCalledWith(
      `/api/v2/mailboxes/${config.mailboxId}/submissions?limit=50&cursor=${cursor}`,
      expect.objectContaining({
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
      }),
    );
    const request = fetchMock.mock.calls[0]?.[1];
    expect(request?.headers).not.toHaveProperty("Authorization");
    expect(String(fetchMock.mock.calls[0]?.[0])).not.toContain("recipient_ref");
  });

  it("fails closed on extra or private fields, invalid aggregate evidence, and cursors", async () => {
    const validPage = {
      items: [sentItem(newestSubmissionId), sentItem(olderSubmissionId)],
      next_cursor: null,
      has_more: false,
    };
    const malformed = [
      { ...validPage, unexpected: true },
      { ...validPage, items: [{ ...sentItem(), submission_id: newestSubmissionId.toUpperCase() }] },
      { ...validPage, items: [{ ...sentItem(), recipient_count: 0 }] },
      { ...validPage, items: [{ ...sentItem(), accepted_at: "July 27, 2026" }] },
      { ...validPage, items: [{ ...sentItem(), delivery: null }] },
      {
        ...validPage,
        items: [{ ...sentItem(), draft_id: "018f0000-0000-7000-8000-000000000001" }],
      },
      {
        ...validPage,
        items: [{
          ...sentItem(),
          delivery: { ...sentItem().delivery, recipient_count: 1 },
        }],
      },
      {
        ...validPage,
        items: [{
          ...sentItem(),
          delivery: { ...sentItem().delivery, observed_at: "2026-07-27T11:59:59Z" },
        }],
      },
      {
        ...validPage,
        items: [{
          ...sentItem(),
          delivery: { ...sentItem().delivery, terminal: true },
        }],
      },
      { ...validPage, items: [...validPage.items].reverse() },
      { ...validPage, next_cursor: `v1.${olderSubmissionId}`, has_more: false },
      { ...validPage, next_cursor: `v1.${newestSubmissionId}`, has_more: true },
    ];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const response of malformed) {
      fetchMock.mockResolvedValueOnce(new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }));
      await expect(listSentSubmissions(config)).rejects.toThrow("sent_list_response_invalid");
    }

    await expect(listSentSubmissions(config, "opaque-but-not-canonical")).rejects.toThrow(
      "sent_list_cursor_invalid",
    );
    expect(fetchMock).toHaveBeenCalledTimes(malformed.length);
  });
});
