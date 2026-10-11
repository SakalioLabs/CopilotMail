import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import {
  mergeSentDirectoryPages,
  useSentDirectory,
  type UseSentDirectoryOptions,
} from "./useSentDirectory";
import type { HumanSentSubmissionPage, MailboxScopedConfig } from "./types";

vi.mock("./api", async (importOriginal) => ({
  ...await importOriginal<typeof import("./api")>(),
  listSentSubmissions: vi.fn(),
}));

const primaryMailbox: MailboxScopedConfig = {
  apiBaseUrl: "",
  mailboxId: "019b0000-0000-7000-8000-000000000020",
};
const secondaryMailbox: MailboxScopedConfig = {
  apiBaseUrl: "",
  mailboxId: "019b0000-0000-7000-8000-000000000021",
};

const newestSubmission = {
  submission_id: "019b0000-0000-7000-8000-000000000080",
  accepted_at: "2026-07-27T12:00:00.000Z",
  recipient_count: 2,
};
const olderSubmission = {
  submission_id: "019b0000-0000-7000-8000-000000000079",
  accepted_at: "2026-07-27T11:59:00.000Z",
  recipient_count: 1,
};
const firstPage: HumanSentSubmissionPage = {
  items: [newestSubmission],
  next_cursor: "v1.019b0000-0000-7000-8000-000000000080",
  has_more: true,
};
const finalPage: HumanSentSubmissionPage = {
  items: [olderSubmission],
  next_cursor: null,
  has_more: false,
};

const activeOptions: UseSentDirectoryOptions = {
  active: true,
  authenticated: true,
  canSend: true,
  mailboxSettings: primaryMailbox,
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

function SentHeadingHarness(props: UseSentDirectoryOptions) {
  const directory = useSentDirectory(props);
  return (
    <>
      <h1 ref={directory.headingRef} tabIndex={-1}>Sent</h1>
      <button type="button" onClick={() => directory.refresh()}>Refresh</button>
      <output>{directory.page?.items.length ?? 0}</output>
      <span data-testid="sent-directory-busy">
        {directory.listBusy ? "busy" : "idle"}
      </span>
    </>
  );
}

beforeEach(() => {
  window.requestAnimationFrame = (callback) => window.setTimeout(
    () => callback(performance.now()),
    0,
  );
  vi.mocked(api.listSentSubmissions).mockReset();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("useSentDirectory", () => {
  it("keeps initial loading separate from an intentional refresh and focuses the title only after the refresh succeeds", async () => {
    const initial = deferred<HumanSentSubmissionPage>();
    const refreshed = deferred<HumanSentSubmissionPage>();
    vi.mocked(api.listSentSubmissions)
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(refreshed.promise);

    render(<SentHeadingHarness {...activeOptions} />);
    const heading = screen.getByRole("heading", { name: "Sent" });
    expect(screen.getByRole("status")).toHaveTextContent("0");
    expect(screen.getByTestId("sent-directory-busy")).toHaveTextContent("busy");
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(1));

    await act(async () => {
      initial.resolve(firstPage);
      await flushMicrotasks();
    });
    expect(screen.getByRole("status")).toHaveTextContent("1");
    expect(screen.getByTestId("sent-directory-busy")).toHaveTextContent("idle");
    expect(heading).not.toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(2));
    expect(screen.getByTestId("sent-directory-busy")).toHaveTextContent("busy");
    await act(async () => {
      refreshed.resolve(finalPage);
      await flushMicrotasks();
    });

    await waitFor(() => expect(heading).toHaveFocus());
    expect(screen.getByRole("status")).toHaveTextContent("1");
  });

  it("synchronously deactivates, aborts, and discards a late directory response", async () => {
    const latePage = deferred<HumanSentSubmissionPage>();
    vi.mocked(api.listSentSubmissions).mockReturnValueOnce(latePage.promise);
    const { result } = renderHook(() => useSentDirectory(activeOptions));

    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(api.listSentSubmissions).mock.calls[0]?.[2];
    act(() => result.current.deactivate());

    expect(signal?.aborted).toBe(true);
    expect(result.current.page).toBeUndefined();
    expect(result.current.listBusy).toBe(false);
    expect(result.current.error).toBeUndefined();

    await act(async () => {
      latePage.resolve(firstPage);
      await flushMicrotasks();
    });
    expect(result.current.page).toBeUndefined();
  });

  it("aborts the old mailbox request and only accepts the new mailbox response", async () => {
    const oldMailbox = deferred<HumanSentSubmissionPage>();
    const newMailbox = deferred<HumanSentSubmissionPage>();
    vi.mocked(api.listSentSubmissions)
      .mockReturnValueOnce(oldMailbox.promise)
      .mockReturnValueOnce(newMailbox.promise);
    const { rerender, result } = renderHook(
      (props: UseSentDirectoryOptions) => useSentDirectory(props),
      { initialProps: activeOptions },
    );

    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(1));
    const oldSignal = vi.mocked(api.listSentSubmissions).mock.calls[0]?.[2];
    rerender({ ...activeOptions, mailboxSettings: secondaryMailbox });

    expect(oldSignal?.aborted).toBe(true);
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api.listSentSubmissions).mock.calls[1]?.[0]).toEqual(secondaryMailbox);

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

  it("aborts an in-flight continuation when the directory unmounts", async () => {
    const continuation = deferred<HumanSentSubmissionPage>();
    vi.mocked(api.listSentSubmissions)
      .mockResolvedValueOnce(firstPage)
      .mockReturnValueOnce(continuation.promise);
    const { result, unmount } = renderHook(() => useSentDirectory(activeOptions));

    await waitFor(() => expect(result.current.page).toEqual(firstPage));
    act(() => { void result.current.loadMore(); });
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(2));
    const continuationSignal = vi.mocked(api.listSentSubmissions).mock.calls[1]?.[2];

    unmount();
    expect(continuationSignal?.aborted).toBe(true);
    await act(async () => {
      continuation.resolve(finalPage);
      await flushMicrotasks();
    });
  });

  it("keeps the previous page and exposes a bounded error when a continuation repeats an identity", async () => {
    const duplicatePage: HumanSentSubmissionPage = {
      items: [{ ...newestSubmission }],
      next_cursor: null,
      has_more: false,
    };
    vi.mocked(api.listSentSubmissions)
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(duplicatePage);
    const { result } = renderHook(() => useSentDirectory(activeOptions));

    await waitFor(() => expect(result.current.page).toEqual(firstPage));
    await act(async () => {
      expect(await result.current.loadMore()).toBe(false);
    });

    expect(result.current.page).toEqual(firstPage);
    expect(result.current.error).toBe("error.sentPageUnavailable");
    expect(result.current.listBusy).toBe(false);
  });

  it("rejects non-progressing and duplicate cursor continuations before they reach the view", () => {
    expect(() => mergeSentDirectoryPages(
      [newestSubmission],
      [{ ...newestSubmission, submission_id: "019b0000-0000-7000-8000-000000000081" }],
    )).toThrow("sent_list_order_conflict");
    expect(() => mergeSentDirectoryPages(
      [newestSubmission, olderSubmission],
      [{
        ...olderSubmission,
        submission_id: "019b0000-0000-7000-8000-000000000078",
        accepted_at: "2026-07-27T11:58:00.000Z",
      }, { ...newestSubmission }],
    )).toThrow("sent_list_identity_conflict");
  });
});
