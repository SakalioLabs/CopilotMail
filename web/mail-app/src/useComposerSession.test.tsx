import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { useComposerSession } from "./useComposerSession";
import type {
  DraftResult,
  HumanDraftRestore,
  MailSubmission,
  MailboxScopedConfig,
} from "./types";

vi.mock("./api", async (importOriginal) => ({
  ...await importOriginal<typeof import("./api")>(),
  getSubmission: vi.fn(),
  requestSubmission: vi.fn(),
  saveDraft: vi.fn(),
}));

const mailboxSettings: MailboxScopedConfig = {
  apiBaseUrl: "",
  mailboxId: "019b0000-0000-7000-8000-000000000020",
};

const draftResult: DraftResult = {
  draft_id: "019b0000-0000-7000-8000-000000000030",
  version: 1,
  reply_to_message_id: null,
  previous_draft_destroyed: true,
};

const restoredDraft: HumanDraftRestore = {
  draft_id: "019b0000-0000-7000-8000-000000000090",
  version: 4,
  reply_to_message_id: null,
  to: ["alice@example.com"],
  cc: ["bob@example.com"],
  bcc: ["audit@example.com"],
  subject: "Restored subject",
  text_body: "Restored body",
};

const deliveredSubmission: MailSubmission = {
  submission_id: "019b0000-0000-7000-8000-000000000040",
  mailbox_id: mailboxSettings.mailboxId,
  draft_id: draftResult.draft_id,
  draft_version: draftResult.version,
  status: "accepted",
  state_version: 1,
  created_at: "2026-07-27T12:00:00.000Z",
  updated_at: "2026-07-27T12:00:01.000Z",
  delivery: {
    observed_at: "2026-07-27T12:00:01.000Z",
    terminal: true,
    recipient_count: 1,
    delivered_count: 1,
    failed_count: 0,
    queued_count: 0,
    unknown_count: 0,
    recipients: [],
  },
};

const manualReviewSubmission: MailSubmission = {
  ...deliveredSubmission,
  status: "manual_review",
  state_version: 2,
  updated_at: "2026-07-27T12:00:02.000Z",
  delivery: null,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, reject, resolve };
}

async function flushMicrotasks() {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

function renderComposer(sessionLocked = false) {
  return renderHook(
    ({ locked }: { locked: boolean }) => useComposerSession({
      authenticated: true,
      mailboxSettings,
      canSend: true,
      sessionLocked: locked,
    }),
    { initialProps: { locked: sessionLocked } },
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

describe("useComposerSession", () => {
  it("does not let an old send completion release a newer generation's flight", async () => {
    vi.useFakeTimers();
    const oldSave = deferred<DraftResult>();
    const newSave = deferred<DraftResult>();
    vi.mocked(api.saveDraft).mockReturnValueOnce(oldSave.promise).mockReturnValueOnce(newSave.promise);
    vi.mocked(api.requestSubmission).mockResolvedValue(deliveredSubmission);
    const { result } = renderComposer();
    act(() => {
      result.current.openComposer();
      result.current.changeTo("old@example.com");
    });
    let oldSend!: Promise<void>;
    await act(async () => { oldSend = result.current.send(); await flushMicrotasks(); });
    act(() => { result.current.clear(); result.current.openComposer(); result.current.changeTo("new@example.com"); });
    let newSend!: Promise<void>;
    await act(async () => { newSend = result.current.send(); await flushMicrotasks(); });
    await act(async () => { oldSave.resolve(draftResult); await oldSend; });
    expect(result.current.sendBusy).toBe(true);
    await act(async () => { await result.current.send(); });
    expect(api.saveDraft).toHaveBeenCalledTimes(2);
    expect(api.requestSubmission).not.toHaveBeenCalled();
    await act(async () => { newSave.resolve(draftResult); await newSend; });
    expect(api.requestSubmission).toHaveBeenCalledTimes(1);
    expect(result.current.sendBusy).toBe(false);
  });

  it("admits only one Send invocation within the same render frame", async () => {
    vi.useFakeTimers();
    const saving = deferred<DraftResult>();
    vi.mocked(api.saveDraft).mockReturnValueOnce(saving.promise).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockResolvedValue(deliveredSubmission);
    const { result } = renderComposer();
    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
    });
    let first!: Promise<void>;
    let duplicate!: Promise<void>;
    await act(async () => {
      first = result.current.send();
      duplicate = result.current.send();
      await flushMicrotasks();
      saving.resolve(draftResult);
      await Promise.all([first, duplicate]);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    expect(api.requestSubmission).toHaveBeenCalledTimes(1);
  });

  it("deduplicates same-frame unknown-result retries without replacing their idempotency key", async () => {
    vi.useFakeTimers();
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockRejectedValueOnce(new Error("network_unknown"))
      .mockResolvedValue(deliveredSubmission);
    const { result } = renderComposer();
    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
    });
    await act(async () => { await result.current.send(); });
    const key = vi.mocked(api.requestSubmission).mock.calls[0]?.[3];
    expect(result.current.draftStatus.key).toBe("delivery.requestUnknown");
    await act(async () => {
      await Promise.all([result.current.send(), result.current.send()]);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    expect(api.requestSubmission).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.requestSubmission).mock.calls[1]?.[3]).toBe(key);
  });

  it("reset releases an old observer waiting on a locked Session without another poll", async () => {
    vi.useFakeTimers();
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockResolvedValue({ ...deliveredSubmission, status: "pending", delivery: null });
    const { result, rerender } = renderComposer();
    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
    });
    let finished = false;
    await act(async () => {
      void result.current.send().then(() => { finished = true; });
      await flushMicrotasks();
    });
    act(() => rerender({ locked: true }));
    await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
    expect(finished).toBe(false);
    await act(async () => {
      result.current.clear();
      await flushMicrotasks();
    });
    expect(finished).toBe(true);
    expect(api.getSubmission).not.toHaveBeenCalled();
  });

  it("cancels a pending autosave while Session verification is locked and resumes after unlock", async () => {
    vi.useFakeTimers();
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    const { rerender, result } = renderComposer();

    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
      result.current.changeTextBody("Keep this exact Session-bound snapshot");
    });
    act(() => rerender({ locked: true }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2400);
    });
    expect(api.saveDraft).not.toHaveBeenCalled();

    act(() => rerender({ locked: false }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    expect(api.saveDraft).toHaveBeenCalledWith(mailboxSettings, expect.objectContaining({
      to: ["alice@example.com"],
      text_body: "Keep this exact Session-bound snapshot",
    }));
  });

  it("does not issue the next delivery poll until Session verification unlocks", async () => {
    vi.useFakeTimers();
    const queuedSubmission: MailSubmission = {
      ...deliveredSubmission,
      status: "pending",
      delivery: null,
    };
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockResolvedValue(queuedSubmission);
    vi.mocked(api.getSubmission).mockResolvedValue(deliveredSubmission);
    const { rerender, result } = renderComposer();

    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
    });
    let sending!: Promise<void>;
    await act(async () => {
      sending = result.current.send();
      await flushMicrotasks();
    });
    expect(api.requestSubmission).toHaveBeenCalledTimes(1);

    act(() => rerender({ locked: true }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.getSubmission).not.toHaveBeenCalled();

    act(() => rerender({ locked: false }));
    await act(async () => {
      await flushMicrotasks();
      await sending;
    });
    expect(api.getSubmission).toHaveBeenCalledTimes(1);
    expect(result.current.terminalSubmission).toBe(true);
  });

  it("does not let an old autosave bind a new reply session", async () => {
    vi.useFakeTimers();
    const staleSave = deferred<DraftResult>();
    vi.mocked(api.saveDraft)
      .mockReturnValueOnce(staleSave.promise)
      .mockResolvedValueOnce({
        ...draftResult,
        draft_id: "019b0000-0000-7000-8000-000000000031",
        reply_to_message_id: "019b0000-0000-7000-8000-000000000001",
      });
    const { result } = renderComposer();

    act(() => {
      result.current.openComposer();
      result.current.changeTo("old-session@example.com");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.openReply({
        to: "alice@example.com",
        subject: "Re: Current thread",
        replyToMessageId: "019b0000-0000-7000-8000-000000000001",
      });
      result.current.changeTextBody("Fresh reply body");
    });
    await act(async () => {
      staleSave.resolve(draftResult);
      await flushMicrotasks();
      await vi.advanceTimersByTimeAsync(1200);
    });

    expect(api.saveDraft).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.saveDraft).mock.calls[1]?.[1]).toMatchObject({
      draft_id: null,
      expected_version: null,
      reply_to_message_id: "019b0000-0000-7000-8000-000000000001",
      to: ["alice@example.com"],
      text_body: "Fresh reply body",
    });
  });

  it("preserves the restored draft version and skips only its first autosave", async () => {
    vi.useFakeTimers();
    vi.mocked(api.saveDraft).mockResolvedValue({ ...draftResult, version: 5 });
    const { result } = renderComposer();

    act(() => result.current.restoreFromDraft(restoredDraft));
    expect(result.current.composerKind).toBe("restored");
    expect(result.current.textBody).toBe("Restored body");
    expect(api.saveDraft).not.toHaveBeenCalled();

    act(() => result.current.changeTextBody("Edited after restore"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });

    expect(api.saveDraft).toHaveBeenCalledWith(mailboxSettings, {
      draft_id: restoredDraft.draft_id,
      expected_version: restoredDraft.version,
      reply_to_message_id: null,
      to: restoredDraft.to,
      cc: restoredDraft.cc,
      bcc: restoredDraft.bcc,
      subject: restoredDraft.subject,
      text_body: "Edited after restore",
    });
  });

  it("does not report an unsavable recipient-free snapshot as saved", async () => {
    vi.useFakeTimers();
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    const { result } = renderComposer();

    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
      result.current.changeTextBody("Persist this exact snapshot");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });
    expect(result.current.draftStatus.key).toBe("draft.saved");

    act(() => {
      result.current.changeTo("");
      result.current.changeTextBody("This local-only edit cannot be autosaved yet");
    });
    expect(result.current.draftStatus.key).toBe("draft.autosaveHint");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2400);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    expect(result.current.draftStatus.key).toBe("draft.autosaveHint");
  });

  it("reuses the idempotency key when the first submission outcome is unknown", async () => {
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission)
      .mockRejectedValueOnce(new Error("request may have reached the server"))
      .mockResolvedValueOnce(deliveredSubmission);
    const { result, unmount } = renderComposer();

    act(() => {
      result.current.openComposer();
      result.current.changeTo("alice@example.com");
    });
    await act(async () => {
      await result.current.send();
    });

    expect(result.current.submissionUnresolved).toBe(true);
    const initialKey = vi.mocked(api.requestSubmission).mock.calls[0]?.[3];
    expect(initialKey).toBeTruthy();

    await act(async () => {
      await result.current.send();
    });

    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    expect(api.requestSubmission).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.requestSubmission).mock.calls[1]?.[3]).toBe(initialKey);
    expect(result.current.terminalSubmission).toBe(true);
    unmount();
  });

  it("reopens one exact manual-review submission instead of clearing it for a new message", async () => {
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockResolvedValue(manualReviewSubmission);
    const { result } = renderComposer();

    act(() => {
      result.current.openComposer();
      result.current.changeTo("review-target@example.com");
      result.current.changeSubject("Evidence must remain bound");
      result.current.changeTextBody("Frozen review body");
    });
    await act(async () => {
      await result.current.send();
    });

    expect(result.current.terminalSubmission).toBe(true);
    expect(result.current.submissionUnresolved).toBe(true);
    expect(result.current.manualReviewSubmissionId).toBe(
      manualReviewSubmission.submission_id,
    );
    expect(result.current.draftStatus.key).toBe("delivery.manualReview");
    expect(result.current.submissionEvidence).toEqual({
      submissionId: manualReviewSubmission.submission_id,
      status: "manual_review",
      stateVersion: manualReviewSubmission.state_version,
      updatedAt: manualReviewSubmission.updated_at,
      delivery: null,
    });
    expect(result.current.submissionEvidence).not.toHaveProperty("mailbox_id");
    expect(result.current.submissionEvidence).not.toHaveProperty("draft_id");
    expect(result.current.submissionEvidence).not.toHaveProperty("delivery.recipients");

    act(() => result.current.close());
    expect(result.current.composeOpen).toBe(false);
    act(() => result.current.openComposer());

    expect(result.current.composeOpen).toBe(true);
    expect(result.current.to).toBe("review-target@example.com");
    expect(result.current.subject).toBe("Evidence must remain bound");
    expect(result.current.textBody).toBe("Frozen review body");
    expect(result.current.manualReviewSubmissionId).toBe(
      manualReviewSubmission.submission_id,
    );
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    expect(api.requestSubmission).toHaveBeenCalledTimes(1);
  });
});
