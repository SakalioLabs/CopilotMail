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
  HumanSentSubmissionPage,
  InboxPage,
  MailboxDirectoryPage,
  MailSubmission,
  MessageDetail,
  MessageState,
  MessageSummary,
} from "./types";

vi.mock("./api", async (importOriginal) => ({
  ...await importOriginal<typeof import("./api")>(),
  getMessage: vi.fn(),
  getSubmission: vi.fn(),
  listDrafts: vi.fn(),
  listInbox: vi.fn(),
  listMailboxes: vi.fn(),
  listSentSubmissions: vi.fn(),
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
  expires_at: "2099-07-27T13:00:00.000Z",
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

const sentSubmissionId = "019b0000-0000-7000-8000-000000000080";
const olderSentSubmissionId = "019b0000-0000-7000-8000-000000000079";
const sentPage: HumanSentSubmissionPage = {
  items: [{
    submission_id: sentSubmissionId,
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
  }],
  next_cursor: null,
  has_more: false,
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

function composerIdentity(dialog: HTMLElement) {
  return within(dialog).getByRole("region", {
    name: /写信上下文|Compose context/,
  });
}

function switchMailboxViaMenu(sidebar: HTMLElement, address: string) {
  fireEvent.click(sidebar.querySelector<HTMLButtonElement>(".identity-card")!);
  fireEvent.click(within(sidebar).getByRole("menuitemradio", { name: address }));
}

function setNarrowViewport(narrow: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: narrow && query === "(max-width: 1007px)",
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
  window.history.replaceState(null, "", "/");
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
  vi.mocked(api.listSentSubmissions).mockResolvedValue(sentPage);
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
  window.history.replaceState(null, "", "/");
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
    fireEvent.click(container.querySelector<HTMLButtonElement>(".mail-topbar__menu")!);
    const mobileDrafts = within(await screen.findByRole("dialog", { name: "Mail navigation" }))
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

  it("exposes Sent navigation and reads only accepted delivery metadata", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    setNarrowViewport(true);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const desktopSent = within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Sent" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".mail-topbar__menu")!);
    const mobileSent = within(await screen.findByRole("dialog", { name: "Mail navigation" }))
      .getByRole("button", { name: "Sent" });
    expect(desktopSent).toBeEnabled();
    expect(mobileSent).toBeEnabled();

    fireEvent.click(mobileSent);

    const heading = await screen.findByRole("heading", { name: "Sent" });
    await waitFor(() => expect(heading).toHaveFocus());
    expect(await screen.findByText(
      "Delivery update: 1/2 delivered, 0 failed, 1 queued, 0 unknown",
    )).toBeInTheDocument();
    expect(api.listSentSubmissions).toHaveBeenCalledWith(
      expect.objectContaining({ mailboxId: mailboxDirectory.items[0].mailbox_id }),
      undefined,
      expect.any(AbortSignal),
    );
    expect(screen.queryByText("Restored private subject")).not.toBeInTheDocument();
    expect(screen.queryByText("250 delivered")).not.toBeInTheDocument();
  });

  it("refreshes and appends only a progressing sent page", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const firstPage: HumanSentSubmissionPage = {
      ...sentPage,
      next_cursor: `v1.${sentSubmissionId}`,
      has_more: true,
    };
    const nextPage: HumanSentSubmissionPage = {
      items: [{
        ...sentPage.items[0],
        submission_id: olderSentSubmissionId,
        recipient_count: 1,
        delivery: undefined,
      }],
      next_cursor: null,
      has_more: false,
    };
    vi.mocked(api.listSentSubmissions)
      .mockReset()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(nextPage);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Sent" }));

    const refresh = await screen.findByRole("button", { name: "Refresh sent mail" });
    fireEvent.click(refresh);
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(2));
    const loadMore = await screen.findByRole("button", { name: "Load more sent mail" });
    fireEvent.click(loadMore);

    await waitFor(() => expect(screen.getAllByText("Accepted for delivery")).toHaveLength(2));
    expect(api.listSentSubmissions).toHaveBeenLastCalledWith(
      expect.objectContaining({ mailboxId: mailboxDirectory.items[0].mailbox_id }),
      `v1.${sentSubmissionId}`,
      expect.any(AbortSignal),
    );
  });

  it("aborts and ignores sent directory responses after leaving the view", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const latePage = deferred<HumanSentSubmissionPage>();
    vi.mocked(api.listSentSubmissions).mockReset().mockReturnValueOnce(latePage.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const sidebar = container.querySelector<HTMLElement>(".sidebar")!;
    fireEvent.click(within(sidebar).getByRole("button", { name: "Sent" }));
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(api.listSentSubmissions).mock.calls[0]?.[2];
    fireEvent.click(within(sidebar).getByRole("button", { name: "Inbox" }));
    expect(signal?.aborted).toBe(true);

    await act(async () => {
      latePage.resolve(sentPage);
      await flushMicrotasks();
    });
    expect(await screen.findByRole("heading", { name: "Inbox" })).toBeInTheDocument();
    expect(screen.queryByText("Delivery update: 1/2 delivered")).not.toBeInTheDocument();
  });

  it("deactivates the sent directory synchronously before changing mailbox scope", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const secondaryMailbox = {
      mailbox_id: "019b0000-0000-7000-8000-000000000021",
      kind: "agent" as const,
      primary_address: "triage-agent@example.com",
      can_send: true,
    };
    const latePage = deferred<HumanSentSubmissionPage>();
    vi.mocked(api.listMailboxes).mockResolvedValue({
      ...mailboxDirectory,
      items: [...mailboxDirectory.items, secondaryMailbox],
    });
    vi.mocked(api.listSentSubmissions).mockReset().mockReturnValueOnce(latePage.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const sidebar = container.querySelector<HTMLElement>(".sidebar")!;
    fireEvent.click(within(sidebar).getByRole("button", { name: "Sent" }));
    await waitFor(() => expect(api.listSentSubmissions).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(api.listSentSubmissions).mock.calls[0]?.[2];

    switchMailboxViaMenu(sidebar, secondaryMailbox.primary_address);

    expect(signal?.aborted).toBe(true);
    await act(async () => {
      latePage.resolve(sentPage);
      await flushMicrotasks();
    });
    expect(await screen.findByRole("heading", { name: "Inbox" })).toBeInTheDocument();
    expect(screen.queryByText("Delivery update: 1/2 delivered")).not.toBeInTheDocument();
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
    fireEvent.click(within(container.querySelector<HTMLElement>(".empty--actionable")!)
      .getByRole("button", { name: "Refresh drafts" }));

    expect(await screen.findByRole("button", { name: /Resume editing/ }))
      .toBeInTheDocument();
    expect(api.listDrafts).toHaveBeenCalledTimes(2);
  });

  it("confirms before a mailbox change clears local writing content", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const secondMailbox = {
      mailbox_id: "019b0000-0000-7000-8000-000000000021",
      kind: "shared" as const,
      primary_address: "support@example.com",
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

    fireEvent.click(screen.getByRole("button", { name: "Compose" }));
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Local content that has no recipient and cannot autosave" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));
    switchMailboxViaMenu(sidebar, secondMailbox.primary_address);

    const warning = await screen.findByRole("alertdialog", {
      name: "Leave the current writing context?",
    });
    expect(within(warning).getByRole("button", { name: "Keep editing" })).toHaveFocus();
    fireEvent.click(within(warning).getByRole("button", { name: "Keep editing" }));
    expect(sidebar.querySelector(".identity-card__email"))
      .toHaveTextContent(mailboxDirectory.items[0].primary_address);
    const reopenedComposer = await screen.findByRole("dialog", { name: "New message" });
    expect(within(reopenedComposer).getByLabelText("Message body"))
      .toHaveValue("Local content that has no recipient and cannot autosave");
    await waitFor(() => expect(within(reopenedComposer).getByLabelText("To")).toHaveFocus());
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    switchMailboxViaMenu(sidebar, secondMailbox.primary_address);
    fireEvent.click(await screen.findByRole("button", { name: "Leave and continue" }));
    await waitFor(() => expect(sidebar.querySelector(".identity-card__email"))
      .toHaveTextContent(secondMailbox.primary_address));
    fireEvent.click(screen.getByRole("button", { name: "Compose" }));
    expect(screen.getByLabelText("Message body")).toHaveValue("");
  });

  it("confirms before Compose replaces a closed reply context", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");
    fireEvent.click(screen.getByRole("button", { name: "Reply" }));
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Reply context that must not disappear silently" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    const compose = container.querySelector<HTMLButtonElement>(".compose-fab")!;
    fireEvent.click(compose);
    const warning = await screen.findByRole("alertdialog", {
      name: "Leave the current writing context?",
    });
    fireEvent.click(within(warning).getByRole("button", { name: "Leave and continue" }));

    expect(await screen.findByRole("dialog", { name: "New message" })).toBeInTheDocument();
    expect(screen.getByLabelText("To")).toHaveValue("");
    expect(screen.getByLabelText("Message body")).toHaveValue("");
  });

  it("confirms sign-out while a local writing context is still present", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const sidebar = container.querySelector<HTMLElement>(".sidebar")!;
    fireEvent.click(screen.getByRole("button", { name: "Compose" }));
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Do not lose this on an accidental sign-out" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    const identityCard = container.querySelector<HTMLElement>(".identity-card")!;
    const requestSignOut = () => {
      fireEvent.click(identityCard);
      fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    };
    requestSignOut();
    const warning = await screen.findByRole("alertdialog", {
      name: "Leave the current writing context?",
    });
    fireEvent.click(within(warning).getByRole("button", { name: "Keep editing" }));
    expect(auth.logout).not.toHaveBeenCalled();
    const reopenedComposer = await screen.findByRole("dialog", { name: "New message" });
    expect(within(reopenedComposer).getByLabelText("Message body"))
      .toHaveValue("Do not lose this on an accidental sign-out");
    await waitFor(() => expect(within(reopenedComposer).getByLabelText("To")).toHaveFocus());
    fireEvent.click(within(reopenedComposer).getByRole("button", { name: "Close composer" }));

    requestSignOut();
    fireEvent.click(await screen.findByRole("button", { name: "Leave and continue" }));
    await waitFor(() => expect(auth.logout).toHaveBeenCalledTimes(1));
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
    expect(composerIdentity(composer)).toHaveTextContent("reader@example.com");
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
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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
    switchMailboxViaMenu(sidebar, secondMailbox.primary_address);
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
      expect(window.location.hash).toBe(`#/message/${message.message_id}`);
      expect(screen.getByRole("heading", { name: "Quarterly review" }))
        .toHaveFocus();
    });
    expect(await screen.findByText("The safe plain-text message body."))
      .toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Alice/ })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /返回收件箱/ }));

    await waitFor(() => {
      expect(window.location.hash).toBe("#/");
      expect(screen.getByRole("button", { name: /Alice/ })).toHaveFocus();
    });
  });

  it("keeps compose and the account menu reachable in the mobile drawer", async () => {
    setNarrowViewport(true);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "收件箱" });
    const composeFab = container.querySelector<HTMLButtonElement>(".compose-fab");

    expect(composeFab).toHaveAccessibleName("写信");

    fireEvent.click(container.querySelector<HTMLButtonElement>(".mail-topbar__menu")!);
    const drawer = await screen.findByRole("dialog", { name: "邮件导航" });

    const identityCard = within(drawer).getByRole("button", { name: /reader@example\.com/ });
    expect(identityCard).toHaveClass("identity-card");
    fireEvent.click(identityCard);
    const accountMenu = within(drawer).getByRole("menu", { name: "账户" });
    expect(within(accountMenu).getByRole("menuitem", { name: "设置" })).toBeEnabled();
    expect(within(accountMenu).getByRole("menuitem", { name: "退出登录" })).toBeEnabled();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    composeFab?.focus();
    fireEvent.click(composeFab!);
    expect(await screen.findByRole("dialog", { name: "新邮件" })).toBeInTheDocument();
    expect(container.querySelector(".mail-shell")).toHaveAttribute("inert");

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(composeFab).toHaveFocus();
  });

  it("switches every visible label and the document language without a reload", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "收件箱" });
    expect(document.documentElement.lang).toBe("zh-CN");

    const identityCard = screen.getByRole("button", { name: /reader@example\.com/ });
    fireEvent.click(identityCard);
    fireEvent.click(screen.getByRole("menuitem", { name: "设置" }));
    const settings = await screen.findByRole("dialog", { name: "设置" });
    fireEvent.change(within(settings).getByRole("combobox", { name: "语言" }), {
      target: { value: "en" },
    });

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(identityCard).toHaveFocus();

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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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
    fireEvent.click(await screen.findByRole("button", { name: "Leave and continue" }));
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(api.requestSubmission).toHaveBeenCalledTimes(1));
    await act(async () => {
      unknownSubmission.reject(new Error("connection reset after request write"));
      await flushMicrotasks();
    });

    const unknownStatus = screen.getByText(
      "The send request status is unknown; confirm the same submission again—no duplicate email will be created",
    );
    expect(unknownStatus.closest(".status-indicator")).toHaveClass(
      "status-indicator--warning",
    );
    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(screen.getByLabelText("Message body")).toBeDisabled();
    expect(screen.getByRole("button", {
      name: "Confirm same submission (no duplicate email)",
    })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));
    fireEvent.click(within(container.querySelector<HTMLElement>(".sidebar")!)
      .getByRole("button", { name: "Drafts" }));
    fireEvent.click(await screen.findByRole("button", { name: /Resume editing/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The current send status is not known, so another draft cannot be opened",
    );
    expect(api.restoreDraft).not.toHaveBeenCalled();
  });

  it("reopens exact unknown-submission evidence from commands without enabling Reply", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.saveDraft).mockResolvedValue(draftResult);
    vi.mocked(api.requestSubmission).mockRejectedValueOnce(
      new Error("request may have reached the server"),
    );
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "frozen@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Subject"), {
      target: { value: "Frozen subject" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText(
      "The send request status is unknown; confirm the same submission again—no duplicate email will be created",
    );
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .command-trigger")!);
    const lockedCommandSearch = await screen.findByRole("combobox", { name: "Search commands" });
    const lockedCompose = screen.getByRole("option", { name: "Review pending send" });
    expect(lockedCompose).toBeEnabled();
    expect(lockedCompose).toHaveAccessibleDescription(
      "Reopen the frozen evidence for the previous email send",
    );
    fireEvent.change(lockedCommandSearch, { target: { value: "pending" } });
    fireEvent.keyDown(lockedCommandSearch, { key: "Enter" });
    expect(await screen.findByRole("dialog", { name: "New message" })).toBeInTheDocument();
    expect(screen.getByLabelText("To")).toHaveValue("frozen@example.com");
    expect(screen.getByLabelText("Subject")).toHaveValue("Frozen subject");
    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    const reply = screen.getByRole("button", { name: "Reply" });
    expect(reply).toBeDisabled();
    fireEvent.click(reply);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.keyDown(document, { key: "c" });
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
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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
      "The send request status is unknown; confirm the same submission again—no duplicate email will be created",
    )).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.click(screen.getByRole("button", {
      name: "Confirm same submission (no duplicate email)",
    }));
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "alice@example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText(
      "The send request status is unknown; confirm the same submission again—no duplicate email will be created",
    );
    expect(screen.getByLabelText("To")).toBeDisabled();
    const originalKey = vi.mocked(api.requestSubmission).mock.calls[0]?.[3];

    fireEvent.keyDown(document, { key: "Escape" });
    const identityCard = container.querySelector<HTMLButtonElement>(
      ".sidebar .identity-card",
    )!;
    expect(identityCard).toBeDisabled();
    fireEvent.click(identityCard);
    expect(container.querySelector(".identity-menu")).not.toBeInTheDocument();
    expect(vi.mocked(api.listInbox).mock.calls.at(-1)?.[0].mailboxId).toBe(
      mailboxDirectory.items[0].mailbox_id,
    );
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);

    expect(composerIdentity(screen.getByRole("dialog"))).toHaveTextContent("reader@example.com");
    expect(screen.getByLabelText("To")).toBeDisabled();
    expect(screen.getByLabelText("To")).toHaveValue("alice@example.com");
    expect(screen.getByLabelText("Message body")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", {
      name: "Confirm same submission (no duplicate email)",
    }));
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "old@example.com" },
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(1200); });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
    fireEvent.keyDown(document, { key: "Escape" });
    switchMailboxViaMenu(container.querySelector<HTMLElement>(".sidebar")!, "support@example.com");
    fireEvent.click(await screen.findByRole("button", { name: "Leave and continue" }));
    await waitFor(() => expect(vi.mocked(api.listInbox).mock.calls.at(-1)?.[0].mailboxId)
      .toBe("019b0000-0000-7000-8000-000000000021"));
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    expect(composerIdentity(screen.getByRole("dialog"))).toHaveTextContent("support@example.com");
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

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("To"), { target: { value: "old@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(api.requestSubmission).toHaveBeenCalledTimes(1));

    const identityCard = container.querySelector<HTMLButtonElement>(
      ".sidebar .identity-card",
    )!;
    expect(identityCard).toBeDisabled();
    fireEvent.click(identityCard);
    expect(container.querySelector(".identity-menu")).not.toBeInTheDocument();
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    expect(composerIdentity(screen.getByRole("dialog"))).toHaveTextContent("reader@example.com");
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
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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
    const detailPane = container.querySelector<HTMLElement>(".detail")!;
    await waitFor(() => expect(api.updateMessageState).toHaveBeenCalledTimes(1));
    fireEvent.click(within(detailPane).getByRole("button", { name: "Add star" }));
    expect(api.updateMessageState).toHaveBeenCalledTimes(2);

    await act(async () => {
      flagUpdate.resolve({
        message_id: message.message_id,
        is_read: false,
        is_flagged: true,
      });
      await flushMicrotasks();
    });
    expect(within(detailPane).getByRole("button", { name: "Remove star" })).toBeInTheDocument();

    await act(async () => {
      readUpdate.resolve({
        message_id: message.message_id,
        is_read: true,
        is_flagged: false,
      });
      await flushMicrotasks();
    });
    expect(within(detailPane).getByRole("button", { name: "Remove star" })).toBeInTheDocument();
    fireEvent.click(within(detailPane).getByRole("button", { name: "Back to inbox" }));
    const listRow = await screen.findByRole("button", { name: /Alice/ });
    await waitFor(() => expect(listRow).not.toHaveClass("unread"));
    expect(within(listRow).getByText("Starred")).toBeInTheDocument();
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

  it("rebinds an opened detail to the latest summary for the same message identity", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const updatedMessage: MessageSummary = {
      ...message,
      subject: "Quarterly review — updated",
      preview: "Fresh server summary",
      is_flagged: true,
    };
    vi.mocked(api.listInbox)
      .mockReset()
      .mockResolvedValueOnce(page)
      .mockResolvedValueOnce({ ...page, items: [updatedMessage] });
    const { container } = render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Quarterly review/ }));
    expect(await screen.findByText(detail.text_body)).toBeVisible();
    // The routed detail replaces the list surface, so the list refresh
    // control is unmounted; refresh the inbox through the command palette.
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const refreshCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(refreshCommand, { target: { value: "refresh current" } });
    fireEvent.keyDown(refreshCommand, { key: "Enter" });

    expect(await screen.findByRole("heading", {
      name: "Quarterly review — updated",
    })).toBeVisible();
    expect(within(container.querySelector<HTMLElement>(".detail")!)
      .getByRole("button", { name: "Remove star" })).toBeVisible();
    expect(api.getMessage).toHaveBeenCalledTimes(1);
  });

  it("closes a removed detail and moves the inbox cursor deterministically after refresh", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const survivingMessage: MessageSummary = {
      ...message,
      message_id: "019b0000-0000-7000-8000-000000000002",
      subject: "Roadmap after refresh",
      from: [{ name: "Bob", email: "bob@example.com" }],
    };
    vi.mocked(api.listInbox)
      .mockReset()
      .mockResolvedValueOnce({
        items: [{ ...message }, survivingMessage],
        next_cursor: null,
        has_more: false,
      })
      .mockResolvedValueOnce({
        items: [survivingMessage],
        next_cursor: null,
        has_more: false,
      });
    vi.mocked(api.getMessage).mockImplementation(async (_settings, messageId) => ({
      ...detail,
      message: messageId === survivingMessage.message_id ? survivingMessage : message,
      reply_to_message_id: messageId,
      text_body: messageId === survivingMessage.message_id
        ? "The surviving message body."
        : detail.text_body,
    }));
    const { container } = render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Quarterly review/ }));
    expect(await screen.findByText(detail.text_body)).toBeVisible();
    // The routed detail replaces the list surface, so refresh the inbox
    // through the command palette while the detail stays open.
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const refreshCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(refreshCommand, { target: { value: "refresh current" } });
    fireEvent.keyDown(refreshCommand, { key: "Enter" });

    const fallback = await screen.findByRole("button", { name: /Roadmap after refresh/ });
    await waitFor(() => expect(fallback).toHaveAttribute("aria-current", "true"));
    await waitFor(() => expect(fallback).toHaveFocus());
    expect(screen.queryByText(detail.text_body)).not.toBeInTheDocument();
    expect(window.location.hash).toBe("#/");

    fireEvent.keyDown(document, { key: "Enter" });
    expect(await screen.findByText("The surviving message body.")).toBeVisible();
    expect(api.getMessage).toHaveBeenLastCalledWith(
      expect.anything(),
      survivingMessage.message_id,
      expect.anything(),
    );
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

  it("preserves a confirmed star mutation while an inbox page is in flight", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const more = deferred<InboxPage>();
    const firstPage: InboxPage = {
      items: [{ ...message }],
      next_cursor: "next-page-token",
      has_more: true,
    };
    const olderMessage = {
      ...message,
      message_id: "019b0000-0000-7000-8000-000000000107",
      subject: "Older message remains appended",
    };
    vi.mocked(api.listInbox)
      .mockReset()
      .mockResolvedValueOnce(firstPage)
      .mockReturnValueOnce(more.promise);
    vi.mocked(api.updateMessageState).mockResolvedValue({
      message_id: message.message_id,
      is_read: true,
      is_flagged: true,
    });
    const { container } = render(<App />);

    const row = await screen.findByRole("button", { name: /Quarterly review/ });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    fireEvent.click(row);
    await screen.findByText("The safe plain-text message body.");
    fireEvent.click(within(container.querySelector<HTMLElement>(".detail")!)
      .getByRole("button", { name: "Add star" }));
    await waitFor(() => expect(api.updateMessageState).toHaveBeenCalledWith(
      expect.anything(),
      message.message_id,
      { is_flagged: true },
    ));
    fireEvent.click(screen.getByRole("button", { name: "Back to inbox" }));

    await act(async () => {
      more.resolve({ items: [olderMessage], next_cursor: null, has_more: false });
      await flushMicrotasks();
    });

    expect(await screen.findByText("Older message remains appended")).toBeInTheDocument();
    const listRow = await screen.findByRole("button", { name: /Quarterly review/ });
    expect(within(listRow).getByText("Starred")).toBeInTheDocument();
  });

  it("fails closed without changing the inbox when a later page repeats an identity", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const firstPage: InboxPage = {
      items: [{ ...message }],
      next_cursor: "next-page-token",
      has_more: true,
    };
    vi.mocked(api.listInbox)
      .mockReset()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce({
        items: [{ ...message, subject: "Conflicting duplicate" }],
        next_cursor: null,
        has_more: false,
      });
    render(<App />);

    await screen.findByRole("button", { name: /Quarterly review/ });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Unable to load more inbox messages",
    );
    expect(screen.getByText("Quarterly review")).toBeInTheDocument();
    expect(screen.queryByText("Conflicting duplicate")).not.toBeInTheDocument();
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

    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(1));
    switchMailboxViaMenu(
      container.querySelector<HTMLElement>(".sidebar")!,
      "support@example.com",
    );

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

  it("uses the Agent mailbox identity exactly as returned by the directory when composing", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [{
        mailbox_id: "019b0000-0000-7000-8000-000000000024",
        kind: "agent",
        primary_address: "triage-agent@example.com",
        can_send: true,
      }],
      next_cursor: null,
      has_more: false,
    });

    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);

    const composer = await screen.findByRole("dialog", { name: "New message" });
    const identity = composerIdentity(composer);
    expect(within(identity).getByText("Agent")).toBeInTheDocument();
    expect(within(identity).getByText("triage-agent@example.com")).toBeInTheDocument();
    expect(within(identity).getByText(
      "Server status: this mailbox can send email.",
    )).toBeInTheDocument();
    expect(identity).not.toHaveTextContent(/owner|delegat|on behalf|actor/i);
    expect(identity.querySelector("[data-owner], [data-scope], [data-delegation]")).toBeNull();
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

    expect(container.querySelector(".sidebar .identity-card__email"))
      .toHaveTextContent("reader@example.com");
    await waitFor(() => {
      expect(vi.mocked(api.listInbox).mock.calls[0]?.[0].mailboxId).toBe(
        "019b0000-0000-7000-8000-000000000020",
      );
    });
  });

  it("fails closed with localized guidance when the directory is empty", async () => {
    vi.mocked(api.listMailboxes).mockResolvedValueOnce({
      items: [],
      next_cursor: null,
      has_more: false,
    });
    const { container } = render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "你的账号当前没有可访问的邮箱，请联系管理员",
    );
    const accessSurface = container.querySelector<HTMLElement>("[data-mail-access='directory']");
    expect(accessSurface).toBeInTheDocument();
    expect(accessSurface?.querySelector(".mail-access__visual")).toBeNull();
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
    expect(container.querySelector(".sidebar .identity-card__email"))
      .toHaveTextContent("preferred@example.com");
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

    await waitFor(() => {
      expect(api.listMailboxes).toHaveBeenCalledTimes(10);
      expect(api.listInbox).toHaveBeenCalledTimes(1);
    });
    expect(vi.mocked(api.listInbox).mock.calls[0]?.[0].mailboxId).toBe(
      "019b0000-0000-7000-8000-000000000020",
    );
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .identity-card")!);
    expect(screen.getByRole("menuitem", { name: "Load more mailboxes" }))
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
    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .identity-card")!);
    expect(screen.getAllByRole("alert")[0]).toHaveTextContent(
      "Unable to load more mailboxes; try again",
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Load more mailboxes" }));
    await waitFor(() => expect(api.listMailboxes).toHaveBeenCalledTimes(3));
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
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
    expect(container.querySelector(".sidebar .identity-card__email"))
      .toHaveTextContent("reader@example.com");
    expect(composerIdentity(screen.getByRole("dialog"))).toHaveTextContent("reader@example.com");
  });

  it("wires keyboard-first inbox navigation without overriding editable fields or dialogs", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const secondMessage: MessageSummary = {
      ...message,
      message_id: "019b0000-0000-7000-8000-000000000002",
      subject: "Roadmap review",
      from: [{ name: "Bob", email: "bob@example.com" }],
      is_read: false,
    };
    const unreadFirstMessage = { ...message, is_read: false };
    vi.mocked(api.listInbox).mockResolvedValueOnce({
      ...page,
      items: [unreadFirstMessage, secondMessage],
    });
    vi.mocked(api.getMessage).mockImplementation(async (_settings, messageId) => ({
      ...detail,
      message: messageId === secondMessage.message_id ? secondMessage : unreadFirstMessage,
      reply_to_message_id: messageId,
    }));
    const { container } = render(<App />);

    await screen.findByRole("heading", { name: "Inbox" });
    const search = screen.getByRole("textbox", { name: "Search email" });
    fireEvent.keyDown(document, { key: "/" });
    await waitFor(() => expect(search).toHaveFocus());

    fireEvent.keyDown(search, { key: "c" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    search.blur();

    fireEvent.keyDown(document, { key: "j" });
    await waitFor(() => {
      expect(container.querySelectorAll<HTMLButtonElement>(".message")[0])
        .toHaveAttribute("aria-current", "true");
    });
    fireEvent.keyDown(document, { key: "j" });
    await waitFor(() => {
      expect(container.querySelectorAll<HTMLButtonElement>(".message")[1])
        .toHaveAttribute("aria-current", "true");
    });
    fireEvent.keyDown(document, { key: "k" });
    await waitFor(() => {
      expect(container.querySelectorAll<HTMLButtonElement>(".message")[0])
        .toHaveAttribute("aria-current", "true");
    });
    expect(api.getMessage).not.toHaveBeenCalled();
    expect(api.updateMessageState).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: "Enter" });
    await screen.findByText("The safe plain-text message body.");
    expect(api.getMessage).toHaveBeenCalledWith(
      expect.anything(),
      message.message_id,
      expect.anything(),
    );
    await waitFor(() => expect(api.updateMessageState).toHaveBeenCalledWith(
      expect.anything(),
      message.message_id,
      { is_read: true },
    ));

    fireEvent.keyDown(document, { key: "r" });
    expect(await screen.findByRole("dialog", { name: "Reply" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "s" });
    expect(vi.mocked(api.updateMessageState).mock.calls.some(([, , update]) => (
      update.is_flagged !== undefined
    ))).toBe(false);
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    fireEvent.keyDown(document, { key: "s" });
    await waitFor(() => expect(api.updateMessageState).toHaveBeenCalledWith(
      expect.anything(),
      message.message_id,
      { is_flagged: true },
    ));
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(container.querySelectorAll<HTMLButtonElement>(".message")[0])
        .toHaveAttribute("aria-current", "true");
      expect(container.querySelectorAll<HTMLButtonElement>(".message")[0]).toHaveFocus();
    });
    fireEvent.keyDown(document, { key: "Enter" });
    await screen.findByText("The safe plain-text message body.");

    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "d" });
    await screen.findByRole("heading", { name: "Drafts" });
    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "s" });
    await screen.findByRole("heading", { name: "Sent" });
    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "i" });
    await screen.findByRole("heading", { name: "Inbox" });
  });

  it("executes localized Compose, navigation, search, and refresh commands through Ctrl K", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const trigger = container.querySelector<HTMLButtonElement>(".sidebar .command-trigger")!;

    trigger.focus();
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const commandSearch = await screen.findByRole("combobox", { name: "Search commands" });
    const background = container.querySelector<HTMLElement>(".mail-shell");
    expect(background).toHaveAttribute("data-dialog-background");
    expect(background).toHaveAttribute("aria-hidden", "true");
    expect(background).toHaveAttribute("inert", "");
    expect(background?.contains(screen.getByRole("dialog", { name: "Command palette" })))
      .toBe(false);
    expect(screen.getByRole("option", { name: "Compose" })).toHaveTextContent("C");
    expect(screen.getByRole("option", { name: "Go to sent" })).toHaveTextContent("G S");
    fireEvent.keyDown(commandSearch, { key: "Escape" });
    await waitFor(() => {
      expect(trigger).toHaveFocus();
      expect(background).not.toHaveAttribute("aria-hidden");
      expect(background).not.toHaveAttribute("inert");
    });

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const composeCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(composeCommand, { target: { value: "compose" } });
    fireEvent.keyDown(composeCommand, { key: "Enter" });
    expect(await screen.findByRole("dialog", { name: "New message" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("To")).toHaveFocus());
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    fireEvent.keyDown(document, { key: "k", metaKey: true });
    const searchCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(searchCommand, { target: { value: "focus email search" } });
    fireEvent.keyDown(searchCommand, { key: "Enter" });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Search email" })).toHaveFocus());

    trigger.focus();
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const refreshCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(refreshCommand, { target: { value: "refresh current" } });
    fireEvent.keyDown(refreshCommand, { key: "Enter" });
    await waitFor(() => expect(api.listInbox).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(trigger).toHaveFocus());

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const sentCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(sentCommand, { target: { value: "go to sent" } });
    fireEvent.keyDown(sentCommand, { key: "Enter" });
    await screen.findByRole("heading", { name: "Sent" });

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const inboxCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(inboxCommand, { target: { value: "go to inbox" } });
    fireEvent.keyDown(inboxCommand, { key: "Enter" });
    await screen.findByRole("heading", { name: "Inbox" });
  });

  it("shows detail-only Reply and star commands only after a message is actually open", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    const trigger = container.querySelector<HTMLButtonElement>(".sidebar .command-trigger")!;
    fireEvent.click(trigger);
    const beforeDetailSearch = await screen.findByRole("combobox", { name: "Search commands" });
    expect(screen.queryByRole("option", { name: "Reply to this email" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Add star" })).not.toBeInTheDocument();
    fireEvent.keyDown(beforeDetailSearch, { key: "Escape" });
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");

    fireEvent.click(trigger);
    const commandSearch = await screen.findByRole("combobox", { name: "Search commands" });
    expect(screen.getByRole("option", { name: "Reply to this email" })).toHaveTextContent("R");
    expect(screen.getByRole("option", { name: "Add star" })).toHaveTextContent("S");
    fireEvent.change(commandSearch, { target: { value: "add star" } });
    fireEvent.keyDown(commandSearch, { key: "Enter" });
    await waitFor(() => expect(api.updateMessageState).toHaveBeenCalledWith(
      expect.anything(),
      message.message_id,
      { is_flagged: true },
    ));

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const replyCommand = await screen.findByRole("combobox", { name: "Search commands" });
    fireEvent.change(replyCommand, { target: { value: "reply to this" } });
    fireEvent.keyDown(replyCommand, { key: "Enter" });
    expect(await screen.findByRole("dialog", { name: "Reply" })).toBeInTheDocument();
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
    expect(container.querySelector<HTMLButtonElement>(".compose-fab")).toBeDisabled();
    const sidebarNav = container.querySelector<HTMLElement>(".sidebar")!;
    expect(within(sidebarNav).getByRole("button", { name: "Drafts" })).toBeDisabled();
    expect(within(sidebarNav).getByRole("button", { name: "Sent" })).toBeDisabled();
    fireEvent.click(container.querySelector<HTMLButtonElement>(".mail-topbar__menu")!);
    const drawer = await screen.findByRole("dialog", { name: "Mail navigation" });
    expect(within(drawer).getByRole("button", { name: "Drafts" })).toBeDisabled();
    expect(within(drawer).getByRole("button", { name: "Sent" })).toBeDisabled();
    expect(api.listDrafts).not.toHaveBeenCalled();
    expect(api.listSentSubmissions).not.toHaveBeenCalled();
    // The read-only boundary stays discoverable inside the drawer's account
    // menu instead of a permanent scope section.
    fireEvent.click(within(drawer).getByRole("button", { name: /reader@example\.com|@/ }));
    expect(within(drawer).getAllByText("This mailbox is read-only")).toHaveLength(1);
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(drawer).not.toBeInTheDocument());
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    await screen.findByText("The safe plain-text message body.");
    expect(screen.getByRole("button", { name: "Reply" })).toBeDisabled();
    fireEvent.keyDown(document, { key: "c" });
    fireEvent.keyDown(document, { key: "r" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(container.querySelector<HTMLButtonElement>(".sidebar .command-trigger")!);
    const commandSearch = await screen.findByRole("combobox", { name: "Search commands" });
    const commandCompose = screen.getByRole("option", { name: "Compose" });
    expect(commandCompose).toBeDisabled();
    expect(commandCompose).toHaveAccessibleDescription("This mailbox is read-only");
    expect(screen.getByRole("option", { name: "Go to drafts" })).toBeDisabled();
    expect(screen.getByRole("option", { name: "Go to sent" })).toBeDisabled();
    fireEvent.change(commandSearch, { target: { value: "compose" } });
    fireEvent.keyDown(commandSearch, { key: "Enter" });
    expect(screen.queryByRole("dialog", { name: "New message" })).not.toBeInTheDocument();
  });

  it("single-flights focus and protected-request revalidation while locking the old workspace", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const refresh = deferred<typeof session>();
    vi.mocked(auth.loadSession)
      .mockResolvedValueOnce(session)
      .mockReturnValueOnce(refresh.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    await screen.findByRole("button", { name: /Quarterly review/ });
    fireEvent.keyDown(document, { key: "j" });
    await waitFor(() => expect(container.querySelector<HTMLButtonElement>(".message"))
      .toHaveAttribute("aria-current", "true"));
    const protectedSettings = vi.mocked(api.listMailboxes).mock.calls[0]?.[0] as (
      typeof window.__COPILOTMAIL_MAIL_CONFIG__ & { onUnauthorized?: () => void }
    );

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(auth.loadSession).toHaveBeenCalledTimes(2));
    act(() => {
      protectedSettings.onUnauthorized?.();
      protectedSettings.onUnauthorized?.();
      window.dispatchEvent(new Event("focus"));
    });

    const workspace = container.querySelector<HTMLElement>(".mail-shell")!;
    expect(workspace).toHaveAttribute("inert");
    expect(workspace).toHaveAttribute("aria-busy", "true");
    const verificationNotice = container.querySelector<HTMLElement>(
      "[data-session-verification='checking']",
    );
    expect(verificationNotice).toHaveAttribute("role", "status");
    expect(verificationNotice).toHaveClass(
      "session-check",
      "session-check--checking",
    );
    expect(verificationNotice).toHaveTextContent("Checking your organization session…");
    expect(verificationNotice).not.toHaveClass("sr-only");
    expect(auth.loadSession).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(document, { key: "s" });
    fireEvent.keyDown(document, { key: "c" });
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "d" });
    expect(api.updateMessageState).not.toHaveBeenCalled();
    expect(api.listDrafts).not.toHaveBeenCalled();
    expect(container.querySelector(".composer-dialog")).toBeNull();
    expect(container.querySelector(".command-palette")).toBeNull();

    refresh.resolve(session);
    await act(async () => refresh.promise);
    await waitFor(() => expect(workspace).not.toHaveAttribute("inert"));
    expect(workspace).not.toHaveAttribute("aria-busy");
    expect(auth.loadSession).toHaveBeenCalledTimes(2);
  });

  it("keeps the existing workspace inert after a temporary verification failure and restores it on retry", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const retry = deferred<typeof session>();
    vi.mocked(auth.loadSession)
      .mockReset()
      .mockResolvedValueOnce(session)
      .mockRejectedValueOnce(new Error("gateway unavailable"))
      .mockReturnValueOnce(retry.promise);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Private local body retained across a 502" },
    });

    act(() => window.dispatchEvent(new Event("focus")));
    const verificationFailure = await screen.findByRole("alert");
    expect(verificationFailure).toHaveTextContent(
      "Unable to verify your organization session; try again",
    );
    expect(verificationFailure).toHaveClass(
      "session-check",
      "session-check--failed",
    );
    expect(verificationFailure).not.toHaveAttribute("aria-modal");
    await waitFor(() => expect(verificationFailure).toHaveFocus());
    const workspace = container.querySelector<HTMLElement>(".mail-shell")!;
    expect(workspace).toHaveAttribute("inert");
    expect(workspace).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByRole("dialog", { name: "New message" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", {
      name: "Sign in with your organization account",
    })).not.toBeInTheDocument();

    fireEvent.click(within(verificationFailure).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(auth.loadSession).toHaveBeenCalledTimes(3));
    expect(workspace).toHaveAttribute("inert");
    await act(async () => {
      retry.resolve(session);
      await retry.promise;
    });

    const restoredComposer = await screen.findByRole("dialog", { name: "New message" });
    expect(within(restoredComposer).getByLabelText("Message body"))
      .toHaveValue("Private local body retained across a 502");
    await waitFor(() => expect(container.querySelector<HTMLElement>(".mail-shell"))
      .not.toHaveAttribute("aria-busy"));
    expect(api.listMailboxes).toHaveBeenCalledTimes(1);
    expect(auth.logout).not.toHaveBeenCalled();
  });

  it("preserves workspace state when only the Session expiry is renewed", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(auth.loadSession)
      .mockReset()
      .mockResolvedValueOnce(session)
      .mockResolvedValueOnce({
        ...session,
        expires_at: "2099-07-28T13:00:00.000Z",
      });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Do not remount this workspace on an expiry-only renewal" },
    });

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(auth.loadSession).toHaveBeenCalledTimes(2));
    const composer = await screen.findByRole("dialog", { name: "New message" });
    expect(within(composer).getByLabelText("Message body"))
      .toHaveValue("Do not remount this workspace on an expiry-only renewal");
    expect(api.listMailboxes).toHaveBeenCalledTimes(1);
  });

  it("rebuilds the whole mail workspace when the authenticated context rotates", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const rotatedSession = {
      ...session,
      principal_id: "019b0000-0000-7000-8000-000000000099",
      expires_at: "2099-07-28T13:00:00.000Z",
    };
    vi.mocked(auth.loadSession)
      .mockResolvedValueOnce(session)
      .mockResolvedValueOnce(rotatedSession);
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(await screen.findByRole("button", { name: /Alice/ }));
    expect(await screen.findByText(detail.text_body)).toBeVisible();
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "private@example.test" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Private unsaved session content" },
    });

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(auth.loadSession).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(api.listMailboxes).toHaveBeenCalledTimes(2));
    // The detail URL is the source of truth: the rebuilt workspace re-fetches
    // the routed message instead of leaking the rotated session's state.
    await waitFor(() => expect(api.getMessage).toHaveBeenCalledTimes(2));
    await screen.findByText(detail.text_body);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    expect(await screen.findByRole("dialog", { name: "New message" })).toBeVisible();
    expect(screen.getByLabelText("To")).toHaveValue("");
    expect(screen.getByLabelText("Message body")).toHaveValue("");
  });

  it("cannot bind a late old-identity draft write result into the rotated workspace", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    const rotatedSession = {
      ...session,
      principal_id: "019b0000-0000-7000-8000-000000000099",
      expires_at: "2099-07-28T13:00:00.000Z",
    };
    const staleSave = deferred<DraftResult>();
    vi.mocked(auth.loadSession)
      .mockReset()
      .mockResolvedValueOnce(session)
      .mockResolvedValueOnce(rotatedSession);
    vi.mocked(api.saveDraft)
      .mockReturnValueOnce(staleSave.promise)
      .mockResolvedValueOnce({
        ...draftResult,
        draft_id: "019b0000-0000-7000-8000-000000000031",
      });
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Inbox" });
    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "old-identity@example.test" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "Old identity private body" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });
    expect(api.saveDraft).toHaveBeenCalledTimes(1);
    vi.useRealTimers();

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(api.listMailboxes).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await act(async () => {
      staleSave.resolve(draftResult);
      await flushMicrotasks();
    });

    fireEvent.click(container.querySelector<HTMLButtonElement>(".compose-fab")!);
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "new-identity@example.test" },
    });
    fireEvent.change(screen.getByLabelText("Message body"), {
      target: { value: "New identity body" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });

    expect(api.saveDraft).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.saveDraft).mock.calls[1]?.[1]).toMatchObject({
      draft_id: null,
      expected_version: null,
      to: ["new-identity@example.test"],
      text_body: "New identity body",
    });
  });

  it("revalidates and signs out when a protected mail request returns 401", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.mocked(auth.loadSession)
      .mockResolvedValueOnce(session)
      .mockResolvedValueOnce(null);
    vi.mocked(api.listMailboxes).mockImplementationOnce(async (protectedSettings) => {
      (protectedSettings as typeof protectedSettings & { onUnauthorized?: () => void })
        .onUnauthorized?.();
      throw new Error("mailbox_directory_401");
    });

    render(<App />);

    expect(await screen.findByRole("button", {
      name: "Sign in with your organization account",
    })).toBeVisible();
    expect(auth.loadSession).toHaveBeenCalledTimes(2);
  });

  it("revalidates at the authenticated session expiry deadline", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-24T02:00:00Z"));
    vi.mocked(auth.loadSession)
      .mockResolvedValueOnce({
        ...session,
        expires_at: "2026-08-24T02:00:01Z",
      })
      .mockResolvedValueOnce(null);
    render(<App />);
    await act(async () => flushMicrotasks());
    expect(screen.getByRole("heading", { name: "Inbox" })).toBeVisible();

    await act(async () => {
      vi.advanceTimersByTime(1_000);
      await flushMicrotasks();
    });

    expect(screen.getByRole("button", {
      name: "Sign in with your organization account",
    })).toBeVisible();
    expect(auth.loadSession).toHaveBeenCalledTimes(2);
  });

  it("maps session failures to localized text without exposing machine errors", async () => {
    vi.mocked(auth.loadSession).mockRejectedValueOnce(
      new Error("secret_provider_session_failure"),
    );
    const { container } = render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "暂时无法验证企业会话，请重试",
    );
    const accessSurface = container.querySelector<HTMLElement>("[data-mail-access='login']");
    expect(accessSurface?.querySelector(".mail-access__visual img")).toHaveAttribute("alt", "");
    expect(container.querySelector("[data-mail-access='directory']")).toBeNull();
    expect(screen.queryByText(/secret_provider_session_failure/)).not.toBeInTheDocument();
  });
});
