// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import {
  mergeDraftDirectoryPages,
  useDraftDirectory,
  type UseDraftDirectoryOptions,
} from "./useDraftDirectory";
import type {
  HumanDraftPage,
  HumanDraftRestore,
  MailboxScopedConfig,
} from "./types";

vi.mock("./api", async (importOriginal) => ({
  ...await importOriginal<typeof import("./api")>(),
  listDrafts: vi.fn(),
  restoreDraft: vi.fn(),
}));

const primaryMailbox: MailboxScopedConfig = {
  apiBaseUrl: "",
  mailboxId: "019b0000-0000-7000-8000-000000000020",
};
const secondaryMailbox: MailboxScopedConfig = {
  apiBaseUrl: "",
  mailboxId: "019b0000-0000-7000-8000-000000000021",
};
const newestDraft = {
  draft_id: "019b0000-0000-7000-8000-000000000090",
  version: 4,
  reply_to_message_id: null,
  recipient_count: 2,
  created_at: "2026-07-27T11:00:00.000Z",
  updated_at: "2026-07-27T12:00:00.000Z",
};
const olderDraft = {
  ...newestDraft,
  draft_id: "019b0000-0000-7000-8000-000000000089",
  version: 3,
  updated_at: "2026-07-27T11:59:00.000Z",
};
const firstPage: HumanDraftPage = {
  items: [newestDraft],
  next_cursor: `v1.${newestDraft.draft_id}`,
  has_more: true,
};
const finalPage: HumanDraftPage = {
  items: [olderDraft],
  next_cursor: null,
  has_more: false,
};
const restoredDraft: HumanDraftRestore = {
  draft_id: newestDraft.draft_id,
  version: newestDraft.version,
  reply_to_message_id: null,
  to: ["alice@example.com"],
  cc: [],
  bcc: [],
  subject: "Recovered message",
  text_body: "Recovered body",
};

const activeOptions: UseDraftDirectoryOptions = {
  active: true,
  authenticated: true,
  canSend: true,
  mailboxSettings: primaryMailbox,
  hasComposerContent: () => false,
  hasUnresolvedSubmission: () => false,
  matchesRestoredDraft: () => false,
  reopenRestoredDraft: () => undefined,
  restoreIntoComposer: () => undefined,
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
  for (let index = 0; index < 10; index += 1) await Promise.resolve();
}

function DraftDirectoryHarness(props: UseDraftDirectoryOptions) {
  const directory = useDraftDirectory(props);
  return (
    <>
      <button type="button">Elsewhere</button>
      <h1 ref={directory.headingRef} tabIndex={-1}>Drafts</h1>
      <button type="button" onClick={() => directory.refresh()}>Refresh</button>
      <output data-testid="draft-list-busy">{directory.listBusy ? "busy" : "idle"}</output>
      <output data-testid="draft-list-error">{directory.listError ?? ""}</output>
      <output data-testid="draft-restore-error">{directory.restoreError ?? ""}</output>
      <output data-testid="draft-pending">
        {directory.pendingRestore?.draft_id ?? ""}
      </output>
      {directory.pendingRestore && (
        <button type="button" onClick={() => directory.confirmRestore()}>
          Confirm restore
        </button>
      )}
      {directory.page?.items.map((draft) => (
        <button
          key={draft.draft_id}
          ref={directory.draftButtonRef(draft.draft_id)}
          type="button"
          onClick={() => directory.openDraft(draft)}
        >
          {draft.draft_id}
        </button>
      ))}
    </>
  );
}

beforeEach(() => {
  window.requestAnimationFrame = (callback) => window.setTimeout(
    () => callback(performance.now()),
    0,
  );
  vi.mocked(api.listDrafts).mockReset();
  vi.mocked(api.restoreDraft).mockReset();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("useDraftDirectory", () => {
  it("synchronously deactivates, aborts, and discards a late list response", async () => {
    const latePage = deferred<HumanDraftPage>();
    vi.mocked(api.listDrafts).mockReturnValueOnce(latePage.promise);
    const { result } = renderHook(() => useDraftDirectory(activeOptions));

    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(api.listDrafts).mock.calls[0]?.[2];
    act(() => result.current.deactivate());

    expect(signal?.aborted).toBe(true);
    expect(result.current.page).toBeUndefined();
    expect(result.current.listBusy).toBe(false);
    expect(result.current.listError).toBeUndefined();

    await act(async () => {
      latePage.resolve(firstPage);
      await flushMicrotasks();
    });
    expect(result.current.page).toBeUndefined();
  });

  it("aborts the old mailbox list and only accepts the new mailbox response", async () => {
    const oldMailbox = deferred<HumanDraftPage>();
    const newMailbox = deferred<HumanDraftPage>();
    vi.mocked(api.listDrafts)
      .mockReturnValueOnce(oldMailbox.promise)
      .mockReturnValueOnce(newMailbox.promise);
    const { rerender, result } = renderHook(
      (props: UseDraftDirectoryOptions) => useDraftDirectory(props),
      { initialProps: activeOptions },
    );

    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(1));
    const oldSignal = vi.mocked(api.listDrafts).mock.calls[0]?.[2];
    rerender({ ...activeOptions, mailboxSettings: secondaryMailbox });

    expect(oldSignal?.aborted).toBe(true);
    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api.listDrafts).mock.calls[1]?.[0]).toEqual(secondaryMailbox);

    await act(async () => {
      oldMailbox.resolve(firstPage);
      await flushMicrotasks();
    });
    expect(result.current.page).toBeUndefined();

    await act(async () => {
      newMailbox.resolve(finalPage);
      await flushMicrotasks();
    });
    expect(result.current.page).toEqual(finalPage);
  });

  it("synchronously aborts an in-flight restore and never hands it to the composer", async () => {
    const lateRestore = deferred<HumanDraftRestore>();
    const restoreIntoComposer = vi.fn();
    vi.mocked(api.listDrafts).mockResolvedValueOnce(finalPage);
    vi.mocked(api.restoreDraft).mockReturnValueOnce(lateRestore.promise);
    const { result } = renderHook(() => useDraftDirectory({
      ...activeOptions,
      restoreIntoComposer,
    }));

    await waitFor(() => expect(result.current.page).toEqual(finalPage));
    act(() => { result.current.openDraft(olderDraft); });
    await waitFor(() => expect(api.restoreDraft).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(api.restoreDraft).mock.calls[0]?.[2];

    act(() => result.current.deactivate());
    expect(signal?.aborted).toBe(true);
    expect(result.current.restoreBusyId).toBeUndefined();

    await act(async () => {
      lateRestore.resolve(restoredDraft);
      await flushMicrotasks();
    });
    expect(restoreIntoComposer).not.toHaveBeenCalled();
  });

  it("keeps directory and pending restore memory while Session verification is locked", async () => {
    const restoreIntoComposer = vi.fn();
    vi.mocked(api.listDrafts)
      .mockResolvedValueOnce(finalPage)
      .mockResolvedValueOnce(finalPage);
    vi.mocked(api.restoreDraft).mockResolvedValueOnce(restoredDraft);
    const { rerender, result } = renderHook(
      (props: UseDraftDirectoryOptions) => useDraftDirectory(props),
      {
        initialProps: {
          ...activeOptions,
          hasComposerContent: () => true,
          restoreIntoComposer,
        },
      },
    );

    await waitFor(() => expect(result.current.page).toEqual(finalPage));
    act(() => { result.current.openDraft(olderDraft); });
    expect(result.current.pendingRestore).toEqual(olderDraft);

    rerender({
      ...activeOptions,
      hasComposerContent: () => true,
      restoreIntoComposer,
      sessionLocked: true,
    });
    expect(result.current.page).toEqual(finalPage);
    expect(result.current.pendingRestore).toEqual(olderDraft);
    expect(api.restoreDraft).not.toHaveBeenCalled();

    rerender({
      ...activeOptions,
      hasComposerContent: () => true,
      restoreIntoComposer,
      sessionLocked: false,
    });
    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(2));
    expect(result.current.pendingRestore).toEqual(olderDraft);
    act(() => { result.current.confirmRestore(); });
    await waitFor(() => expect(restoreIntoComposer).toHaveBeenCalledWith(restoredDraft));
  });

  it("rejects a non-progressing continuation without replacing the current page", async () => {
    const nonProgressingPage: HumanDraftPage = {
      items: [olderDraft],
      next_cursor: firstPage.next_cursor,
      has_more: true,
    };
    vi.mocked(api.listDrafts)
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(nonProgressingPage);
    const { result } = renderHook(() => useDraftDirectory(activeOptions));

    await waitFor(() => expect(result.current.page).toEqual(firstPage));
    await act(async () => {
      expect(await result.current.loadMore()).toBe(false);
    });

    expect(result.current.page).toEqual(firstPage);
    expect(result.current.listError).toBe("error.draftsPageUnavailable");
    expect(result.current.listBusy).toBe(false);
  });

  it("rejects out-of-order and repeated draft identities before they reach the view", () => {
    expect(() => mergeDraftDirectoryPages(
      [newestDraft],
      [{ ...newestDraft, draft_id: "019b0000-0000-7000-8000-000000000091" }],
    )).toThrow("draft_list_order_conflict");
    expect(() => mergeDraftDirectoryPages(
      [newestDraft, olderDraft],
      [{ ...newestDraft, draft_id: "019b0000-0000-7000-8000-000000000088" }, newestDraft],
    )).toThrow("draft_list_identity_conflict");
  });

  it("keeps restore conflict recovery bounded, refreshes metadata, and returns focus to the draft", async () => {
    vi.mocked(api.listDrafts)
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce({ ...firstPage, items: [newestDraft], has_more: false, next_cursor: null });
    vi.mocked(api.restoreDraft).mockRejectedValueOnce(new Error("draft_restore_409"));
    render(<DraftDirectoryHarness {...activeOptions} />);

    const draftButton = await screen.findByRole("button", { name: newestDraft.draft_id });
    fireEvent.click(draftButton);

    await waitFor(() => expect(api.restoreDraft).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(draftButton).toHaveFocus());
    expect(screen.getByTestId("draft-restore-error")).toHaveTextContent(
      "error.draftRestoreConflict",
    );
  });

  it("focuses the failed restore action and does not issue a restore while submission state is unresolved", async () => {
    const failedRestore = deferred<HumanDraftRestore>();
    vi.mocked(api.listDrafts).mockResolvedValueOnce(finalPage);
    vi.mocked(api.restoreDraft).mockReturnValueOnce(failedRestore.promise);
    render(<DraftDirectoryHarness {...activeOptions} />);

    const draftButton = await screen.findByRole("button", { name: olderDraft.draft_id });
    fireEvent.click(draftButton);
    screen.getByRole("button", { name: "Elsewhere" }).focus();
    await act(async () => {
      failedRestore.reject(new Error("network"));
      await flushMicrotasks();
    });
    await waitFor(() => expect(draftButton).toHaveFocus());
    expect(screen.getByTestId("draft-restore-error")).toHaveTextContent(
      "error.draftRestoreUnavailable",
    );

    cleanup();
    vi.mocked(api.listDrafts).mockResolvedValueOnce(finalPage);
    render(<DraftDirectoryHarness
      {...activeOptions}
      hasUnresolvedSubmission={() => true}
    />);
    const lockedButton = await screen.findByRole("button", { name: olderDraft.draft_id });
    fireEvent.click(lockedButton);
    expect(api.restoreDraft).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("draft-restore-error")).toHaveTextContent(
      "error.draftSubmissionLocked",
    );
  });

  it("keeps confirmation state in the directory until the restored draft is handed to the composer", async () => {
    const restoreIntoComposer = vi.fn();
    vi.mocked(api.listDrafts).mockResolvedValueOnce(finalPage);
    vi.mocked(api.restoreDraft).mockResolvedValueOnce(restoredDraft);
    render(<DraftDirectoryHarness
      {...activeOptions}
      hasComposerContent={() => true}
      restoreIntoComposer={restoreIntoComposer}
    />);

    fireEvent.click(await screen.findByRole("button", { name: olderDraft.draft_id }));
    expect(api.restoreDraft).not.toHaveBeenCalled();
    expect(screen.getByTestId("draft-pending")).toHaveTextContent(olderDraft.draft_id);
    fireEvent.click(screen.getByRole("button", { name: "Confirm restore" }));
    await waitFor(() => expect(restoreIntoComposer).toHaveBeenCalledWith(restoredDraft));
    expect(screen.getByTestId("draft-pending")).toBeEmptyDOMElement();
  });
});
