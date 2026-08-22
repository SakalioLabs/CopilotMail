import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import App from "./App";
import * as auth from "./auth";
import { LOCALE_STORAGE_KEY } from "./i18n";
import type {
  DraftResult,
  HumanDraftPage,
  HumanDraftRestore,
  InboxPage,
  MailboxDirectoryPage,
  MailSubmission,
  MessageDetail,
  MessageState,
  MessageSummary,
} from "./types";

vi.mock("./api", () => ({
  getMessage: vi.fn(),
  getSubmission: vi.fn(),
  listDrafts: vi.fn(),
  listInbox: vi.fn(),
  listMailboxes: vi.fn(),
  requestSubmission: vi.fn(),
  restoreDraft: vi.fn(),
  saveDraft: vi.fn(),
  searchInbox: vi.fn(),
  updateMessageState: vi.fn(),
}));

vi.mock("./auth", () => ({
  loadSession: vi.fn(),
  logout: vi.fn(),
  startLogin: vi.fn(),
}));

const message: MessageSummary = {
  message_id: "019b0000-0000-7000-8000-000000000001",
  subject: "Quarterly review",
  from: [{ name: "Alice", email: "alice@example.com" }],
  received_at: "2026-07-27T12:00:00.000Z",
  preview: "A short preview",
  is_read: true,
  is_flagged: false,
  has_attachment: false,
};

const page: InboxPage = {
  items: [{ ...message }],
  next_cursor: null,
  has_more: false,
};

const mailboxDirectory: MailboxDirectoryPage = {
  items: [{
    mailbox_id: "019b0000-0000-7000-8000-000000000020",
    kind: "human",
    primary_address: "reader@example.com",
    can_send: true,
  }],
  next_cursor: null,
  has_more: false,
};

function inboxPage(
  subject: string,
  messageId: string,
  nextCursor: string | null = null,
): InboxPage {
  return {
    items: [{ ...message, message_id: messageId, subject }],
    next_cursor: nextCursor,
    has_more: nextCursor !== null,
  };
}

const detail: MessageDetail = {
  message: { ...message },
  reply_to_message_id: message.message_id,
  to: [{ name: null, email: "reader@example.com" }],
  cc: [],
  text_body: "The safe plain-text message body.",
  text_body_truncated: false,
  attachments: [],
};

const session = {
  principal_id: "019b0000-0000-7000-8000-000000000010",
  organization_id: "019b0000-0000-7000-8000-000000000011",
  client: "mail" as const,
  expires_at: "2026-07-27T13:00:00.000Z",
};

const draftResult: DraftResult = {
  draft_id: "019b0000-0000-7000-8000-000000000030",
  version: 1,
  reply_to_message_id: null,
  previous_draft_destroyed: true,
};

const listedDraftId = "019b0000-0000-7000-8000-000000000090";
const olderDraftId = "019b0000-0000-7000-8000-000000000089";
const draftPage: HumanDraftPage = {
  items: [{
    draft_id: listedDraftId,
    version: 4,
    reply_to_message_id: null,
    recipient_count: 2,
    created_at: "2026-07-27T11:00:00.000Z",
    updated_at: "2026-07-27T12:00:00.000Z",
  }],
  next_cursor: null,
  has_more: false,
};
const restoredDraft: HumanDraftRestore = {
  draft_id: listedDraftId,
  version: 4,
  reply_to_message_id: null,
  to: ["alice@example.com"],
  cc: ["bob@example.com"],
  bcc: ["audit@example.com"],
  subject: "Restored private subject",
  text_body: "Restored private body",
};

const deliveredSubmission: MailSubmission = {
  submission_id: "019b0000-0000-7000-8000-000000000040",
  mailbox_id: "019b0000-0000-7000-8000-000000000020",
  draft_id: draftResult.draft_id,
  draft_version: 1,
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
    recipients: [{
      recipient_ref: "a".repeat(64),
      status: "delivered",
      smtp_reply: "250 delivered",
    }],
  },
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
  for (let index = 0; index < 12; index += 1) await Promise.resolve();
}

function setNarrowViewport(narrow: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: narrow && query === "(max-width: 900px)",
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem(LOCALE_STORAGE_KEY, "zh-CN");
  document.documentElement.lang = "";
  window.__COPILOTMAIL_MAIL_CONFIG__ = {
    apiBaseUrl: "",
    mailboxId: "019b0000-0000-7000-8000-000000000020",
  };
  setNarrowViewport(false);
  window.requestAnimationFrame = (callback) =>
    window.setTimeout(() => callback(performance.now()), 0);
  vi.mocked(auth.loadSession).mockResolvedValue(session);
  vi.mocked(auth.logout).mockResolvedValue(undefined);
  vi.mocked(api.listMailboxes).mockResolvedValue(mailboxDirectory);
  vi.mocked(api.listInbox).mockResolvedValue(page);
  vi.mocked(api.listDrafts).mockResolvedValue(draftPage);
  vi.mocked(api.restoreDraft).mockResolvedValue(restoredDraft);
  vi.mocked(api.getMessage).mockResolvedValue(detail);
  vi.mocked(api.updateMessageState).mockResolvedValue({
    message_id: message.message_id,
    is_read: true,
    is_flagged: false,
  });
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  document.body.replaceChildren();
  localStorage.clear();
  vi.clearAllMocks();
});

describe("human mail application shell", () => {
  it("exposes desktop and mobile Drafts entry points and keeps the list metadata-only", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    setNarrowViewport(true);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const desktopDrafts = within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" });
    const mobileDrafts = within(container.querySelector<HTMLElement>(".mobile-view-tabs")!)
      .getByRole("button", { name: "Drafts" });
    expect(desktopDrafts).toBeEnabled();
    expect(mobileDrafts).toBeEnabled();

    fireEvent.click(mobileDrafts);

    const heading = await screen.findByRole("heading", { name: "Drafts" });
    await waitFor(() => expect(heading).toHaveFocus());
    expect(await screen.findByRole("button", { name: /Resume editing/ }))
      .toHaveAccessibleName(/2 recipients/);
    expect(api.listDrafts).toHaveBeenCalledWith(
      expect.objectContaining({ mailboxId: mailboxDirectory.items[0].mailbox_id }),
      undefined,
      expect.any(AbortSignal),
    );
    expect(screen.queryByText(restoredDraft.subject)).not.toBeInTheDocument();
    expect(screen.queryByText(restoredDraft.text_body)).not.toBeInTheDocument();
  });

  it("renders an empty draft state and can refresh it without retaining content", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.listDrafts)
      .mockReset()
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false })
      .mockResolvedValueOnce(draftPage);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));

    expect(await screen.findByText("This mailbox has no draft that can be restored"))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh drafts" }));

    expect(await screen.findByRole("button", { name: /Resume editing/ }))
      .toBeInTheDocument();
    expect(api.listDrafts).toHaveBeenCalledTimes(2);
  });

  it("shows bounded draft errors and appends only a progressing next page", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const firstPage: HumanDraftPage = {
      ...draftPage,
      next_cursor: `v1.${listedDraftId}`,
      has_more: true,
    };
    const nextPage: HumanDraftPage = {
      items: [{
        ...draftPage.items[0],
        draft_id: olderDraftId,
        recipient_count: 1,
      }],
      next_cursor: null,
      has_more: false,
    };
    vi.mocked(api.listDrafts)
      .mockReset()
      .mockRejectedValueOnce(new Error("secret backend detail"))
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(nextPage);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Drafts are temporarily unavailable",
    );
    expect(screen.queryByText("secret backend detail")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh drafts" }));
    const loadMore = await screen.findByRole("button", { name: "Load more drafts" });
    fireEvent.click(loadMore);

    await waitFor(() => expect(screen.getAllByRole("button", { name: /Resume editing/ }))
      .toHaveLength(2));
    expect(api.listDrafts).toHaveBeenLastCalledWith(
      expect.objectContaining({ mailboxId: mailboxDirectory.items[0].mailbox_id }),
      `v1.${listedDraftId}`,
      expect.any(AbortSignal),
    );
  });

  it("restores the exact draft version into the current mailbox composer and autosaves from it", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.saveDraft).mockResolvedValue({
      ...draftResult,
      draft_id: listedDraftId,
      version: 5,
    });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));
    fireEvent.click(await screen.findByRole("button", { name: /Resume editing/ }));

    const composer = await screen.findByRole("dialog", { name: "Edit draft" });
    expect(api.restoreDraft).toHaveBeenCalledWith(
      expect.objectContaining({ mailboxId: mailboxDirectory.items[0].mailbox_id }),
      expect.objectContaining({ draft_id: listedDraftId, version: 4 }),
      expect.any(AbortSignal),
    );
    expect(within(composer).getByLabelText("From")).toHaveValue("reader@example.com");
    expect(within(composer).getByLabelText("To")).toHaveValue("alice@example.com");
    expect(within(composer).getByLabelText("Cc")).toHaveValue("bob@example.com");
    expect(within(composer).getByLabelText("Bcc")).toHaveValue("audit@example.com");
    expect(within(composer).getByLabelText("Subject")).toHaveValue(restoredDraft.subject);
    expect(within(composer).getByLabelText("Message body")).toHaveValue(
      restoredDraft.text_body,
    );
    expect(within(composer).getByLabelText("Message body")).toHaveFocus();
    expect(api.saveDraft).not.toHaveBeenCalled();
    expect(JSON.stringify({ ...localStorage, ...sessionStorage })).not.toContain(
      restoredDraft.text_body,
    );

    vi.useFakeTimers();
    fireEvent.change(within(composer).getByLabelText("Message body"), {
      target: { value: "Edited after restore" },
    });
    await act(async () => {
      vi.advanceTimersByTime(1200);
      await flushMicrotasks();
    });

    expect(api.saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ mailboxId: mailboxDirectory.items[0].mailbox_id }),
      {
        draft_id: listedDraftId,
        expected_version: 4,
        reply_to_message_id: null,
        to: ["alice@example.com"],
        cc: ["bob@example.com"],
        bcc: ["audit@example.com"],
        subject: restoredDraft.subject,
        text_body: "Edited after restore",
      },
    );
  });

  it("requires an accessible confirmation before replacing a different hidden composer", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const restoreResponse = deferred<HumanDraftRestore>();
    vi.mocked(api.restoreDraft).mockReturnValueOnce(restoreResponse.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "someone@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Do not silently discard me" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));
    const restoreButton = await screen.findByRole("button", { name: /Resume editing/ });
    restoreButton.focus();
    fireEvent.click(restoreButton);

    const confirmation = screen.getByRole("alertdialog", {
      name: "Replace the current message?",
    });
    const cancel = within(confirmation).getByRole("button", { name: "Keep current content" });
    expect(cancel).toHaveFocus();
    expect(api.restoreDraft).not.toHaveBeenCalled();
    fireEvent.click(cancel);
    await waitFor(() => expect(restoreButton).toHaveFocus());

    fireEvent.click(restoreButton);
    fireEvent.click(within(screen.getByRole("alertdialog"))
      .getByRole("button", { name: "Restore draft" }));
    const busyConfirmation = screen.getByRole("alertdialog", {
      name: "Replace the current message?",
    });
    expect(busyConfirmation).toHaveAttribute("aria-busy", "true");
    expect(busyConfirmation).toHaveFocus();
    expect(within(busyConfirmation).getByRole("button", { name: "Restoring…" }))
      .toBeDisabled();
    await act(async () => {
      restoreResponse.resolve(restoredDraft);
      await flushMicrotasks();
    });
    expect(await screen.findByRole("dialog", { name: "Edit draft" })).toBeInTheDocument();
    expect(api.restoreDraft).toHaveBeenCalledTimes(1);
  });

  it("stops on a restore conflict, refreshes metadata, and never forks the draft", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.restoreDraft).mockRejectedValueOnce(new Error("draft_restore_409"));
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));
    fireEvent.click(await screen.findByRole("button", { name: /Resume editing/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This draft changed elsewhere; the list was refreshed",
    );
    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole("button", { name: /Resume editing/ }))
      .toHaveFocus());
    expect(api.saveDraft).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Edit draft" })).not.toBeInTheDocument();
  });

  it("aborts and ignores draft list and restore responses after view or mailbox changes", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const lateList = deferred<HumanDraftPage>();
    vi.mocked(api.listDrafts)
      .mockReset()
      .mockReturnValueOnce(lateList.promise)
      .mockResolvedValue(draftPage);
    const secondMailbox = {
      mailbox_id: "019b0000-0000-7000-8000-000000000021",
      kind: "shared" as const,
      primary_address: "team@example.com",
      can_send: true,
    };
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [...mailboxDirectory.items, secondMailbox],
      next_cursor: null,
      has_more: false,
    });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const sidebar = container.querySelector<HTMLElement>(".sidebar")!;
    fireEvent.click(within(sidebar).getByRole("button", { name: "Drafts" }));
    await waitFor(() => expect(api.listDrafts).toHaveBeenCalledTimes(1));
    const listSignal = vi.mocked(api.listDrafts).mock.calls[0]?.[2];
    fireEvent.click(within(sidebar).getByRole("button", { name: "Inbox" }));
    expect(listSignal?.aborted).toBe(true);
    await act(async () => {
      lateList.resolve(draftPage);
      await flushMicrotasks();
    });
    expect(screen.queryByRole("button", { name: /Resume editing/ })).not.toBeInTheDocument();

    fireEvent.click(within(sidebar).getByRole("button", { name: "Drafts" }));
    const lateRestore = deferred<HumanDraftRestore>();
    vi.mocked(api.restoreDraft).mockReset().mockReturnValueOnce(lateRestore.promise);
    fireEvent.click(await screen.findByRole("button", { name: /Resume editing/ }));
    const restoreSignal = vi.mocked(api.restoreDraft).mock.calls[0]?.[2];
    fireEvent.change(within(sidebar).getByRole("combobox", { name: "Switch mailbox" }), {
      target: { value: secondMailbox.mailbox_id },
    });
    expect(restoreSignal?.aborted).toBe(true);
    await act(async () => {
      lateRestore.resolve({ ...restoredDraft, subject: "Wrong mailbox content" });
      await flushMicrotasks();
    });
    expect(await screen.findByRole("heading", { name: "Inbox" })).toBeInTheDocument();
    expect(screen.queryByText("Wrong mailbox content")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Edit draft" })).not.toBeInTheDocument();
  });

  it("provides a focus-preserving mobile list-to-detail navigation", async () => {
    setNarrowViewport(true);
    const { container } = render(<App />);
    const messageButton = await screen.findByRole("button", { name: /Alice/ });

    fireEvent.click(messageButton);

    await waitFor(() => {
      expect(container.querySelector(".mail-shell")).toHaveClass("message-open");
      expect(screen.getByRole("heading", { name: "Quarterly review" }))
        .toHaveFocus();
    });
    expect(await screen.findByText("The safe plain-text message body."))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /返回收件箱/ }));

    await waitFor(() => {
      expect(container.querySelector(".mail-shell")).not.toHaveClass("message-open");
      expect(messageButton).toHaveFocus();
    });
  });

  it("keeps compose, sign-out, and locale controls reachable in the mobile toolbar", async () => {
    setNarrowViewport(true);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "收件箱" });
    const mobileCompose = container.querySelector<HTMLButtonElement>(".mobile-compose");
    const mobileSignOut = container.querySelector<HTMLButtonElement>(".mobile-sign-out");

    expect(mobileCompose).toHaveAccessibleName("写信");
    expect(mobileSignOut).toHaveAccessibleName("退出登录");
    expect(container.querySelector(".mobile-toolbar .language-picker select"))
      .toHaveAccessibleName("语言");
    expect(container.querySelector(".mobile-toolbar .mailbox-picker select"))
      .toHaveAccessibleName("切换邮箱");

    mobileCompose?.focus();
    fireEvent.click(mobileCompose!);
    expect(await screen.findByRole("dialog", { name: "新邮件" })).toBeInTheDocument();
    expect(container.querySelector(".mobile-toolbar")).toHaveAttribute("inert");

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(mobileCompose).toHaveFocus();
  });

  it("switches every visible label and the document language without a reload", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "收件箱" });
    expect(document.documentElement.lang).toBe("zh-CN");

    const localeSelectors = screen.getAllByRole("combobox", { name: "语言" });
    fireEvent.change(localeSelectors[0], { target: { value: "en" } });

    await screen.findByRole("heading", { name: "Inbox" });
    expect(document.documentElement.lang).toBe("en");
    expect(screen.getAllByRole("button", { name: "Compose" })).not.toHaveLength(0);
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("en");
  });

  it("does not let a late autosave result contaminate a new reply session", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const staleSave = deferred<DraftResult>();
    vi.mocked(api.saveDraft)
      .mockReturnValueOnce(staleSave.promise)
      .mockResolvedValueOnce({
        ...draftResult,
        draft_id: "019b0000-0000-7000-8000-000000000031",
        reply_to_message_id: message.message_id,
      });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "old-session@example.com" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    vi.useRealTimers();

    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");
    fireEvent.click(screen.getByRole("button", { name: "Reply" }));
    expect(screen.getByRole("dialog", { name: "Reply" })).toBeInTheDocument();

    await act(async () => {
      staleSave.resolve(draftResult);
      await flushMicrotasks();
    });
    await waitFor(() => expect(api.saveDraft).toHaveBeenCalledTimes(2), { timeout: 2500 });

    expect(vi.mocked(api.saveDraft).mock.calls[1]?.[1]).toMatchObject({
      draft_id: null,
      expected_version: null,
      reply_to_message_id: message.message_id,
      to: ["alice@example.com"],
    });
  });

  it("locks every editable field before awaiting the final draft save", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const finalSave = deferred<DraftResult>();
    vi.mocked(api.saveDraft).mockReturnValueOnce(finalSave.promise);
    vi.mocked(api.requestSubmission).mockResolvedValue(deliveredSubmission);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Subject"), {
      target: { value: "Frozen subject" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Frozen body" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(screen.getByLabelText("Subject")).toBeDisabled();
    expect(screen.getByLabelText("Message body")).toBeDisabled();
    expect(api.requestSubmission).not.toHaveBeenCalled();

    await act(async () => {
      finalSave.resolve(draftResult);
      await flushMicrotasks();
    });
    expect(api.requestSubmission).toHaveBeenCalledTimes(1);
  });

  it("freezes the clicked snapshot behind an older autosave and ignores later changes", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const olderSave = deferred<DraftResult>();
    vi.mocked(api.saveDraft)
      .mockReturnValueOnce(olderSave.promise)
      .mockResolvedValueOnce({ ...draftResult, version: 2 });
    vi.mocked(api.requestSubmission).mockResolvedValue({
      ...deliveredSubmission,
      draft_version: 2,
    });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Older autosave body" },
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(1200); });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);

    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Frozen send body" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Late mutation" },
    });
    expect(screen.getByLabelText("Message body")).toHaveValue("Frozen send body");

    await act(async () => {
      olderSave.resolve(draftResult);
      await flushMicrotasks();
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.saveDraft).mock.calls[1]?.[1]).toMatchObject({
      draft_id: draftResult.draft_id,
      expected_version: 1,
      text_body: "Frozen send body",
    });
    expect(vi.mocked(api.requestSubmission).mock.calls[0]?.[2]).toBe(2);
  });

  it("keeps an unknown submission immutable for an idempotent retry", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const unknownSubmission = deferred<MailSubmission>();
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockReturnValueOnce(unknownSubmission.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(api.requestSubmission).toHaveBeenCalledTimes(1));
    await act(async () => {
      unknownSubmission.reject(new Error("connection reset after request write"));
      await flushMicrotasks();
    });

    expect(screen.getByText(
      "The send request status is unknown; retrying will not send a duplicate",
    )).toBeInTheDocument();
    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(screen.getByLabelText("Message body")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Retry send" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));
    fireEvent.click(await screen.findByRole("button", { name: /Resume editing/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The current send status is not known, so another draft cannot be opened",
    );
    expect(api.restoreDraft).not.toHaveBeenCalled();
  });

  it("cannot bypass an unknown submission through closed-composer Reply or Compose", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockRejectedValueOnce(
      new Error("request may have reached the server"),
    );
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");

    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "frozen@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Subject"), {
      target: { value: "Frozen subject" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText(
      "The send request status is unknown; retrying will not send a duplicate",
    );
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    const reply = screen.getByRole("button", { name: "Reply" });
    expect(reply).toBeDisabled();
    fireEvent.click(reply);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    expect(screen.getByRole("dialog", { name: "New message" })).toBeInTheDocument();
    expect(screen.getByLabelText("To")).toHaveValue("frozen@example.com");
    expect(screen.getByLabelText("Subject")).toHaveValue("Frozen subject");
    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
  });

  it("continues reconciliation from the last bound submission after a poll becomes unknown", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const pending: MailSubmission = {
      ...deliveredSubmission,
      status: "pending",
      state_version: 1,
      updated_at: deliveredSubmission.created_at,
      delivery: null,
    };
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockResolvedValue(pending);
    vi.mocked(api.getSubmission)
      .mockRejectedValueOnce(new Error("poll response was lost"))
      .mockResolvedValueOnce(deliveredSubmission);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });

    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await act(async () => {
      await flushMicrotasks();
    });
    expect(screen.getByText("Queued for sending…")).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
      await flushMicrotasks();
    });
    expect(screen.getByText(
      "The send request status is unknown; retrying will not send a duplicate",
    )).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    fireEvent.click(screen.getByRole("button", { name: "Retry send" }));
    await act(async () => {
      await flushMicrotasks();
    });

    expect(api.requestSubmission).toHaveBeenCalledTimes(1);
    expect(api.getSubmission).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.getSubmission).mock.calls[0]?.[1]).toEqual(pending);
    expect(vi.mocked(api.getSubmission).mock.calls[1]?.[1]).toEqual(pending);
    expect(screen.getByText("Delivered to all 1 recipient servers")).toBeInTheDocument();
  });

  it("unlocks editing only when the final draft save definitively failed before submission", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const failedSave = deferred<DraftResult>();
    vi.mocked(api.saveDraft).mockReturnValueOnce(failedSave.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(screen.getByLabelText("To")).toBeDisabled();
    await act(async () => {
      failedSave.reject(new Error("draft rejected before submission"));
      await flushMicrotasks();
    });

    expect(screen.getByText(
      "The draft could not be saved before sending; review it and try again",
    )).toBeInTheDocument();
    expect(api.requestSubmission).not.toHaveBeenCalled();
    expect(screen.getByLabelText("To")).toBeEnabled();
    expect(screen.getByLabelText("Message body")).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Editable after definite failure" },
    });
    expect(screen.getByLabelText("Message body"))
      .toHaveValue("Editable after definite failure");
  });

  it("pins an unknown submission to its mailbox and only reopens the frozen composer", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const sharedMailboxId = "019b0000-0000-7000-8000-000000000021";
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [
        ...mailboxDirectory.items,
        {
          mailbox_id: sharedMailboxId,
          kind: "shared",
          primary_address: "support@example.com",
          can_send: true,
        },
      ],
      next_cursor: null,
      has_more: false,
    });
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission)
      .mockRejectedValueOnce(new Error("unknown first-mailbox outcome"));
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText(
      "The send request status is unknown; retrying will not send a duplicate",
    );
    expect(screen.getByLabelText("To")).toBeDisabled();
    const originalKey = vi.mocked(api.requestSubmission).mock.calls[0]?.[3];

    fireEvent.keyDown(document, { key: "Escape" });
    const mailboxPicker = container.querySelector<HTMLSelectElement>(
      ".sidebar .mailbox-picker select",
    )!;
    expect(mailboxPicker).toBeDisabled();
    fireEvent.change(mailboxPicker, { target: { value: sharedMailboxId } });
    expect(vi.mocked(api.listInbox).mock.calls.at(-1)?.[0].mailboxId).toBe(
      mailboxDirectory.items[0].mailbox_id,
    );
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);

    expect(screen.getByLabelText("From")).toHaveValue("reader@example.com");
    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(screen.getByLabelText("To")).toHaveValue("alice@example.com");
    expect(screen.getByLabelText("Message body")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Retry send" }));
    await waitFor(() => expect(api.requestSubmission).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api.requestSubmission).mock.calls[1]?.[3]).toBe(originalKey);
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
  });

  it("invalidates an old mailbox autosave before composing from a new mailbox", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const staleSave = deferred<DraftResult>();
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [
        ...mailboxDirectory.items,
        {
          mailbox_id: "019b0000-0000-7000-8000-000000000021",
          kind: "shared",
          primary_address: "support@example.com",
          can_send: true,
        },
      ],
      next_cursor: null,
      has_more: false,
    });
    vi.mocked(api.saveDraft)
      .mockReturnValueOnce(staleSave.promise)
      .mockResolvedValueOnce({ ...draftResult, draft_id: "019b0000-0000-7000-8000-000000000032" });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "old@example.com" },
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(1200); });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.change(container.querySelector<HTMLSelectElement>(".sidebar .mailbox-picker select")!, {
      target: { value: "019b0000-0000-7000-8000-000000000021" },
    });
    await waitFor(() => expect(vi.mocked(api.listInbox).mock.calls.at(-1)?.[0].mailboxId)
      .toBe("019b0000-0000-7000-8000-000000000021"));
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    expect(screen.getByLabelText("From")).toHaveValue("support@example.com");
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "new@example.com" },
    });

    await act(async () => {
      staleSave.resolve(draftResult);
      await flushMicrotasks();
    });
    await waitFor(() => expect(api.saveDraft).toHaveBeenCalledTimes(2), { timeout: 2500 });
    expect(vi.mocked(api.saveDraft).mock.calls[1]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000021",
    );
    expect(vi.mocked(api.saveDraft).mock.calls[1]?.[1]).toMatchObject({
      draft_id: null,
      expected_version: null,
      to: ["new@example.com"],
    });
  });

  it("keeps an in-flight submission session pinned until its exact response settles", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const sharedMailboxId = "019b0000-0000-7000-8000-000000000021";
    const staleSubmission = deferred<MailSubmission>();
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [
        ...mailboxDirectory.items,
        {
          mailbox_id: sharedMailboxId,
          kind: "shared",
          primary_address: "support@example.com",
          can_send: true,
        },
      ],
      next_cursor: null,
      has_more: false,
    });
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockReturnValueOnce(staleSubmission.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    fireEvent.change(screen.getByLabelText("To"), { target: { value: "old@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(api.requestSubmission).toHaveBeenCalledTimes(1));

    const mailboxPicker = container.querySelector<HTMLSelectElement>(
      ".sidebar .mailbox-picker select",
    )!;
    expect(mailboxPicker).toBeDisabled();
    fireEvent.change(mailboxPicker, { target: { value: sharedMailboxId } });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    expect(screen.getByLabelText("From")).toHaveValue("reader@example.com");
    expect(screen.getByLabelText("To")).toHaveValue("old@example.com");
    expect(screen.getByLabelText("To")).toBeDisabled();
    await act(async () => {
      staleSubmission.resolve(deliveredSubmission);
      await flushMicrotasks();
    });
    expect(await screen.findByText("Delivered to all 1 recipient servers"))
      .toBeInTheDocument();
    expect(api.requestSubmission).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.requestSubmission).mock.calls[0]?.[0].mailboxId).toBe(
      mailboxDirectory.items[0].mailbox_id,
    );
    expect(api.getSubmission).not.toHaveBeenCalled();
  });

  it("does not let a delivered message timer close a newer composer session", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockResolvedValue(deliveredSubmission);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    vi.useFakeTimers();
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await act(async () => {
      await flushMicrotasks();
    });
    expect(screen.getByText("Delivered to all 1 recipient servers")).toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose")!);
    expect(screen.getByRole("dialog", { name: "New message" })).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(900);
    });
    expect(screen.getByRole("dialog", { name: "New message" })).toBeInTheDocument();
  });

  it("terminates a failed mobile detail load and retries from the visible pane", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    setNarrowViewport(true);
    vi.mocked(api.getMessage)
      .mockRejectedValueOnce(new Error("detail unavailable"))
      .mockResolvedValueOnce(detail);
    const { container } = render(<App />);
    const messageButton = await screen.findByRole("button", { name: /Alice/ });

    fireEvent.click(messageButton);
    const detailPane = container.querySelector<HTMLElement>(".detail")!;
    const alert = await within(detailPane).findByRole("alert");
    expect(alert).toHaveTextContent("This message is temporarily unavailable");
    expect(within(detailPane).queryByText("Loading?")).not.toBeInTheDocument();

    fireEvent.click(within(detailPane).getByRole("button", { name: "Retry" }));
    expect(await within(detailPane).findByText("The safe plain-text message body."))
      .toBeInTheDocument();
    expect(api.getMessage).toHaveBeenCalledTimes(2);
  });

  it("shows mobile read and star failures in the visible detail pane", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    setNarrowViewport(true);
    vi.mocked(api.listInbox).mockResolvedValue({
      ...page,
      items: [{ ...message, is_read: false }],
    });
    vi.mocked(api.updateMessageState).mockRejectedValue(new Error("state unavailable"));
    const { container } = render(<App />);
    const messageButton = await screen.findByRole("button", { name: /Alice/ });

    fireEvent.click(messageButton);
    const detailPane = container.querySelector<HTMLElement>(".detail")!;
    expect(await within(detailPane).findByRole("alert"))
      .toHaveTextContent("Unable to update the read state");
    await within(detailPane).findByText("The safe plain-text message body.");

    fireEvent.click(within(detailPane).getByRole("button", { name: "Add star" }));
    await waitFor(() => expect(within(detailPane).getByRole("alert"))
      .toHaveTextContent("Unable to update the star"));
  });

  it("does not let late read and flag responses roll back each other", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.listInbox).mockResolvedValue({
      ...page,
      items: [{ ...message, is_read: false }],
    });
    const readUpdate = deferred<MessageState>();
    const flagUpdate = deferred<MessageState>();
    vi.mocked(api.updateMessageState)
      .mockReturnValueOnce(readUpdate.promise)
      .mockReturnValueOnce(flagUpdate.promise);
    const { container } = render(<App />);
    const messageButton = await screen.findByRole("button", { name: /Alice/ });

    fireEvent.click(messageButton);
    await screen.findByText("The safe plain-text message body.");
    await waitFor(() => expect(api.updateMessageState).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Add star" }));
    expect(api.updateMessageState).toHaveBeenCalledTimes(2);

    await act(async () => {
      flagUpdate.resolve({
        message_id: message.message_id,
        is_read: false,
        is_flagged: true,
      });
      await flushMicrotasks();
    });
    expect(screen.getByRole("button", { name: "Remove star" })).toBeInTheDocument();

    await act(async () => {
      readUpdate.resolve({
        message_id: message.message_id,
        is_read: true,
        is_flagged: false,
      });
      await flushMicrotasks();
    });
    expect(screen.getByRole("button", { name: "Remove star" })).toBeInTheDocument();
    expect(container.querySelector(".message")).not.toHaveClass("unread");
  });

  it("does not let the initial inbox response overwrite a newer search", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const initial = deferred<InboxPage>();
    const searchResult = inboxPage(
      "Search result wins",
      "019b0000-0000-7000-8000-000000000101",
    );
    vi.mocked(api.listInbox).mockReturnValueOnce(initial.promise);
    vi.mocked(api.searchInbox).mockResolvedValueOnce(searchResult);
    render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByRole("textbox", { name: "Search email" }), {
      target: { value: "search term" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("Search result wins")).toBeInTheDocument();

    await act(async () => {
      initial.resolve(page);
      await flushMicrotasks();
    });
    expect(screen.getByText("Search result wins")).toBeInTheDocument();
    expect(screen.queryByText("Quarterly review")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Search results" })).toBeInTheDocument();
  });

  it("does not let the initial inbox response overwrite a newer refresh", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const initial = deferred<InboxPage>();
    const refreshed = inboxPage(
      "Refresh result wins",
      "019b0000-0000-7000-8000-000000000102",
    );
    vi.mocked(api.listInbox)
      .mockReturnValueOnce(initial.promise)
      .mockResolvedValueOnce(refreshed);
    render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Refresh result wins")).toBeInTheDocument();

    await act(async () => {
      initial.reject(new Error("stale initial inbox failure"));
      await flushMicrotasks();
    });
    expect(screen.getByText("Refresh result wins")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("keeps a clear-search request authoritative over the late initial inbox", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const initial = deferred<InboxPage>();
    const cleared = deferred<InboxPage>();
    const searchResult = inboxPage(
      "Search before clear",
      "019b0000-0000-7000-8000-000000000103",
    );
    const clearedResult = inboxPage(
      "Clear result wins",
      "019b0000-0000-7000-8000-000000000104",
    );
    vi.mocked(api.listInbox)
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(cleared.promise);
    vi.mocked(api.searchInbox).mockResolvedValueOnce(searchResult);
    render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByRole("textbox", { name: "Search email" }), {
      target: { value: "search term" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("Search before clear")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));

    await act(async () => {
      initial.resolve(page);
      await flushMicrotasks();
    });
    expect(screen.getByText("Search before clear")).toBeInTheDocument();
    expect(screen.queryByText("Quarterly review")).not.toBeInTheDocument();

    await act(async () => {
      cleared.resolve(clearedResult);
      await flushMicrotasks();
    });
    expect(await screen.findByText("Clear result wins")).toBeInTheDocument();
    expect(screen.queryByText("Search before clear")).not.toBeInTheDocument();
  });

  it("keeps load-more aggregation authoritative over the late initial inbox", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const initial = deferred<InboxPage>();
    const more = deferred<InboxPage>();
    const firstSearchPage = inboxPage(
      "First search page",
      "019b0000-0000-7000-8000-000000000105",
      "next-page-token",
    );
    const secondSearchPage = inboxPage(
      "Second search page",
      "019b0000-0000-7000-8000-000000000106",
    );
    vi.mocked(api.listInbox).mockReturnValueOnce(initial.promise);
    vi.mocked(api.searchInbox)
      .mockResolvedValueOnce(firstSearchPage)
      .mockReturnValueOnce(more.promise);
    render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByRole("textbox", { name: "Search email" }), {
      target: { value: "search term" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("First search page")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));

    await act(async () => {
      initial.resolve(page);
      await flushMicrotasks();
    });
    expect(screen.getByText("First search page")).toBeInTheDocument();
    expect(screen.queryByText("Quarterly review")).not.toBeInTheDocument();

    await act(async () => {
      more.resolve(secondSearchPage);
      await flushMicrotasks();
    });
    expect(await screen.findByText("Second search page")).toBeInTheDocument();
    expect(screen.getByText("First search page")).toBeInTheDocument();
  });

  it("routes every inbox request to the selected mailbox and rejects a late old-mailbox page", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const oldMailboxPage = deferred<InboxPage>();
    const sharedPage = inboxPage(
      "Shared mailbox message",
      "019b0000-0000-7000-8000-000000000201",
    );
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [
        ...mailboxDirectory.items,
        {
          mailbox_id: "019b0000-0000-7000-8000-000000000021",
          kind: "shared",
          primary_address: "support@example.com",
          can_send: true,
        },
      ],
      next_cursor: null,
      has_more: false,
    });
    vi.mocked(api.listInbox)
      .mockReturnValueOnce(oldMailboxPage.promise)
      .mockResolvedValueOnce(sharedPage);

    render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getAllByRole("combobox", { name: "Switch mailbox" })[0], {
      target: { value: "019b0000-0000-7000-8000-000000000021" },
    });

    expect(await screen.findByText("Shared mailbox message")).toBeInTheDocument();
    expect(vi.mocked(api.listInbox).mock.calls[1]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000021",
    );
    await act(async () => {
      oldMailboxPage.resolve(page);
      await flushMicrotasks();
    });
    expect(screen.getByText("Shared mailbox message")).toBeInTheDocument();
    expect(screen.queryByText("Quarterly review")).not.toBeInTheDocument();
  });

  it("ignores an unknown runtime preference and safely prefers a Human mailbox", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    window.__COPILOTMAIL_MAIL_CONFIG__ = {
      apiBaseUrl: "",
      mailboxId: "019b0000-0000-7000-8000-000000000099",
    };
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [
        {
          mailbox_id: "019b0000-0000-7000-8000-000000000019",
          kind: "shared",
          primary_address: "shared@example.com",
          can_send: true,
        },
        ...mailboxDirectory.items,
      ],
      next_cursor: null,
      has_more: false,
    });

    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    expect(container.querySelector<HTMLSelectElement>(".sidebar .mailbox-picker select"))
      .toHaveValue("019b0000-0000-7000-8000-000000000020");
    expect(vi.mocked(api.listInbox).mock.calls[0]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000020",
    );
  });

  it("fails closed with localized guidance when the directory is empty", async () => {
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [],
      next_cursor: null,
      has_more: false,
    });
    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "你的账号当前没有可访问的邮箱，请联系管理员",
    );
    expect(api.listInbox).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "退出登录" })).toBeInTheDocument();
  });

  it("resolves a later-page runtime preference before issuing any inbox request", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    window.__COPILOTMAIL_MAIL_CONFIG__ = {
      apiBaseUrl: "",
      mailboxId: "019b0000-0000-7000-8000-000000000022",
    };
    const cursor = "v1.019b0000-0000-7000-8000-000000000020";
    const laterDirectoryPage = deferred<MailboxDirectoryPage>();
    vi.mocked(api.listMailboxes)
      .mockResolvedValueOnce({
        ...mailboxDirectory,
        next_cursor: cursor,
        has_more: true,
      })
      .mockReturnValueOnce(laterDirectoryPage.promise);

    const { container } = render(<App />);
    await waitFor(() => expect(api.listMailboxes).toHaveBeenLastCalledWith(
      expect.anything(),
      cursor,
    ));
    expect(api.listInbox).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Inbox" })).not.toBeInTheDocument();

    await act(async () => {
      laterDirectoryPage.resolve({
        items: [{
          mailbox_id: "019b0000-0000-7000-8000-000000000022",
          kind: "shared",
          primary_address: "preferred@example.com",
          can_send: true,
        }],
        next_cursor: null,
        has_more: false,
      });
      await flushMicrotasks();
    });

    await screen.findByRole("heading", { name: "Inbox" });
    expect(container.querySelector<HTMLSelectElement>(".sidebar .mailbox-picker select")).toHaveValue(
      "019b0000-0000-7000-8000-000000000022",
    );
    expect(api.listInbox).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.listInbox).mock.calls[0]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000022",
    );
  });

  it("exhausts an absent preference before falling back to the Human mailbox", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    window.__COPILOTMAIL_MAIL_CONFIG__ = {
      apiBaseUrl: "",
      mailboxId: "019b0000-0000-7000-8000-000000000099",
    };
    const cursor = "v1.019b0000-0000-7000-8000-000000000020";
    const finalDirectoryPage = deferred<MailboxDirectoryPage>();
    vi.mocked(api.listMailboxes)
      .mockResolvedValueOnce({
        ...mailboxDirectory,
        next_cursor: cursor,
        has_more: true,
      })
      .mockReturnValueOnce(finalDirectoryPage.promise);

    render(<App />);
    await waitFor(() => expect(api.listMailboxes).toHaveBeenCalledTimes(2));
    expect(api.listInbox).not.toHaveBeenCalled();
    await act(async () => {
      finalDirectoryPage.resolve({
        items: [{
          mailbox_id: "019b0000-0000-7000-8000-000000000022",
          kind: "shared",
          primary_address: "other@example.com",
          can_send: true,
        }],
        next_cursor: null,
        has_more: false,
      });
      await flushMicrotasks();
    });

    await screen.findByRole("heading", { name: "Inbox" });
    expect(api.listInbox).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.listInbox).mock.calls[0]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000020",
    );
  });

  it("bounds initial preference discovery to ten pages and leaves explicit pagination", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    window.__COPILOTMAIL_MAIL_CONFIG__ = {
      apiBaseUrl: "",
      mailboxId: "019b0000-0000-7000-8000-000000000999",
    };
    const directoryPages: MailboxDirectoryPage[] = Array.from({ length: 10 }, (_, index) => {
      const suffix = String(index + 20).padStart(12, "0");
      const mailboxId = `019b0000-0000-7000-8000-${suffix}`;
      return {
        items: [{
          mailbox_id: mailboxId,
          kind: index === 0 ? "human" : "shared",
          primary_address: `mailbox-${index}@example.com`,
          can_send: true,
        }],
        next_cursor: `v1.${mailboxId}`,
        has_more: true,
      };
    });
    vi.mocked(api.listMailboxes).mockReset();
    for (const directoryPage of directoryPages) {
      vi.mocked(api.listMailboxes).mockResolvedValueOnce(directoryPage);
    }

    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });

    expect(api.listMailboxes).toHaveBeenCalledTimes(10);
    expect(api.listInbox).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.listInbox).mock.calls[0]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000020",
    );
    expect(container.querySelector<HTMLButtonElement>(".sidebar .mailbox-load-more"))
      .toHaveAccessibleName("Load more mailboxes");
  });

  it("keeps a current draft and mailbox when manual retry later reveals the preference", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    window.__COPILOTMAIL_MAIL_CONFIG__ = {
      apiBaseUrl: "",
      mailboxId: "019b0000-0000-7000-8000-000000000022",
    };
    const cursor = "v1.019b0000-0000-7000-8000-000000000020";
    const laterDirectoryPage = deferred<MailboxDirectoryPage>();
    vi.mocked(api.listMailboxes)
      .mockResolvedValueOnce({
        ...mailboxDirectory,
        next_cursor: cursor,
        has_more: true,
      })
      .mockRejectedValueOnce(new Error("temporary directory page failure"))
      .mockReturnValueOnce(laterDirectoryPage.promise);

    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    expect(screen.getAllByRole("alert")[0]).toHaveTextContent(
      "Unable to load more mailboxes; try again",
    );
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .mailbox-load-more")!);
    await waitFor(() => expect(api.listMailboxes).toHaveBeenCalledTimes(3));
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .compose")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Keep this unsaved draft" },
    });

    await act(async () => {
      laterDirectoryPage.resolve({
        items: [{
          mailbox_id: "019b0000-0000-7000-8000-000000000022",
          kind: "shared",
          primary_address: "preferred@example.com",
          can_send: true,
        }],
        next_cursor: null,
        has_more: false,
      });
      await flushMicrotasks();
    });

    expect(screen.getByRole("dialog", { name: "New message" })).toBeInTheDocument();
    expect(screen.getByLabelText("Message body")).toHaveValue("Keep this unsaved draft");
    expect(container.querySelector<HTMLSelectElement>(".sidebar .mailbox-picker select")).toHaveValue(
      "019b0000-0000-7000-8000-000000000020",
    );
    expect(screen.getByLabelText("From")).toHaveValue("reader@example.com");
  });

  it("disables compose and reply affordances for a read-only shared mailbox", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [{
        mailbox_id: "019b0000-0000-7000-8000-000000000021",
        kind: "shared",
        primary_address: "readonly@example.com",
        can_send: false,
      }],
      next_cursor: null,
      has_more: false,
    });
    window.__COPILOTMAIL_MAIL_CONFIG__ = { apiBaseUrl: "" };
    const { container } = render(<App />);

    await screen.findByRole("heading", { name: "Inbox" });
    expect(container.querySelector<HTMLButtonElement>(".sidebar .compose")).toBeDisabled();
    expect(container.querySelector<HTMLButtonElement>(".mobile-compose")).toBeDisabled();
    const draftEntries = screen.getAllByRole("button", { name: "Drafts" });
    expect(draftEntries).toHaveLength(2);
    draftEntries.forEach((entry) => expect(entry).toBeDisabled());
    expect(api.listDrafts).not.toHaveBeenCalled();
    expect(screen.getAllByText("This mailbox is read-only")).toHaveLength(2);
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");
    expect(screen.getByRole("button", { name: "Reply" })).toBeDisabled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("maps session failures to localized text without exposing machine errors", async () => {
    vi.mocked(auth.loadSession).mockRejectedValueOnce(
      new Error("secret_provider_session_failure"),
    );
    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "暂时无法验证企业会话，请重试",
    );
    expect(screen.queryByText(/secret_provider_session_failure/)).not.toBeInTheDocument();
  });
});
