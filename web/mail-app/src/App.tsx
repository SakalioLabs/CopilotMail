import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getMessage,
  getSubmission,
  listDrafts,
  listInbox,
  listMailboxes,
  requestSubmission,
  restoreDraft,
  saveDraft,
  searchInbox,
  updateMessageState,
} from "./api";
import { loadSession, logout, startLogin } from "./auth";
import { ComposerDialog } from "./components/ComposerDialog";
import { DraftList } from "./components/DraftList";
import { DraftRestoreConfirmDialog } from "./components/DraftRestoreConfirmDialog";
import {
  DesktopNavigation,
  MobileToolbar,
  type MailView,
} from "./components/MailNavigation";
import { MessageDetail as MessageDetailView } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { readRuntimeConfig } from "./config";
import {
  MessageDetailRequestGate,
  detailMatchesSelection,
} from "./detail-request";
import {
  createTranslator,
  detectLocale,
  persistLocale,
  type Locale,
  type MessageKey,
  type TranslationValue,
  type Translator,
} from "./i18n";
import type {
  DraftInput,
  HumanDraftListItem,
  HumanDraftPage,
  HumanSessionResponse,
  InboxPage,
  MailboxDirectoryItem,
  MailboxDirectoryPage,
  MailboxScopedConfig,
  MailSubmission,
  MessageDetail,
  MessageSummary,
} from "./types";

const b64 = (bytes: Uint8Array) => {
  let value = "";
  bytes.forEach((byte) => { value += String.fromCharCode(byte); });
  return btoa(value).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};
const random = (size: number) => b64(crypto.getRandomValues(new Uint8Array(size)));

type AuthState =
  | { kind: "checking" }
  | { kind: "signed-out" }
  | { kind: "authenticated"; session: HumanSessionResponse }
  | { kind: "error"; messageKey: UiErrorKey };

type UiErrorKey =
  | "error.loginFailed"
  | "error.sessionUnavailable"
  | "error.signOutFailed"
  | "error.mailboxDirectoryUnavailable"
  | "error.mailboxDirectoryEmpty"
  | "error.mailboxDirectoryPageUnavailable"
  | "error.inboxUnavailable"
  | "error.messageUnavailable"
  | "error.updateReadState"
  | "error.searchUnavailable"
  | "error.searchTooShort"
  | "error.loadMoreSearch"
  | "error.loadMoreInbox"
  | "error.updateStar"
  | "error.draftsUnavailable"
  | "error.draftsPageUnavailable"
  | "error.draftRestoreUnavailable"
  | "error.draftRestoreConflict"
  | "error.draftSubmissionLocked";

interface ComposerStatus {
  key: MessageKey;
  params?: Record<string, TranslationValue>;
}

interface DetailLoadError {
  messageId: string;
  key: UiErrorKey;
}

interface DetailActionError extends DetailLoadError {
  operation: "read" | "flag";
}

function statusText(t: Translator, status: ComposerStatus) {
  const runtimeTranslator = t as (
    key: MessageKey,
    params?: Record<string, TranslationValue>,
  ) => string;
  return runtimeTranslator(status.key, status.params);
}

function narrowViewport() {
  return typeof window.matchMedia === "function"
    && window.matchMedia("(max-width: 900px)").matches;
}

function preferredMailbox(
  items: readonly MailboxDirectoryItem[],
  preferredId?: string,
): MailboxDirectoryItem | undefined {
  return (preferredId ? items.find((item) => item.mailbox_id === preferredId) : undefined)
    ?? items.find((item) => item.kind === "human")
    ?? items[0];
}

const INITIAL_DIRECTORY_PAGE_LIMIT = 10;

function mergeMailboxPages(
  current: readonly MailboxDirectoryItem[],
  next: readonly MailboxDirectoryItem[],
): MailboxDirectoryItem[] {
  const byId = new Map(current.map((mailbox) => [mailbox.mailbox_id, mailbox]));
  const currentHighWatermark = current.at(-1)?.mailbox_id;
  for (const mailbox of next) {
    const existing = byId.get(mailbox.mailbox_id);
    if (existing) {
      if (
        existing.kind !== mailbox.kind
        || existing.primary_address !== mailbox.primary_address
        || existing.can_send !== mailbox.can_send
      ) {
        throw new Error("mailbox_directory_identity_conflict");
      }
      continue;
    }
    if (currentHighWatermark && mailbox.mailbox_id <= currentHighWatermark) {
      throw new Error("mailbox_directory_order_conflict");
    }
    byId.set(mailbox.mailbox_id, mailbox);
  }
  return Array.from(byId.values());
}

function mergeDraftPages(
  current: readonly HumanDraftListItem[],
  next: readonly HumanDraftListItem[],
): HumanDraftListItem[] {
  const currentTail = current.at(-1)?.draft_id;
  if (currentTail && next[0] && next[0].draft_id >= currentTail) {
    throw new Error("draft_list_order_conflict");
  }
  const ids = new Set(current.map((draft) => draft.draft_id));
  for (const draft of next) {
    if (ids.has(draft.draft_id)) throw new Error("draft_list_identity_conflict");
    ids.add(draft.draft_id);
  }
  return [...current, ...next];
}

const recipients = (value: string) =>
  value.split(/[;,]/).map((item) => item.trim()).filter(Boolean);

export default function App() {
  const settings = useMemo(readRuntimeConfig, []);
  const [locale, setLocale] = useState<Locale>(() => detectLocale());
  const t = useMemo(() => createTranslator(locale), [locale]);
  const [auth, setAuth] = useState<AuthState>({ kind: "checking" });
  const [mailboxes, setMailboxes] = useState<MailboxDirectoryItem[]>([]);
  const [activeMailboxId, setActiveMailboxId] = useState<string>();
  const [mailboxDirectoryPage, setMailboxDirectoryPage] = useState<
    Pick<MailboxDirectoryPage, "next_cursor" | "has_more">
  >({ next_cursor: null, has_more: false });
  const [mailboxDirectoryLoading, setMailboxDirectoryLoading] = useState(false);
  const [mailboxLoadMoreBusy, setMailboxLoadMoreBusy] = useState(false);
  const [mailboxDirectoryError, setMailboxDirectoryError] = useState<UiErrorKey>();
  const [mailboxDirectoryPageError, setMailboxDirectoryPageError] = useState<UiErrorKey>();
  const [mailboxDirectoryRetry, setMailboxDirectoryRetry] = useState(0);
  const [mailView, setMailView] = useState<MailView>("inbox");
  const [page, setPage] = useState<InboxPage>();
  const [searchText, setSearchText] = useState("");
  const [activeSearch, setActiveSearch] = useState<string>();
  const [searchBusy, setSearchBusy] = useState(false);
  const [selected, setSelected] = useState<MessageSummary>();
  const [detail, setDetail] = useState<MessageDetail>();
  const [detailLoadError, setDetailLoadError] = useState<DetailLoadError>();
  const [detailActionError, setDetailActionError] = useState<DetailActionError>();
  const [detailRetryGeneration, setDetailRetryGeneration] = useState(0);
  const [error, setError] = useState<UiErrorKey>();
  const [composeOpen, setComposeOpen] = useState(false);
  const [to, setTo] = useState("");
  const [cc, setCc] = useState("");
  const [bcc, setBcc] = useState("");
  const [subject, setSubject] = useState("");
  const [textBody, setTextBody] = useState("");
  const [replyToMessageId, setReplyToMessageId] = useState<string | null>(null);
  const [draftStatus, setDraftStatus] = useState<ComposerStatus>({
    key: "draft.autosaveHint",
  });
  const [composerKind, setComposerKind] = useState<"new" | "reply" | "restored">("new");
  const [composerInitialFocus, setComposerInitialFocus] = useState<"recipient" | "body">(
    "recipient",
  );
  const [draftPage, setDraftPage] = useState<HumanDraftPage>();
  const [draftListBusy, setDraftListBusy] = useState(false);
  const [draftListError, setDraftListError] = useState<UiErrorKey>();
  const [draftRestoreBusyId, setDraftRestoreBusyId] = useState<string>();
  const [draftRestoreError, setDraftRestoreError] = useState<UiErrorKey>();
  const [draftRefreshGeneration, setDraftRefreshGeneration] = useState(0);
  const [pendingDraftRestore, setPendingDraftRestore] = useState<HumanDraftListItem>();
  const draftId = useRef<string | undefined>(undefined);
  const draftVersion = useRef<number | undefined>(undefined);
  const saveChain = useRef<Promise<void>>(Promise.resolve());
  const latestQueuedSave = useRef(0);
  const pendingSaveTimeout = useRef<number | undefined>(undefined);
  const submissionKey = useRef<string | undefined>(undefined);
  const submissionId = useRef<string | undefined>(undefined);
  const lastSubmission = useRef<MailSubmission | undefined>(undefined);
  const submissionRequestAttempted = useRef(false);
  const submissionLockedRef = useRef(false);
  const submissionTerminalRef = useRef(false);
  const skipNextAutosave = useRef(false);
  const composerGeneration = useRef(0);
  const inboxRequestGeneration = useRef(0);
  const mailViewRef = useRef<MailView>("inbox");
  const draftListGeneration = useRef(0);
  const draftListAbort = useRef<AbortController | undefined>(undefined);
  const draftPageRequest = useRef<string | undefined>(undefined);
  const draftRestoreGeneration = useRef(0);
  const draftRestoreAbort = useRef<AbortController | undefined>(undefined);
  const draftRestoreInFlight = useRef<string | undefined>(undefined);
  const draftFocusAfterRefresh = useRef<string | undefined>(undefined);
  const mailboxDirectoryGeneration = useRef(0);
  const mailboxPageRequest = useRef<string | undefined>(undefined);
  const activeMailboxIdRef = useRef<string | undefined>(undefined);
  const detailRequests = useRef(new MessageDetailRequestGate());
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const inboxHeading = useRef<HTMLHeadingElement>(null);
  const draftHeading = useRef<HTMLHeadingElement>(null);
  const messageButtons = useRef(new Map<string, HTMLButtonElement>());
  const draftButtons = useRef(new Map<string, HTMLButtonElement>());
  const [sendBusy, setSendBusy] = useState(false);
  const [submissionLocked, setSubmissionLocked] = useState(false);
  const [terminalSubmission, setTerminalSubmission] = useState(false);
  const submissionUnresolved = submissionLocked && !terminalSubmission;
  const authenticated = auth.kind === "authenticated";
  const activeMailbox = mailboxes.find((item) => item.mailbox_id === activeMailboxId);
  const mailboxSettings = useMemo<MailboxScopedConfig | undefined>(() => activeMailbox
    ? { ...settings, mailboxId: activeMailbox.mailbox_id }
    : undefined, [activeMailbox, settings]);
  const selectedMessageId = selected?.message_id;
  const registerMessageButton = useCallback(
    (messageId: string) => (node: HTMLButtonElement | null) => {
      if (node) {
        messageButtons.current.set(messageId, node);
      } else {
        messageButtons.current.delete(messageId);
      }
    },
    [],
  );
  const registerDraftButton = useCallback(
    (draftId: string) => (node: HTMLButtonElement | null) => {
      if (node) draftButtons.current.set(draftId, node);
      else draftButtons.current.delete(draftId);
    },
    [],
  );

  const resetComposerPersistence = () => {
    composerGeneration.current += 1;
    if (pendingSaveTimeout.current) {
      window.clearTimeout(pendingSaveTimeout.current);
      pendingSaveTimeout.current = undefined;
    }
    draftId.current = undefined;
    draftVersion.current = undefined;
    submissionKey.current = undefined;
    submissionId.current = undefined;
    lastSubmission.current = undefined;
    submissionRequestAttempted.current = false;
    submissionLockedRef.current = false;
    submissionTerminalRef.current = false;
    skipNextAutosave.current = false;
    setSubmissionLocked(false);
    latestQueuedSave.current = 0;
    saveChain.current = Promise.resolve();
  };

  const clearComposer = () => {
    resetComposerPersistence();
    setComposeOpen(false);
    setTo("");
    setCc("");
    setBcc("");
    setSubject("");
    setTextBody("");
    setReplyToMessageId(null);
    setDraftStatus({ key: "draft.autosaveHint" });
    setComposerKind("new");
    setComposerInitialFocus("recipient");
    setSendBusy(false);
    setTerminalSubmission(false);
  };

  useEffect(() => {
    document.documentElement.lang = locale;
    persistLocale(locale);
  }, [locale]);

  const queueDraftSave = useCallback((
    snapshot: Omit<DraftInput, "draft_id" | "expected_version">,
    allowSubmissionLock = false,
  ) => {
    if (
      !authenticated
      || !mailboxSettings
      || !activeMailbox?.can_send
      || (submissionLockedRef.current && !allowSubmissionLock)
    ) {
      return Promise.reject(new Error("mailbox_not_writable"));
    }
    const generation = composerGeneration.current;
    const sequence = ++latestQueuedSave.current;
    setDraftStatus({ key: "draft.saving" });
    const operation = saveChain.current
      .catch(() => undefined)
      .then(() => {
        if (generation !== composerGeneration.current) {
          throw new Error("stale_composer_session");
        }
        return saveDraft(mailboxSettings, {
          draft_id: draftId.current ?? null,
          expected_version: draftVersion.current ?? null,
          ...snapshot,
        });
      })
      .then((result) => {
        if (generation !== composerGeneration.current) {
          throw new Error("stale_composer_session");
        }
        draftId.current = result.draft_id;
        draftVersion.current = result.version;
        if (sequence === latestQueuedSave.current) {
          setDraftStatus({
            key: result.previous_draft_destroyed
              ? "draft.saved"
              : "draft.savedCleanupFailed",
          });
        }
        return result;
      });
    saveChain.current = operation.then(() => undefined, () => undefined);
    return operation;
  }, [activeMailbox?.can_send, authenticated, mailboxSettings]);

  useEffect(() => {
    loadSession(settings)
      .then((session) => setAuth(session
        ? { kind: "authenticated", session }
        : { kind: "signed-out" }))
      .catch(() => setAuth({
        kind: "error",
        messageKey: "error.sessionUnavailable",
      }));
  }, [settings]);

  useEffect(() => {
    const generation = ++mailboxDirectoryGeneration.current;
    activeMailboxIdRef.current = undefined;
    setMailboxes([]);
    setActiveMailboxId(undefined);
    setMailboxDirectoryPage({ next_cursor: null, has_more: false });
    setMailboxLoadMoreBusy(false);
    mailboxPageRequest.current = undefined;
    setMailboxDirectoryError(undefined);
    setMailboxDirectoryPageError(undefined);
    if (!authenticated) {
      setMailboxDirectoryLoading(false);
      return;
    }

    setMailboxDirectoryLoading(true);
    void (async () => {
      let accumulated: MailboxDirectoryItem[] = [];
      let nextCursor: string | null = null;
      let hasMore = false;
      let firstRequest = true;
      let pagesLoaded = 0;
      try {
        while (true) {
          const directory = await listMailboxes(
            settings,
            firstRequest ? undefined : nextCursor ?? undefined,
          );
          if (generation !== mailboxDirectoryGeneration.current) return;
          if (
            !firstRequest
            && directory.next_cursor !== null
            && nextCursor !== null
            && directory.next_cursor <= nextCursor
          ) throw new Error("mailbox_directory_cursor_did_not_advance");
          accumulated = mergeMailboxPages(accumulated, directory.items);
          pagesLoaded += 1;
          nextCursor = directory.next_cursor;
          hasMore = directory.has_more;
          firstRequest = false;

          const preferenceFound = settings.mailboxId
            ? accumulated.some((item) => item.mailbox_id === settings.mailboxId)
            : accumulated.some((item) => item.kind === "human");
          if (
            preferenceFound
            || !hasMore
            || !nextCursor
            || pagesLoaded >= INITIAL_DIRECTORY_PAGE_LIMIT
          ) break;
        }
      } catch {
        if (generation !== mailboxDirectoryGeneration.current) return;
        if (firstRequest) {
          setMailboxDirectoryError("error.mailboxDirectoryUnavailable");
          return;
        }
        setMailboxDirectoryPageError("error.mailboxDirectoryPageUnavailable");
      } finally {
        if (generation === mailboxDirectoryGeneration.current) {
          setMailboxDirectoryLoading(false);
        }
      }
      if (generation !== mailboxDirectoryGeneration.current || firstRequest) return;

      setMailboxes(accumulated);
      setMailboxDirectoryPage({ next_cursor: nextCursor, has_more: hasMore });
      const selectedMailbox = preferredMailbox(accumulated, settings.mailboxId);
      if (selectedMailbox) {
        activeMailboxIdRef.current = selectedMailbox.mailbox_id;
        setActiveMailboxId(selectedMailbox.mailbox_id);
      } else if (!hasMore) {
        setMailboxDirectoryError("error.mailboxDirectoryEmpty");
      }
    })();
    return () => {
      if (generation === mailboxDirectoryGeneration.current) {
        mailboxDirectoryGeneration.current += 1;
      }
    };
  }, [authenticated, mailboxDirectoryRetry, settings]);

  useEffect(() => {
    if (!authenticated || !mailboxSettings || mailView !== "inbox") {
      inboxRequestGeneration.current += 1;
      return;
    }
    const generation = ++inboxRequestGeneration.current;
    listInbox(mailboxSettings)
      .then((nextPage) => {
        if (generation !== inboxRequestGeneration.current) return;
        setPage(nextPage);
      })
      .catch(() => {
        if (generation === inboxRequestGeneration.current) {
          setError("error.inboxUnavailable");
        }
      });
    return () => {
      if (generation === inboxRequestGeneration.current) {
        inboxRequestGeneration.current += 1;
      }
    };
  }, [authenticated, mailboxSettings, mailView]);
  useEffect(() => {
    draftListAbort.current?.abort();
    const generation = ++draftListGeneration.current;
    draftPageRequest.current = undefined;
    setDraftPage(undefined);
    setDraftListError(undefined);
    setDraftListBusy(false);
    if (
      !authenticated
      || !mailboxSettings
      || !activeMailbox?.can_send
      || mailView !== "drafts"
    ) return;

    const requestMailboxId = mailboxSettings.mailboxId;
    const controller = new AbortController();
    draftListAbort.current = controller;
    setDraftListBusy(true);
    listDrafts(mailboxSettings, undefined, controller.signal)
      .then((nextPage) => {
        if (
          generation !== draftListGeneration.current
          || controller.signal.aborted
          || mailViewRef.current !== "drafts"
          || activeMailboxIdRef.current !== requestMailboxId
        ) return;
        setDraftPage(nextPage);
        const focusDraftId = draftFocusAfterRefresh.current;
        if (focusDraftId) {
          draftFocusAfterRefresh.current = undefined;
          window.requestAnimationFrame(() => {
            (draftButtons.current.get(focusDraftId) ?? draftHeading.current)?.focus();
          });
        }
      })
      .catch(() => {
        if (
          generation === draftListGeneration.current
          && !controller.signal.aborted
          && mailViewRef.current === "drafts"
          && activeMailboxIdRef.current === requestMailboxId
        ) setDraftListError("error.draftsUnavailable");
      })
      .finally(() => {
        if (generation === draftListGeneration.current && !controller.signal.aborted) {
          setDraftListBusy(false);
        }
      });
    return () => controller.abort();
  }, [
    activeMailbox?.can_send,
    authenticated,
    draftRefreshGeneration,
    mailboxSettings,
    mailView,
  ]);
  useEffect(() => {
    setDetail(undefined);
    setDetailLoadError(undefined);
    if (!authenticated || !mailboxSettings || !selectedMessageId) {
      detailRequests.current.invalidate();
      return;
    }
    const token = detailRequests.current.begin(selectedMessageId);
    if (!token) return;
    const controller = new AbortController();
    getMessage(mailboxSettings, selectedMessageId, controller.signal)
      .then((nextDetail) => {
        if (detailRequests.current.accepts(token, nextDetail)) {
          setDetail(nextDetail);
          setDetailLoadError(undefined);
        }
      })
      .catch(() => {
        if (!controller.signal.aborted && detailRequests.current.isCurrent(token)) {
          setDetailLoadError({
            messageId: token.messageId,
            key: "error.messageUnavailable",
          });
        }
      });
    return () => controller.abort();
  }, [authenticated, detailRetryGeneration, mailboxSettings, selectedMessageId]);
  useEffect(() => {
    if (!composeOpen || !authenticated || !mailboxSettings || !activeMailbox?.can_send
      || submissionLockedRef.current
      || recipients(to).length === 0) return;
    if (skipNextAutosave.current) {
      skipNextAutosave.current = false;
      return;
    }
    const generation = composerGeneration.current;
    setDraftStatus({ key: "draft.waitingForAutosave" });
    pendingSaveTimeout.current = window.setTimeout(() => {
      if (submissionLockedRef.current) return;
      void queueDraftSave({
        reply_to_message_id: replyToMessageId,
        to: recipients(to),
        cc: recipients(cc),
        bcc: recipients(bcc),
        subject,
        text_body: textBody,
      }).catch(() => {
        if (generation === composerGeneration.current && !submissionLockedRef.current) {
          setDraftStatus({ key: "draft.autosaveFailed" });
        }
      });
    }, 1200);
    return () => window.clearTimeout(pendingSaveTimeout.current);
  }, [activeMailbox?.can_send, authenticated, bcc, cc, composeOpen, mailboxSettings,
    queueDraftSave, replyToMessageId, subject, textBody, to]);
  const selectedDetail = detailMatchesSelection(detail, selectedMessageId)
    ? detail
    : undefined;
  const applyState = (
    messageId: string,
    update: Partial<Pick<MessageSummary, "is_read" | "is_flagged">>,
  ) => {
    setPage((current) => current && ({
      ...current,
      items: current.items.map((item) => item.message_id === messageId
        ? { ...item, ...update }
        : item),
    }));
    setSelected((current) => current?.message_id === messageId
      ? { ...current, ...update }
      : current);
    setDetail((current) => current?.message.message_id === messageId
      ? { ...current, message: { ...current.message, ...update } }
      : current);
  };
  const openMessage = (message: MessageSummary) => {
    if (!mailboxSettings) return;
    if (selectedMessageId !== message.message_id) {
      setDetailLoadError(undefined);
      setDetailActionError(undefined);
      detailRequests.current.invalidate();
      setDetail(undefined);
    } else if (detailLoadError?.messageId === message.message_id) {
      setDetailLoadError(undefined);
      detailRequests.current.invalidate();
      setDetailRetryGeneration((generation) => generation + 1);
    }
    setSelected(message);
    if (narrowViewport()) {
      window.requestAnimationFrame(() => detailHeading.current?.focus());
    }
    if (!message.is_read && authenticated) {
      const requestMailboxId = mailboxSettings.mailboxId;
      updateMessageState(mailboxSettings, message.message_id, { is_read: true })
        .then((state) => {
          if (activeMailboxIdRef.current !== requestMailboxId) return;
          applyState(state.message_id, { is_read: state.is_read });
          setDetailActionError((current) => current?.messageId === state.message_id
            && current.operation === "read"
            ? undefined
            : current);
        })
        .catch(() => {
          if (activeMailboxIdRef.current === requestMailboxId) {
            setDetailActionError({
              messageId: message.message_id,
              operation: "read",
              key: "error.updateReadState",
            });
          }
        });
    }
  };
  const refreshMessages = async () => {
    if (!authenticated || !mailboxSettings) return;
    const generation = ++inboxRequestGeneration.current;
    setSearchBusy(true);
    setError(undefined);
    try {
      const nextPage = activeSearch
        ? await searchInbox(mailboxSettings, activeSearch)
        : await listInbox(mailboxSettings);
      if (generation === inboxRequestGeneration.current) {
        setPage(nextPage);
      }
    } catch {
      if (generation === inboxRequestGeneration.current) {
        setError(activeSearch ? "error.searchUnavailable" : "error.inboxUnavailable");
      }
    } finally {
      if (generation === inboxRequestGeneration.current) {
        setSearchBusy(false);
      }
    }
  };
  const clearMessageSelection = (restoreFocus = false) => {
    const messageId = selectedMessageId;
    detailRequests.current.invalidate();
    setSelected(undefined);
    setDetail(undefined);
    setDetailLoadError(undefined);
    setDetailActionError(undefined);
    if (restoreFocus && messageId) {
      window.requestAnimationFrame(() => messageButtons.current.get(messageId)?.focus());
    }
  };
  const switchView = (nextView: MailView) => {
    if (nextView === mailViewRef.current) return;
    if (nextView === "drafts" && !activeMailbox?.can_send) return;
    mailViewRef.current = nextView;
    setMailView(nextView);
    clearMessageSelection();
    setPendingDraftRestore(undefined);
    if (nextView === "drafts") {
      inboxRequestGeneration.current += 1;
      setDraftRestoreError(undefined);
    } else {
      draftListGeneration.current += 1;
      draftListAbort.current?.abort();
      draftRestoreGeneration.current += 1;
      draftRestoreAbort.current?.abort();
      draftRestoreInFlight.current = undefined;
      draftFocusAfterRefresh.current = undefined;
      setDraftRestoreBusyId(undefined);
    }
    if (narrowViewport()) {
      window.requestAnimationFrame(() => {
        (nextView === "drafts" ? draftHeading.current : inboxHeading.current)?.focus();
      });
    }
  };
  const activateMailbox = (mailbox: MailboxDirectoryItem) => {
    if (mailbox.mailbox_id === activeMailboxIdRef.current) return;
    activeMailboxIdRef.current = mailbox.mailbox_id;
    mailViewRef.current = "inbox";
    setMailView("inbox");
    inboxRequestGeneration.current += 1;
    draftListGeneration.current += 1;
    draftListAbort.current?.abort();
    draftRestoreGeneration.current += 1;
    draftRestoreAbort.current?.abort();
    draftRestoreInFlight.current = undefined;
    draftFocusAfterRefresh.current = undefined;
    draftPageRequest.current = undefined;
    detailRequests.current.invalidate();
    setPage(undefined);
    setDraftPage(undefined);
    setDraftListBusy(false);
    setDraftListError(undefined);
    setDraftRestoreBusyId(undefined);
    setDraftRestoreError(undefined);
    setPendingDraftRestore(undefined);
    setSearchText("");
    setActiveSearch(undefined);
    setSearchBusy(false);
    setError(undefined);
    clearMessageSelection();
    clearComposer();
    setActiveMailboxId(mailbox.mailbox_id);
  };
  const switchMailbox = (mailboxId: string) => {
    if (submissionLockedRef.current && !submissionTerminalRef.current) {
      setComposeOpen(true);
      return;
    }
    const mailbox = mailboxes.find((item) => item.mailbox_id === mailboxId);
    if (mailbox) activateMailbox(mailbox);
  };
  const refreshDrafts = () => {
    if (
      mailViewRef.current !== "drafts"
      || !activeMailbox?.can_send
      || draftRestoreInFlight.current !== undefined
    ) return;
    setDraftRestoreError(undefined);
    setDraftRefreshGeneration((generation) => generation + 1);
  };
  const loadMoreDrafts = async () => {
    if (
      !authenticated
      || !mailboxSettings
      || mailViewRef.current !== "drafts"
      || !activeMailbox?.can_send
      || !draftPage?.has_more
      || !draftPage.next_cursor
      || draftListBusy
      || draftRestoreInFlight.current !== undefined
      || draftPageRequest.current !== undefined
    ) return;
    const generation = draftListGeneration.current;
    const requestMailboxId = mailboxSettings.mailboxId;
    const cursor = draftPage.next_cursor;
    const controller = new AbortController();
    draftListAbort.current?.abort();
    draftListAbort.current = controller;
    draftPageRequest.current = cursor;
    setDraftListBusy(true);
    setDraftListError(undefined);
    try {
      const nextPage = await listDrafts(mailboxSettings, cursor, controller.signal);
      if (
        generation !== draftListGeneration.current
        || controller.signal.aborted
        || mailViewRef.current !== "drafts"
        || activeMailboxIdRef.current !== requestMailboxId
      ) return;
      setDraftPage((current) => current && ({
        ...nextPage,
        items: mergeDraftPages(current.items, nextPage.items),
      }));
    } catch {
      if (
        generation === draftListGeneration.current
        && !controller.signal.aborted
        && mailViewRef.current === "drafts"
        && activeMailboxIdRef.current === requestMailboxId
      ) setDraftListError("error.draftsPageUnavailable");
    } finally {
      if (draftPageRequest.current === cursor) draftPageRequest.current = undefined;
      if (generation === draftListGeneration.current && !controller.signal.aborted) {
        setDraftListBusy(false);
      }
    }
  };
  const composerHasContent = () => Boolean(
    draftId.current
    || replyToMessageId
    || to
    || cc
    || bcc
    || subject
    || textBody,
  );
  const performDraftRestore = async (draft: HumanDraftListItem) => {
    if (
      !authenticated
      || !mailboxSettings
      || !activeMailbox?.can_send
      || mailViewRef.current !== "drafts"
      || draftRestoreInFlight.current !== undefined
      || (submissionLockedRef.current && !submissionTerminalRef.current)
    ) return;
    const generation = ++draftRestoreGeneration.current;
    const requestMailboxId = mailboxSettings.mailboxId;
    const controller = new AbortController();
    draftRestoreAbort.current?.abort();
    draftRestoreAbort.current = controller;
    draftRestoreInFlight.current = draft.draft_id;
    setDraftRestoreBusyId(draft.draft_id);
    setDraftRestoreError(undefined);
    try {
      const restored = await restoreDraft(
        mailboxSettings,
        draft,
        controller.signal,
      );
      if (
        generation !== draftRestoreGeneration.current
        || controller.signal.aborted
        || mailViewRef.current !== "drafts"
        || activeMailboxIdRef.current !== requestMailboxId
      ) return;
      resetComposerPersistence();
      draftId.current = restored.draft_id;
      draftVersion.current = restored.version;
      skipNextAutosave.current = true;
      setTo(restored.to.join("; "));
      setCc(restored.cc.join("; "));
      setBcc(restored.bcc.join("; "));
      setSubject(restored.subject);
      setTextBody(restored.text_body);
      setReplyToMessageId(restored.reply_to_message_id);
      setComposerKind("restored");
      setComposerInitialFocus("body");
      setDraftStatus({ key: "draft.restored" });
      setSendBusy(false);
      submissionTerminalRef.current = false;
      setTerminalSubmission(false);
      setPendingDraftRestore(undefined);
      setComposeOpen(true);
    } catch (cause) {
      if (
        generation !== draftRestoreGeneration.current
        || controller.signal.aborted
        || mailViewRef.current !== "drafts"
        || activeMailboxIdRef.current !== requestMailboxId
      ) return;
      if (cause instanceof Error && cause.message === "draft_restore_409") {
        draftFocusAfterRefresh.current = draft.draft_id;
        setDraftRestoreError("error.draftRestoreConflict");
        setDraftRefreshGeneration((current) => current + 1);
      } else {
        setDraftRestoreError("error.draftRestoreUnavailable");
        window.requestAnimationFrame(() => draftButtons.current.get(draft.draft_id)?.focus());
      }
      setPendingDraftRestore(undefined);
    } finally {
      if (generation === draftRestoreGeneration.current && !controller.signal.aborted) {
        draftRestoreInFlight.current = undefined;
        setDraftRestoreBusyId(undefined);
      }
    }
  };
  const openDraft = (draft: HumanDraftListItem) => {
    if (submissionLockedRef.current && !submissionTerminalRef.current) {
      setDraftRestoreError("error.draftSubmissionLocked");
      return;
    }
    if (
      draftId.current === draft.draft_id
      && draftVersion.current !== undefined
      && draftVersion.current >= draft.version
    ) {
      setComposerKind("restored");
      setComposerInitialFocus("body");
      setDraftStatus({ key: "draft.restored" });
      setComposeOpen(true);
      return;
    }
    if (composerHasContent()) {
      setPendingDraftRestore(draft);
      return;
    }
    void performDraftRestore(draft);
  };
  const loadMoreMailboxes = async () => {
    if (
      !authenticated
      || mailboxPageRequest.current !== undefined
      || !mailboxDirectoryPage.has_more
      || !mailboxDirectoryPage.next_cursor
    ) return;
    const generation = mailboxDirectoryGeneration.current;
    const cursor = mailboxDirectoryPage.next_cursor;
    mailboxPageRequest.current = cursor;
    setMailboxLoadMoreBusy(true);
    setMailboxDirectoryPageError(undefined);
    try {
      const nextPage = await listMailboxes(settings, cursor);
      if (generation !== mailboxDirectoryGeneration.current) return;
      if (nextPage.next_cursor !== null && nextPage.next_cursor <= cursor) {
        throw new Error("mailbox_directory_cursor_did_not_advance");
      }
      const merged = mergeMailboxPages(mailboxes, nextPage.items);
      setMailboxes(merged);
      setMailboxDirectoryPage({
        next_cursor: nextPage.next_cursor,
        has_more: nextPage.has_more,
      });
      if (!activeMailboxIdRef.current) {
        const selectedMailbox = preferredMailbox(merged, settings.mailboxId);
        if (selectedMailbox) activateMailbox(selectedMailbox);
      }
      if (merged.length === 0 && !nextPage.has_more) {
        setMailboxDirectoryError("error.mailboxDirectoryEmpty");
      }
    } catch {
      if (generation === mailboxDirectoryGeneration.current) {
        setMailboxDirectoryPageError("error.mailboxDirectoryPageUnavailable");
      }
    } finally {
      if (generation === mailboxDirectoryGeneration.current) {
        if (mailboxPageRequest.current === cursor) mailboxPageRequest.current = undefined;
        setMailboxLoadMoreBusy(false);
      }
    }
  };
  const submitSearch = async () => {
    if (!authenticated || !mailboxSettings || searchBusy) return;
    const text = searchText.trim();
    if (text.length === 0) {
      const generation = ++inboxRequestGeneration.current;
      setActiveSearch(undefined);
      clearMessageSelection();
      setSearchBusy(true);
      try {
        const nextPage = await listInbox(mailboxSettings);
        if (generation === inboxRequestGeneration.current) {
          setPage(nextPage);
          setError(undefined);
        }
      } catch {
        if (generation === inboxRequestGeneration.current) {
          setError("error.inboxUnavailable");
        }
      } finally {
        if (generation === inboxRequestGeneration.current) {
          setSearchBusy(false);
        }
      }
      return;
    }
    if ([...text].length < 2) {
      setError("error.searchTooShort");
      return;
    }
    const generation = ++inboxRequestGeneration.current;
    setSearchBusy(true);
    setError(undefined);
    try {
      const result = await searchInbox(mailboxSettings, text);
      if (generation === inboxRequestGeneration.current) {
        setActiveSearch(text);
        setSearchText(text);
        setPage(result);
        clearMessageSelection();
      }
    } catch {
      if (generation === inboxRequestGeneration.current) {
        setError("error.searchUnavailable");
      }
    } finally {
      if (generation === inboxRequestGeneration.current) {
        setSearchBusy(false);
      }
    }
  };
  const clearSearch = async () => {
    if (!authenticated || !mailboxSettings || searchBusy) return;
    const generation = ++inboxRequestGeneration.current;
    setSearchText("");
    setActiveSearch(undefined);
    clearMessageSelection();
    setSearchBusy(true);
    try {
      const nextPage = await listInbox(mailboxSettings);
      if (generation === inboxRequestGeneration.current) {
        setPage(nextPage);
        setError(undefined);
      }
    } catch {
      if (generation === inboxRequestGeneration.current) {
        setError("error.inboxUnavailable");
      }
    } finally {
      if (generation === inboxRequestGeneration.current) {
        setSearchBusy(false);
      }
    }
  };
  const loadMore = async () => {
    if (!authenticated || !mailboxSettings || !page?.next_cursor || searchBusy) return;
    const generation = ++inboxRequestGeneration.current;
    const currentItems = page.items;
    setSearchBusy(true);
    try {
      const next = activeSearch
        ? await searchInbox(mailboxSettings, activeSearch, page.next_cursor)
        : await listInbox(mailboxSettings, page.next_cursor);
      if (generation === inboxRequestGeneration.current) {
        setPage({
          ...next,
          items: [...currentItems, ...next.items],
        });
        setError(undefined);
      }
    } catch {
      if (generation === inboxRequestGeneration.current) {
        setError(activeSearch ? "error.loadMoreSearch" : "error.loadMoreInbox");
      }
    } finally {
      if (generation === inboxRequestGeneration.current) {
        setSearchBusy(false);
      }
    }
  };
  const toggleFlag = () => {
    if (!selected || !authenticated || !mailboxSettings) return;
    const requestMailboxId = mailboxSettings.mailboxId;
    updateMessageState(mailboxSettings, selected.message_id, { is_flagged: !selected.is_flagged })
      .then((state) => {
        if (activeMailboxIdRef.current !== requestMailboxId) return;
        applyState(state.message_id, { is_flagged: state.is_flagged });
        setDetailActionError((current) => current?.messageId === state.message_id
          && current.operation === "flag"
          ? undefined
          : current);
      })
      .catch(() => {
        if (activeMailboxIdRef.current === requestMailboxId) {
          setDetailActionError({
            messageId: selected.message_id,
            operation: "flag",
            key: "error.updateStar",
          });
        }
      });
  };
  const retryMessageDetail = () => {
    if (!selectedMessageId) return;
    setDetailLoadError(undefined);
    detailRequests.current.invalidate();
    setDetailRetryGeneration((generation) => generation + 1);
  };
  const observeSubmission = async (initial: MailSubmission, generation: number) => {
    if (!mailboxSettings || generation !== composerGeneration.current) return;
    let current = initial;
    if (
      current.mailbox_id !== mailboxSettings.mailboxId
      || current.draft_id !== draftId.current
      || current.draft_version !== draftVersion.current
    ) {
      throw new Error("submission_mailbox_mismatch");
    }
    submissionId.current = current.submission_id;
    lastSubmission.current = current;
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (generation !== composerGeneration.current) return;
      if (current.status === "accepted") {
        if (current.delivery?.terminal) {
          submissionTerminalRef.current = true;
          setTerminalSubmission(true);
          if (current.delivery.failed_count === 0) {
            setDraftStatus({
              key: "delivery.allDelivered",
              params: { count: current.delivery.delivered_count },
            });
            window.setTimeout(() => {
              if (generation === composerGeneration.current) {
                setComposeOpen(false);
              }
            }, 900);
          } else {
            setDraftStatus({
              key: "delivery.completed",
              params: {
                delivered: current.delivery.delivered_count,
                failed: current.delivery.failed_count,
              },
            });
          }
          return;
        }
        setDraftStatus(current.delivery
          ? {
              key: "delivery.acceptedProgress",
              params: {
                delivered: current.delivery.delivered_count,
                total: current.delivery.recipient_count,
              },
            }
          : { key: "delivery.acceptedPending" });
      }
      else if (current.status === "rejected") {
        setDraftStatus({ key: "delivery.rejected" });
        submissionTerminalRef.current = true;
        setTerminalSubmission(true);
        return;
      }
      else if (current.status === "manual_review") {
        setDraftStatus({ key: "delivery.manualReview" });
        submissionTerminalRef.current = true;
        setTerminalSubmission(true);
        return;
      }
      else {
        setDraftStatus({
          key: current.status === "pending"
            ? "delivery.queued"
            : "delivery.confirming",
        });
      }
      await new Promise((resolve) => window.setTimeout(resolve, 1500));
      if (generation !== composerGeneration.current) return;
      current = await getSubmission(mailboxSettings, current);
      if (generation !== composerGeneration.current) return;
      if (
        current.mailbox_id !== mailboxSettings.mailboxId
        || current.draft_id !== draftId.current
        || current.draft_version !== draftVersion.current
      ) {
        throw new Error("submission_mailbox_mismatch");
      }
      lastSubmission.current = current;
    }
    setDraftStatus({ key: "delivery.stillProcessing" });
  };
  const sendMessage = async () => {
    if (
      !authenticated
      || !mailboxSettings
      || !activeMailbox?.can_send
      || sendBusy
      || submissionTerminalRef.current
      || recipients(to).length === 0
    ) return;
    const generation = composerGeneration.current;
    const submissionSnapshot = {
      reply_to_message_id: replyToMessageId,
      to: recipients(to),
      cc: recipients(cc),
      bcc: recipients(bcc),
      subject,
      text_body: textBody,
    };
    submissionLockedRef.current = true;
    setSubmissionLocked(true);
    setSendBusy(true);
    if (pendingSaveTimeout.current) {
      window.clearTimeout(pendingSaveTimeout.current);
      pendingSaveTimeout.current = undefined;
    }
    try {
      let current: MailSubmission;
      if (submissionId.current) {
        if (!lastSubmission.current) throw new Error("submission_state_missing");
        current = await getSubmission(mailboxSettings, lastSubmission.current);
      } else {
        if (!submissionRequestAttempted.current) {
          await queueDraftSave(submissionSnapshot, true);
        }
        if (!draftId.current || !draftVersion.current) throw new Error("draft_not_saved");
        submissionKey.current ??= random(32);
        submissionRequestAttempted.current = true;
        current = await requestSubmission(
          mailboxSettings,
          draftId.current,
          draftVersion.current,
          submissionKey.current,
        );
      }
      await observeSubmission(current, generation);
    } catch {
      if (generation === composerGeneration.current) {
        const submissionCouldExist = submissionRequestAttempted.current
          || submissionId.current !== undefined;
        if (!submissionCouldExist) {
          submissionLockedRef.current = false;
          submissionTerminalRef.current = false;
          setSubmissionLocked(false);
        }
        setDraftStatus({
          key: submissionCouldExist
            ? "delivery.requestUnknown"
            : "delivery.draftSaveFailed",
        });
      }
    } finally {
      if (generation === composerGeneration.current) setSendBusy(false);
    }
  };
  const openComposer = () => {
    if (!activeMailbox?.can_send || draftRestoreInFlight.current !== undefined) return;
    if (submissionLockedRef.current && !submissionTerminalRef.current) {
      setComposeOpen(true);
      return;
    }
    if (terminalSubmission || composerKind === "reply") {
      resetComposerPersistence();
      setTo("");
      setCc("");
      setBcc("");
      setSubject("");
      setTextBody("");
      setReplyToMessageId(null);
      setDraftStatus({ key: "draft.autosaveHint" });
      setComposerKind("new");
      setComposerInitialFocus("recipient");
      submissionTerminalRef.current = false;
      setTerminalSubmission(false);
    }
    setComposeOpen(true);
  };
  const replyToSelected = () => {
    if (!activeMailbox?.can_send || !selectedDetail?.reply_to_message_id) return;
    if (submissionLockedRef.current && !submissionTerminalRef.current) {
      setComposeOpen(true);
      return;
    }
    resetComposerPersistence();
    const replyAddress = selectedDetail.message.from[0]?.email ?? "";
    setTo(replyAddress);
    setCc("");
    setBcc("");
    setSubject(/^re:/i.test(selectedDetail.message.subject)
      ? selectedDetail.message.subject
      : t("compose.replySubject", {
          subject: selectedDetail.message.subject || t("subject.none"),
        }));
    setTextBody("");
    setReplyToMessageId(selectedDetail.reply_to_message_id);
    setDraftStatus({ key: "draft.replyAutosaveHint" });
    setComposerKind("reply");
    setComposerInitialFocus("recipient");
    submissionTerminalRef.current = false;
    setTerminalSubmission(false);
    setComposeOpen(true);
  };
  const signOut = () => {
    setAuth({ kind: "checking" });
    logout(settings)
      .then(() => setAuth({ kind: "signed-out" }))
      .catch(() => setAuth({
        kind: "error",
        messageKey: "error.signOutFailed",
      }));
  };
  const signIn = () => {
    try {
      startLogin(settings);
    } catch {
      setAuth({ kind: "error", messageKey: "error.loginFailed" });
    }
  };

  if (auth.kind === "checking") {
    return (
      <main className="login">
        <div>
          <span className="mark" aria-hidden="true">C</span>
          <h1>{t("app.name")}</h1>
          <p role="status">{t("auth.checking")}</p>
        </div>
      </main>
    );
  }
  if (auth.kind !== "authenticated") {
    return (
      <main className="login">
        <div>
          <span className="mark" aria-hidden="true">C</span>
          <h1>{t("app.name")}</h1>
          <p>{t("auth.tagline")}</p>
          {error && <p role="alert">{t(error)}</p>}
          {auth.kind === "error" && <p role="alert">{t(auth.messageKey)}</p>}
          <button onClick={signIn}>{t("auth.signIn")}</button>
        </div>
      </main>
    );
  }
  if (!activeMailbox) {
    const directoryMessage = mailboxDirectoryError
      ?? (mailboxDirectoryLoading
        ? "mailbox.directoryLoading"
        : mailboxDirectoryPage.has_more
          ? "mailbox.directoryNeedsMore"
          : "mailbox.directoryLoading");
    return (
      <main className="login mailbox-gate">
        <div>
          <span className="mark" aria-hidden="true">C</span>
          <h1>{t("mailbox.directoryTitle")}</h1>
          <p
            role={mailboxDirectoryError || mailboxDirectoryPageError ? "alert" : "status"}
          >
            {t(mailboxDirectoryPageError ?? directoryMessage)}
          </p>
          {mailboxDirectoryPage.has_more && mailboxDirectoryPage.next_cursor && (
            <button
              disabled={mailboxLoadMoreBusy}
              onClick={() => void loadMoreMailboxes()}
            >
              {mailboxLoadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
            </button>
          )}
          {mailboxDirectoryError === "error.mailboxDirectoryUnavailable" && (
            <button onClick={() => setMailboxDirectoryRetry((value) => value + 1)}>
              {t("mailbox.retryDirectory")}
            </button>
          )}
          <button className="mailbox-gate-sign-out" onClick={signOut}>
            {t("nav.signOut")}
          </button>
        </div>
      </main>
    );
  }
  return (
    <main className={`mail-shell${mailView === "inbox" && selected ? " message-open" : ""}`}>
      <DesktopNavigation
        activeMailbox={activeMailbox}
        activeView={mailView}
        canCompose={activeMailbox.can_send}
        composeBusy={draftRestoreBusyId !== undefined}
        sessionLocked={submissionUnresolved}
        locale={locale}
        mailboxes={mailboxes}
        mailboxHasMore={mailboxDirectoryPage.has_more}
        mailboxLoadMoreBusy={mailboxLoadMoreBusy}
        mailboxLoadMoreError={mailboxDirectoryPageError
          ? t(mailboxDirectoryPageError)
          : undefined}
        onLocaleChange={setLocale}
        onMailboxChange={switchMailbox}
        onMailboxLoadMore={() => void loadMoreMailboxes()}
        onCompose={openComposer}
        onSignOut={signOut}
        onViewChange={switchView}
        t={t}
      />
      <MobileToolbar
        activeMailbox={activeMailbox}
        activeView={mailView}
        canCompose={activeMailbox.can_send}
        composeBusy={draftRestoreBusyId !== undefined}
        sessionLocked={submissionUnresolved}
        locale={locale}
        mailboxes={mailboxes}
        mailboxHasMore={mailboxDirectoryPage.has_more}
        mailboxLoadMoreBusy={mailboxLoadMoreBusy}
        mailboxLoadMoreError={mailboxDirectoryPageError
          ? t(mailboxDirectoryPageError)
          : undefined}
        onLocaleChange={setLocale}
        onMailboxChange={switchMailbox}
        onMailboxLoadMore={() => void loadMoreMailboxes()}
        onCompose={openComposer}
        onSignOut={signOut}
        onViewChange={switchView}
        t={t}
      />
      {mailView === "inbox" ? (
        <>
          <MessageList
            activeSearch={activeSearch}
            error={error ? t(error) : undefined}
            headingRef={inboxHeading}
            locale={locale}
            page={page}
            searchBusy={searchBusy}
            searchText={searchText}
            selectedMessageId={selectedMessageId}
            t={t}
            onClearSearch={() => void clearSearch()}
            onLoadMore={() => void loadMore()}
            onOpenMessage={openMessage}
            onRefresh={() => void refreshMessages()}
            onSearch={() => void submitSearch()}
            onSearchTextChange={setSearchText}
            messageButtonRef={registerMessageButton}
          />
          <MessageDetailView
            actionError={detailActionError && detailActionError.messageId === selectedMessageId
              ? t(detailActionError.key)
              : undefined}
            detail={selectedDetail}
            headingRef={detailHeading}
            loadError={detailLoadError && detailLoadError.messageId === selectedMessageId
              ? t(detailLoadError.key)
              : undefined}
            locale={locale}
            canReply={activeMailbox.can_send && !submissionUnresolved}
            selected={selected}
            t={t}
            onBack={() => clearMessageSelection(true)}
            onReply={replyToSelected}
            onRetry={retryMessageDetail}
            onToggleFlag={toggleFlag}
          />
        </>
      ) : (
        <>
          <DraftList
            draftButtonRef={registerDraftButton}
            error={draftListError ? t(draftListError) : undefined}
            headingRef={draftHeading}
            listBusy={draftListBusy}
            locale={locale}
            page={draftPage}
            restoreBusyId={draftRestoreBusyId}
            restoreError={draftRestoreError ? t(draftRestoreError) : undefined}
            t={t}
            onLoadMore={() => void loadMoreDrafts()}
            onRefresh={refreshDrafts}
            onRestore={openDraft}
          />
          <section className="detail draft-detail" data-dialog-background>
            <div className="empty-detail">
              <span aria-hidden="true">{t("icon.emptyDetail")}</span>
              <p>{t("drafts.detailHint")}</p>
            </div>
          </section>
        </>
      )}
      {composeOpen && (
        <ComposerDialog
          fromAddress={activeMailbox.primary_address}
          t={t}
          replyMode={Boolean(replyToMessageId)}
          restoredMode={composerKind === "restored"}
          initialFocus={composerInitialFocus}
          to={to}
          cc={cc}
          bcc={bcc}
          subject={subject}
          textBody={textBody}
          draftStatus={statusText(t, draftStatus)}
          submissionLocked={submissionLocked}
          sendBusy={sendBusy}
          terminalSubmission={terminalSubmission}
          onToChange={(value) => {
            if (!submissionLockedRef.current) setTo(value);
          }}
          onCcChange={(value) => {
            if (!submissionLockedRef.current) setCc(value);
          }}
          onBccChange={(value) => {
            if (!submissionLockedRef.current) setBcc(value);
          }}
          onSubjectChange={(value) => {
            if (!submissionLockedRef.current) setSubject(value);
          }}
          onTextBodyChange={(value) => {
            if (!submissionLockedRef.current) setTextBody(value);
          }}
          onClose={() => setComposeOpen(false)}
          onSend={sendMessage}
        />
      )}
      {pendingDraftRestore && !composeOpen && (
        <DraftRestoreConfirmDialog
          busy={draftRestoreBusyId === pendingDraftRestore.draft_id}
          t={t}
          onCancel={() => setPendingDraftRestore(undefined)}
          onConfirm={() => {
            const draft = pendingDraftRestore;
            void performDraftRestore(draft);
          }}
        />
      )}
    </main>
  );
}
