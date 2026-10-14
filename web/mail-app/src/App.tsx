import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CommandPalette,
  ConfirmDialog,
  type BadgeTone,
  type CommandPaletteCommand,
  type CommandPaletteFocusPolicy,
} from "@copilotmail/design-system";
import {
  getMessage,
  listInbox,
  listMailboxes,
  searchInbox,
  updateMessageState,
} from "./api";
import { loadSession, logout, startLogin } from "./auth";
import { ComposerDialog } from "./components/ComposerDialog";
import { DraftList } from "./components/DraftList";
import { DraftRestoreConfirmDialog } from "./components/DraftRestoreConfirmDialog";
import { MailAccessSurface } from "./components/MailAccessSurface";
import {
  DesktopNavigation,
  MobileNavigation,
  type MailView,
} from "./components/MailNavigation";
import { MessageDetail as MessageDetailView } from "./components/MessageDetail";
import { MailTopbar } from "./components/MailTopbar";
import { MessageList } from "./components/MessageList";
import { SentList } from "./components/SentList";
import { SettingsDialog } from "./components/SettingsDialog";
import { SessionVerificationNotice } from "./components/SessionVerificationNotice";
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
import { useMailKeyboardShortcuts } from "./mail-keyboard-shortcuts";
import { MailWorkspaceShell } from "./shell/MailWorkspaceShell";
import {
  useComposerSession,
  type ComposerReply,
  type ComposerStatus,
} from "./useComposerSession";
import { useDraftDirectory } from "./useDraftDirectory";
import { useSentDirectory } from "./useSentDirectory";
import type {
  InboxPage,
  MailboxDirectoryItem,
  MailboxDirectoryPage,
  MailboxScopedConfig,
  MessageDetail,
  MessageSummary,
} from "./types";

type AuthState =
  | { kind: "checking" }
  | { kind: "signed-out" }
  | { kind: "authenticated"; sessionContextKey: string; expiresAt: number }
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
  | "error.draftSubmissionLocked"
  | "error.sentUnavailable"
  | "error.sentPageUnavailable";

interface DetailLoadError {
  messageId: string;
  key: UiErrorKey;
}

interface DetailActionError extends DetailLoadError {
  operation: "read" | "flag";
}

type PendingComposerTransition =
  | { kind: "mailbox"; mailboxId: string }
  | { kind: "new-message" }
  | { kind: "reply"; reply: ComposerReply }
  | { kind: "sign-out" };

function statusText(t: Translator, status: ComposerStatus) {
  const runtimeTranslator = t as (
    key: MessageKey,
    params?: Record<string, TranslationValue>,
  ) => string;
  return runtimeTranslator(status.key, status.params);
}

function composerStatusTone(status: ComposerStatus): BadgeTone {
  switch (status.key) {
    case "draft.saved":
    case "draft.restored":
    case "delivery.allDelivered":
      return "positive";
    case "draft.autosaveFailed":
    case "delivery.rejected":
    case "delivery.draftSaveFailed":
      return "critical";
    case "draft.savedCleanupFailed":
    case "delivery.completed":
    case "delivery.manualReview":
    case "delivery.stillProcessing":
    case "delivery.requestUnknown":
      return "warning";
    case "draft.saving":
    case "draft.waitingForAutosave":
    case "delivery.acceptedProgress":
    case "delivery.acceptedPending":
    case "delivery.queued":
    case "delivery.confirming":
      return "info";
    default:
      return "neutral";
  }
}

function narrowViewport() {
  return typeof window.matchMedia === "function"
    && window.matchMedia("(max-width: 1007px)").matches;
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

interface AuthenticatedWorkspaceProps {
  locale: Locale;
  onLocaleChange: (locale: Locale) => void;
  onSignOut: () => void;
  onUnauthorized: () => void;
  runtimeSettings: ReturnType<typeof readRuntimeConfig>;
  sessionLocked: boolean;
  t: Translator;
}

function AuthenticatedWorkspace({
  locale,
  onLocaleChange,
  onSignOut,
  onUnauthorized,
  runtimeSettings,
  sessionLocked,
  t,
}: AuthenticatedWorkspaceProps) {
  const settings = useMemo(
    () => ({ ...runtimeSettings, onUnauthorized }),
    [onUnauthorized, runtimeSettings],
  );
  const [mailboxes, setMailboxes] = useState<MailboxDirectoryItem[]>([]);
  const [activeMailboxId, setActiveMailboxId] = useState<string>();
  const authenticated = true;
  const activeMailbox = mailboxes.find((item) => item.mailbox_id === activeMailboxId);
  const mailboxSettings = useMemo<MailboxScopedConfig | undefined>(() => activeMailbox
    ? { ...settings, mailboxId: activeMailbox.mailbox_id }
    : undefined, [activeMailbox, settings]);
  const [mailView, setMailView] = useState<MailView>("inbox");
  const composer = useComposerSession({
    authenticated,
    mailboxSettings,
    canSend: activeMailbox?.can_send ?? false,
    sessionLocked,
  });
  const {
    composeOpen,
    to,
    cc,
    bcc,
    subject,
    textBody,
    replyToMessageId,
    draftStatus,
    composerKind,
    composerInitialFocus,
    sendBusy,
    submissionLocked,
    terminalSubmission,
    submissionEvidence,
    submissionUnresolved,
  } = composer;
  const {
    deactivate: deactivateSentDirectory,
    error: sentListError,
    headingRef: sentHeading,
    listBusy: sentListBusy,
    loadMore: loadMoreSentSubmissions,
    page: sentPage,
    refresh: refreshSentSubmissions,
  } = useSentDirectory({
    active: mailView === "sent",
    authenticated,
    canSend: activeMailbox?.can_send ?? false,
    mailboxSettings,
  });
  const {
    cancelRestoreConfirmation,
    confirmRestore,
    deactivate: deactivateDraftDirectory,
    draftButtonRef: registerDraftButton,
    headingRef: draftHeading,
    isRestoreInFlight: isDraftRestoreInFlight,
    listBusy: draftListBusy,
    listError: draftListError,
    loadMore: loadMoreDrafts,
    openDraft,
    page: draftPage,
    pendingRestore: pendingDraftRestore,
    refresh: refreshDrafts,
    restoreBusyId: draftRestoreBusyId,
    restoreError: draftRestoreError,
  } = useDraftDirectory({
    active: mailView === "drafts",
    authenticated,
    canSend: activeMailbox?.can_send ?? false,
    sessionLocked,
    mailboxSettings,
    hasComposerContent: composer.hasContent,
    hasUnresolvedSubmission: composer.hasUnresolvedSubmission,
    matchesRestoredDraft: composer.matchesRestoredDraft,
    reopenRestoredDraft: composer.reopenRestoredDraft,
    restoreIntoComposer: composer.restoreFromDraft,
  });
  const [mailboxDirectoryPage, setMailboxDirectoryPage] = useState<
    Pick<MailboxDirectoryPage, "next_cursor" | "has_more">
  >({ next_cursor: null, has_more: false });
  const [mailboxDirectoryLoading, setMailboxDirectoryLoading] = useState(false);
  const [mailboxLoadMoreBusy, setMailboxLoadMoreBusy] = useState(false);
  const [mailboxDirectoryError, setMailboxDirectoryError] = useState<UiErrorKey>();
  const [mailboxDirectoryPageError, setMailboxDirectoryPageError] = useState<UiErrorKey>();
  const [mailboxDirectoryRetry, setMailboxDirectoryRetry] = useState(0);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [pendingComposerTransition, setPendingComposerTransition] = useState<
    PendingComposerTransition
  >();
  const [page, setPage] = useState<InboxPage>();
  const [searchText, setSearchText] = useState("");
  const [activeSearch, setActiveSearch] = useState<string>();
  const [navDrawerOpen, setNavDrawerOpen] = useState(false);
  const [searchBusy, setSearchBusy] = useState(false);
  const [selected, setSelected] = useState<MessageSummary>();
  const [openedMessageId, setOpenedMessageId] = useState<string>();
  const [detail, setDetail] = useState<MessageDetail>();
  const [detailLoadError, setDetailLoadError] = useState<DetailLoadError>();
  const [detailActionError, setDetailActionError] = useState<DetailActionError>();
  const [detailRetryGeneration, setDetailRetryGeneration] = useState(0);
  const [error, setError] = useState<UiErrorKey>();
  const inboxRequestGeneration = useRef(0);
  const mailViewRef = useRef<MailView>("inbox");
  const mailboxDirectoryGeneration = useRef(0);
  const mailboxPageRequest = useRef<string | undefined>(undefined);
  const activeMailboxIdRef = useRef<string | undefined>(undefined);
  const detailRequests = useRef(new MessageDetailRequestGate());
  const pageRef = useRef<InboxPage | undefined>(undefined);
  const selectedMessageIdRef = useRef<string | undefined>(undefined);
  const openedMessageIdRef = useRef<string | undefined>(undefined);
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const inboxHeading = useRef<HTMLHeadingElement>(null);
  const inboxSearchInput = useRef<HTMLInputElement>(null);
  const commandPaletteReturnFocus = useRef<HTMLElement | null>(null);
  const menuActionReturnFocus = useRef<HTMLElement | null>(null);
  const messageButtons = useRef(new Map<string, HTMLButtonElement>());
  const selectedMessageId = selected?.message_id;
  pageRef.current = page;
  selectedMessageIdRef.current = selectedMessageId;
  openedMessageIdRef.current = openedMessageId;
  const openedMessage = openedMessageId === selectedMessageId ? selected : undefined;
  const draftRestoreInFlight = isDraftRestoreInFlight();
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
  const commitInboxPage = useCallback((nextPage: InboxPage) => {
    const previousPage = pageRef.current;
    const currentSelectedId = selectedMessageIdRef.current;
    const currentOpenedId = openedMessageIdRef.current;
    pageRef.current = nextPage;
    setPage(nextPage);
    if (!currentSelectedId) return;

    const rebound = nextPage.items.find(
      (message) => message.message_id === currentSelectedId,
    );
    if (rebound) {
      setSelected(rebound);
      setDetail((current) => current?.message.message_id === rebound.message_id
        ? { ...current, message: rebound }
        : current);
      return;
    }

    const previousIndex = previousPage?.items.findIndex(
      (message) => message.message_id === currentSelectedId,
    ) ?? -1;
    const fallbackIndex = Math.min(
      Math.max(previousIndex, 0),
      Math.max(nextPage.items.length - 1, 0),
    );
    const fallback = nextPage.items[fallbackIndex];
    selectedMessageIdRef.current = fallback?.message_id;
    openedMessageIdRef.current = undefined;
    detailRequests.current.invalidate();
    setSelected(fallback);
    setOpenedMessageId(undefined);
    setDetail(undefined);
    setDetailLoadError(undefined);
    setDetailActionError(undefined);
    if (currentOpenedId) {
      window.requestAnimationFrame(() => {
        (fallback
          ? messageButtons.current.get(fallback.message_id)
          : inboxHeading.current
        )?.focus();
      });
    }
  }, []);
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
    if (!authenticated || sessionLocked || !mailboxSettings || mailView !== "inbox") {
      inboxRequestGeneration.current += 1;
      return;
    }
    const generation = ++inboxRequestGeneration.current;
    listInbox(mailboxSettings)
      .then((nextPage) => {
        if (generation !== inboxRequestGeneration.current) return;
        commitInboxPage(nextPage);
        setError(undefined);
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
  }, [authenticated, commitInboxPage, mailboxSettings, mailView, sessionLocked]);
  useEffect(() => {
    setDetail(undefined);
    setDetailLoadError(undefined);
    if (!authenticated || sessionLocked || !mailboxSettings || !openedMessageId) {
      detailRequests.current.invalidate();
      return;
    }
    const token = detailRequests.current.begin(openedMessageId);
    if (!token) return;
    const controller = new AbortController();
    getMessage(mailboxSettings, openedMessageId, controller.signal)
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
  }, [
    authenticated,
    detailRetryGeneration,
    mailboxSettings,
    openedMessageId,
    sessionLocked,
  ]);
  const selectedDetail = detailMatchesSelection(detail, openedMessageId)
    ? detail
    : undefined;
  const applyState = (
    messageId: string,
    update: Partial<Pick<MessageSummary, "is_read" | "is_flagged">>,
  ) => {
    setPage((current) => {
      const next = current && ({
        ...current,
        items: current.items.map((item) => item.message_id === messageId
          ? { ...item, ...update }
          : item),
      });
      pageRef.current = next;
      return next;
    });
    setSelected((current) => current?.message_id === messageId
      ? { ...current, ...update }
      : current);
    setDetail((current) => current?.message.message_id === messageId
      ? { ...current, message: { ...current.message, ...update } }
      : current);
  };
  const openMessage = (message: MessageSummary) => {
    if (sessionLocked || !mailboxSettings) return;
    if (openedMessageId !== message.message_id) {
      setDetailLoadError(undefined);
      setDetailActionError(undefined);
      detailRequests.current.invalidate();
      setDetail(undefined);
    } else if (detailLoadError?.messageId === message.message_id) {
      setDetailLoadError(undefined);
      detailRequests.current.invalidate();
      setDetailRetryGeneration((generation) => generation + 1);
    }
    selectedMessageIdRef.current = message.message_id;
    openedMessageIdRef.current = message.message_id;
    setSelected(message);
    setOpenedMessageId(message.message_id);
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
    if (!authenticated || sessionLocked || !mailboxSettings) return;
    const generation = ++inboxRequestGeneration.current;
    setSearchBusy(true);
    setError(undefined);
    try {
      const nextPage = activeSearch
        ? await searchInbox(mailboxSettings, activeSearch)
        : await listInbox(mailboxSettings);
      if (generation === inboxRequestGeneration.current) {
        commitInboxPage(nextPage);
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
    openedMessageIdRef.current = undefined;
    selectedMessageIdRef.current = undefined;
    setOpenedMessageId(undefined);
    setSelected(undefined);
    setDetail(undefined);
    setDetailLoadError(undefined);
    setDetailActionError(undefined);
    if (restoreFocus && messageId) {
      window.requestAnimationFrame(() => messageButtons.current.get(messageId)?.focus());
    }
  };
  const closeMessageDetail = (restoreFocus = false) => {
    const messageId = selectedMessageId;
    detailRequests.current.invalidate();
    openedMessageIdRef.current = undefined;
    setOpenedMessageId(undefined);
    setDetail(undefined);
    setDetailLoadError(undefined);
    setDetailActionError(undefined);
    if (restoreFocus && messageId) {
      window.requestAnimationFrame(() => messageButtons.current.get(messageId)?.focus());
    }
  };
  const switchView = (nextView: MailView) => {
    if (sessionLocked) return;
    if (nextView === mailViewRef.current) return;
    if ((nextView === "drafts" || nextView === "sent") && !activeMailbox?.can_send) {
      return;
    }
    if (nextView !== "sent") {
      // Abort synchronously while the current sent pane is still mounted.
      // The following React state update may otherwise leave a short commit
      // window for a late sent-directory response.
      deactivateSentDirectory();
    }
    if (nextView !== "drafts") {
      // Same hard boundary for the recovery directory and its restore write.
      // Abort before publishing the next view to React.
      deactivateDraftDirectory();
    }
    mailViewRef.current = nextView;
    setMailView(nextView);
    clearMessageSelection();
    if (nextView === "inbox") {
      setSearchText("");
      setActiveSearch(undefined);
      setSearchBusy(false);
      setError(undefined);
    }
    if (nextView === "drafts" || nextView === "sent") {
      inboxRequestGeneration.current += 1;
    }
    if (narrowViewport()) {
      window.requestAnimationFrame(() => {
        (nextView === "drafts"
          ? draftHeading.current
          : nextView === "sent"
            ? sentHeading.current
            : inboxHeading.current
        )?.focus();
      });
    }
  };
  const activateMailbox = (mailbox: MailboxDirectoryItem) => {
    if (mailbox.mailbox_id === activeMailboxIdRef.current) return;
    // A mailbox identity change is a hard boundary for directory data. Abort
    // before publishing the new scope to React or any mailbox-local effects.
    deactivateSentDirectory();
    deactivateDraftDirectory();
    activeMailboxIdRef.current = mailbox.mailbox_id;
    mailViewRef.current = "inbox";
    setMailView("inbox");
    inboxRequestGeneration.current += 1;
    detailRequests.current.invalidate();
    pageRef.current = undefined;
    setPage(undefined);
    setSearchText("");
    setActiveSearch(undefined);
    setSearchBusy(false);
    setError(undefined);
    clearMessageSelection();
    composer.clear();
    setActiveMailboxId(mailbox.mailbox_id);
  };
  const switchMailbox = (mailboxId: string) => {
    if (sessionLocked) return;
    if (composer.hasUnresolvedSubmission()) {
      composer.openComposer();
      return;
    }
    const mailbox = mailboxes.find((item) => item.mailbox_id === mailboxId);
    if (!mailbox) return;
    if (composer.hasContent() && !terminalSubmission) {
      setPendingComposerTransition({ kind: "mailbox", mailboxId });
      return;
    }
    activateMailbox(mailbox);
  };
  const loadMoreMailboxes = async () => {
    if (
      !authenticated
      || sessionLocked
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
    if (!authenticated || sessionLocked || !mailboxSettings || searchBusy) return;
    const text = searchText.trim();
    if (text.length === 0) {
      const generation = ++inboxRequestGeneration.current;
      setActiveSearch(undefined);
      clearMessageSelection();
      setSearchBusy(true);
      try {
        const nextPage = await listInbox(mailboxSettings);
        if (generation === inboxRequestGeneration.current) {
          commitInboxPage(nextPage);
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
        commitInboxPage(result);
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
    if (!authenticated || sessionLocked || !mailboxSettings || searchBusy) return;
    const generation = ++inboxRequestGeneration.current;
    setSearchText("");
    setActiveSearch(undefined);
    clearMessageSelection();
    setSearchBusy(true);
    try {
      const nextPage = await listInbox(mailboxSettings);
      if (generation === inboxRequestGeneration.current) {
        commitInboxPage(nextPage);
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
    if (
      !authenticated
      || sessionLocked
      || !mailboxSettings
      || !page?.next_cursor
      || searchBusy
    ) return;
    const generation = ++inboxRequestGeneration.current;
    const cursor = page.next_cursor;
    const existingMessageIds = new Set(page.items.map((message) => message.message_id));
    setSearchBusy(true);
    try {
      const next = activeSearch
        ? await searchInbox(mailboxSettings, activeSearch, cursor)
        : await listInbox(mailboxSettings, cursor);
      if (next.items.some((message) => existingMessageIds.has(message.message_id))) {
        throw new Error("inbox_page_identity_overlap");
      }
      if (generation === inboxRequestGeneration.current) {
        setPage((current) => {
          if (!current || current.next_cursor !== cursor) return current;
          const accumulated = {
            ...next,
            // Keep read/star mutations that were confirmed while the page
            // request was in flight instead of restoring the request-start snapshot.
            items: [...current.items, ...next.items],
          };
          pageRef.current = accumulated;
          return accumulated;
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
  const requestFlagUpdate = (message: MessageSummary) => {
    if (sessionLocked || !authenticated || !mailboxSettings) return;
    const requestMailboxId = mailboxSettings.mailboxId;
    updateMessageState(mailboxSettings, message.message_id, { is_flagged: !message.is_flagged })
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
            messageId: message.message_id,
            operation: "flag",
            key: "error.updateStar",
          });
        }
      });
  };
  const toggleFlag = () => {
    if (!selected) return;
    requestFlagUpdate(selected);
  };
  const retryMessageDetail = () => {
    if (sessionLocked || !openedMessageId) return;
    setDetailLoadError(undefined);
    detailRequests.current.invalidate();
    setDetailRetryGeneration((generation) => generation + 1);
  };
  const openComposer = () => {
    if (sessionLocked || isDraftRestoreInFlight()) return;
    if (
      composerKind === "reply"
      && composer.hasContent()
      && !terminalSubmission
      && !submissionUnresolved
    ) {
      setPendingComposerTransition({ kind: "new-message" });
      return;
    }
    composer.openComposer();
  };
  const replyToSelected = () => {
    if (sessionLocked || !activeMailbox?.can_send || !selectedDetail?.reply_to_message_id) {
      return;
    }
    const replyAddress = selectedDetail.message.from[0]?.email ?? "";
    const reply: ComposerReply = {
      to: replyAddress,
      subject: /^re:/i.test(selectedDetail.message.subject)
        ? selectedDetail.message.subject
        : t("compose.replySubject", {
            subject: selectedDetail.message.subject || t("subject.none"),
          }),
      replyToMessageId: selectedDetail.reply_to_message_id,
    };
    if (composer.hasContent() && !terminalSubmission) {
      setPendingComposerTransition({ kind: "reply", reply });
      return;
    }
    composer.openReply(reply);
  };
  const requestSignOut = () => {
    if (sessionLocked) return;
    if (composer.hasContent() && (!terminalSubmission || submissionUnresolved)) {
      setPendingComposerTransition({ kind: "sign-out" });
      return;
    }
    onSignOut();
  };
  const openSettings = (trigger: HTMLElement | null) => {
    menuActionReturnFocus.current = trigger;
    setSettingsOpen(true);
  };
  const requestSignOutFromMenu = (trigger: HTMLElement | null) => {
    menuActionReturnFocus.current = trigger;
    requestSignOut();
  };
  const confirmComposerTransition = () => {
    if (sessionLocked) return;
    const transition = pendingComposerTransition;
    setPendingComposerTransition(undefined);
    if (!transition) return;
    switch (transition.kind) {
      case "mailbox": {
        const mailbox = mailboxes.find((item) => item.mailbox_id === transition.mailboxId);
        if (mailbox) activateMailbox(mailbox);
        return;
      }
      case "new-message":
        composer.openComposer();
        return;
      case "reply":
        composer.openReply(transition.reply);
        return;
      case "sign-out":
        onSignOut();
    }
  };
  const focusInboxSearch = () => {
    if (sessionLocked || !activeMailbox) return false;
    if (mailViewRef.current !== "inbox") switchView("inbox");
    window.requestAnimationFrame(() => inboxSearchInput.current?.focus());
    return true;
  };
  const moveMessageSelection = (direction: -1 | 1) => {
    if (sessionLocked || mailViewRef.current !== "inbox" || !page?.items.length) return false;
    const selectedIndex = selectedMessageId
      ? page.items.findIndex((item) => item.message_id === selectedMessageId)
      : direction === 1
        ? -1
        : page.items.length;
    const nextIndex = Math.min(
      page.items.length - 1,
      Math.max(0, selectedIndex + direction),
    );
    const nextMessage = page.items[nextIndex];
    if (!nextMessage || nextMessage.message_id === selectedMessageId) return false;
    closeMessageDetail();
    selectedMessageIdRef.current = nextMessage.message_id;
    setSelected(nextMessage);
    window.requestAnimationFrame(() => {
      messageButtons.current.get(nextMessage.message_id)?.focus();
    });
    return true;
  };
  const openSelectedMessage = () => {
    if (sessionLocked || mailViewRef.current !== "inbox" || !selected) return false;
    openMessage(selected);
    return true;
  };
  const closeSelectedMessage = () => {
    if (sessionLocked || mailViewRef.current !== "inbox" || !openedMessageId) return false;
    closeMessageDetail(true);
    return true;
  };
  const replyFromKeyboard = () => {
    if (
      mailViewRef.current !== "inbox"
      || sessionLocked
      || !activeMailbox?.can_send
      || submissionUnresolved
      || !selectedDetail?.reply_to_message_id
    ) return false;
    replyToSelected();
    return true;
  };
  const toggleFlagFromKeyboard = () => {
    if (
      sessionLocked
      || mailViewRef.current !== "inbox"
      || !selected
      || !authenticated
      || !mailboxSettings
    ) {
      return false;
    }
    toggleFlag();
    return true;
  };
  const composeFromKeyboard = () => {
    if (
      !activeMailbox?.can_send
      || sessionLocked
      || draftRestoreBusyId !== undefined
      || isDraftRestoreInFlight()
    ) return false;
    openComposer();
    return true;
  };
  const switchViewFromKeyboard = (nextView: MailView) => {
    if (sessionLocked) return false;
    if (mailViewRef.current === nextView) return false;
    if ((nextView === "drafts" || nextView === "sent") && !activeMailbox?.can_send) {
      return false;
    }
    switchView(nextView);
    return true;
  };
  const refreshCurrentView = () => {
    if (!authenticated || sessionLocked || !mailboxSettings) return false;
    if (mailViewRef.current === "inbox") {
      if (searchBusy) return false;
      void refreshMessages();
      return true;
    }
    if (mailViewRef.current === "drafts") {
      if (
        !activeMailbox?.can_send
        || draftListBusy
        || draftRestoreBusyId !== undefined
        || isDraftRestoreInFlight()
      ) return false;
      refreshDrafts();
      return true;
    }
    if (!activeMailbox?.can_send || sentListBusy) return false;
    refreshSentSubmissions();
    return true;
  };
  const openCommandPalette = (returnFocusTo?: HTMLElement | null) => {
    if (
      !authenticated
      || sessionLocked
      || !activeMailbox
      || composeOpen
      || pendingDraftRestore !== undefined
    ) {
      return false;
    }
    commandPaletteReturnFocus.current = returnFocusTo ?? (
      document.activeElement instanceof HTMLElement ? document.activeElement : null
    );
    setCommandPaletteOpen(true);
    return true;
  };
  const composeUnavailableReason = !activeMailbox?.can_send
    ? t("command.disabledReadOnly")
    : draftRestoreBusyId !== undefined || draftRestoreInFlight
      ? t("command.disabledDraftRestore")
      : undefined;
  const commandPaletteCommands = useMemo<CommandPaletteCommand[]>(() => {
    const commands: CommandPaletteCommand[] = [{
      id: "compose",
      label: t(submissionUnresolved ? "command.reopenSubmission" : "command.compose"),
      description: t(submissionUnresolved
        ? "command.reopenSubmissionDescription"
        : "command.composeDescription"),
      icon: "compose",
      shortcut: "C",
      disabled: composeUnavailableReason !== undefined,
      disabledReason: composeUnavailableReason,
    }, {
      id: "focus-search",
      label: t("command.focusSearch"),
      description: t("command.focusSearchDescription"),
      icon: "search",
      shortcut: "/",
    }, {
      id: "refresh-current",
      label: t("command.refresh"),
      description: t("command.refreshDescription"),
      icon: "refresh",
      disabled: mailView === "inbox"
        ? searchBusy
        : mailView === "drafts"
          ? draftListBusy || draftRestoreBusyId !== undefined || draftRestoreInFlight
          : sentListBusy,
      disabledReason: mailView === "inbox"
        ? (searchBusy ? t("command.disabledRefreshing") : undefined)
        : mailView === "drafts"
          ? (draftRestoreBusyId !== undefined || draftRestoreInFlight
            ? t("command.disabledDraftRestore")
            : draftListBusy ? t("command.disabledRefreshing") : undefined)
          : sentListBusy ? t("command.disabledRefreshing") : undefined,
    }];

    if (mailView !== "inbox") {
      commands.push({
        id: "view-inbox",
        label: t("command.inbox"),
        description: t("command.inboxDescription"),
        icon: "mail",
        shortcut: "G I",
      });
    }
    if (mailView !== "drafts") {
      commands.push({
        id: "view-drafts",
        label: t("command.drafts"),
        description: t("command.draftsDescription"),
        icon: "compose",
        shortcut: "G D",
        disabled: !activeMailbox?.can_send,
        disabledReason: !activeMailbox?.can_send ? t("command.disabledReadOnly") : undefined,
      });
    }
    if (mailView !== "sent") {
      commands.push({
        id: "view-sent",
        label: t("command.sent"),
        description: t("command.sentDescription"),
        icon: "send",
        shortcut: "G S",
        disabled: !activeMailbox?.can_send,
        disabledReason: !activeMailbox?.can_send ? t("command.disabledReadOnly") : undefined,
      });
    }
    if (mailView === "inbox" && selectedDetail) {
      const replyUnavailableReason = !activeMailbox?.can_send
        ? t("command.disabledReadOnly")
        : submissionUnresolved
          ? t("command.disabledSubmissionLocked")
          : undefined;
      if (selectedDetail.reply_to_message_id) {
        commands.push({
          id: "reply",
          label: t("command.reply"),
          description: t("command.replyDescription"),
          icon: "reply",
          shortcut: "R",
          disabled: replyUnavailableReason !== undefined,
          disabledReason: replyUnavailableReason,
        });
      }
      commands.push({
        id: "toggle-star",
        label: selectedDetail.message.is_flagged
          ? t("command.removeStar")
          : t("command.addStar"),
        description: t("command.starDescription"),
        icon: selectedDetail.message.is_flagged ? "star-filled" : "star",
        shortcut: "S",
      });
    }
    return commands;
  }, [
    activeMailbox?.can_send,
    composeUnavailableReason,
    draftListBusy,
    draftRestoreBusyId,
    draftRestoreInFlight,
    mailView,
    searchBusy,
    selected,
    selectedDetail,
    sentListBusy,
    submissionUnresolved,
    t,
  ]);
  const runCommandPaletteCommand = (
    command: CommandPaletteCommand,
  ): CommandPaletteFocusPolicy => {
    setCommandPaletteOpen(false);
    switch (command.id) {
      case "compose":
        return composeFromKeyboard() ? "transfer" : "restore";
      case "focus-search":
        return focusInboxSearch() ? "transfer" : "restore";
      case "refresh-current":
        refreshCurrentView();
        return "restore";
      case "view-inbox":
        switchViewFromKeyboard("inbox");
        return "restore";
      case "view-drafts":
        switchViewFromKeyboard("drafts");
        return "restore";
      case "view-sent":
        switchViewFromKeyboard("sent");
        return "restore";
      case "reply":
        return replyFromKeyboard() ? "transfer" : "restore";
      case "toggle-star":
        toggleFlagFromKeyboard();
        return "restore";
      default:
        return "restore";
    }
  };
  useMailKeyboardShortcuts({
    enabled: authenticated && !sessionLocked && activeMailbox !== undefined,
    modalOpen: commandPaletteOpen
      || composeOpen
      || settingsOpen
      || pendingDraftRestore !== undefined
      || pendingComposerTransition !== undefined,
    onOpenCommandPalette: openCommandPalette,
    onCompose: composeFromKeyboard,
    onFocusInboxSearch: focusInboxSearch,
    onMoveMessage: moveMessageSelection,
    onOpenSelectedMessage: openSelectedMessage,
    onCloseSelectedMessage: closeSelectedMessage,
    onReply: replyFromKeyboard,
    onToggleFlag: toggleFlagFromKeyboard,
    onViewDrafts: () => switchViewFromKeyboard("drafts"),
    onViewInbox: () => switchViewFromKeyboard("inbox"),
    onViewSent: () => switchViewFromKeyboard("sent"),
  });
  if (!activeMailbox) {
    const directoryMessage = mailboxDirectoryError
      ?? (mailboxDirectoryLoading
        ? "mailbox.directoryLoading"
        : mailboxDirectoryPage.has_more
          ? "mailbox.directoryNeedsMore"
          : "mailbox.directoryLoading");
    const directoryActions = [
      ...(mailboxDirectoryPage.has_more && mailboxDirectoryPage.next_cursor
        ? [{
            label: mailboxLoadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore"),
            disabled: mailboxLoadMoreBusy,
            onPress: () => { void loadMoreMailboxes(); },
          }]
        : []),
      ...(mailboxDirectoryError === "error.mailboxDirectoryUnavailable"
        ? [{
            label: t("mailbox.retryDirectory"),
            onPress: () => setMailboxDirectoryRetry((value) => value + 1),
          }]
        : []),
      {
        appearance: "quiet" as const,
        label: t("nav.signOut"),
        onPress: onSignOut,
      },
    ];
    return (
      <MailAccessSurface
        actions={directoryActions}
        kind="directory"
        locked={sessionLocked}
        notice={{
          text: t(mailboxDirectoryPageError ?? directoryMessage),
          role: sessionLocked
            ? undefined
            : mailboxDirectoryError || mailboxDirectoryPageError
              ? "alert"
              : "status",
        }}
        title={t("mailbox.directoryTitle")}
      />
    );
  }
  const unreadInboxCount = !activeSearch && page && !page.has_more
    ? page.items.filter((item) => !item.is_read).length
    : undefined;
  const navigationProps = {
    activeMailbox,
    activeView: mailView,
    canCompose: activeMailbox.can_send,
    composeBusy: draftRestoreBusyId !== undefined,
    sessionLocked: submissionUnresolved,
    locale,
    mailboxes,
    mailboxHasMore: mailboxDirectoryPage.has_more,
    mailboxLoadMoreBusy,
    mailboxLoadMoreError: mailboxDirectoryPageError
      ? t(mailboxDirectoryPageError)
      : undefined,
    unreadInboxCount,
    onMailboxChange: switchMailbox,
    onMailboxLoadMore: () => { void loadMoreMailboxes(); },
    onCompose: openComposer,
    onOpenCommandPalette: (trigger: HTMLElement) => { openCommandPalette(trigger); },
    onOpenSettings: openSettings,
    onSignOut: requestSignOutFromMenu,
    onViewChange: switchView,
    t,
  };
  return (
    <MailWorkspaceShell
      messageOpen={mailView === "inbox" && Boolean(openedMessage)}
      directoryView={mailView !== "inbox"}
      sessionLocked={sessionLocked}
      topbar={(
        <MailTopbar
        activeSearch={activeSearch}
        navigationOpen={navDrawerOpen}
        searchBusy={searchBusy}
        searchInputRef={inboxSearchInput}
        searchText={searchText}
        t={t}
        onClearSearch={() => void clearSearch()}
        onOpenMenu={() => setNavDrawerOpen(true)}
        onSearch={() => void submitSearch()}
        onSearchTextChange={setSearchText}
        />
      )}
      navigation={(
        <>
          <DesktopNavigation {...navigationProps} />
          <MobileNavigation
            {...navigationProps}
            drawerOpen={navDrawerOpen}
            onDrawerClose={() => setNavDrawerOpen(false)}
          />
        </>
      )}
      content={mailView === "inbox" ? (
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
            onLoadMore={() => void loadMore()}
            onOpenMessage={openMessage}
            onRefresh={() => void refreshMessages()}
            onSearch={() => void submitSearch()}
            onToggleFlag={requestFlagUpdate}
            messageButtonRef={registerMessageButton}
          />
          <MessageDetailView
            actionError={detailActionError && detailActionError.messageId === openedMessageId
              ? t(detailActionError.key)
              : undefined}
            detail={selectedDetail}
            headingRef={detailHeading}
            loadError={detailLoadError && detailLoadError.messageId === openedMessageId
              ? t(detailLoadError.key)
              : undefined}
            locale={locale}
            canReply={activeMailbox.can_send && !submissionUnresolved}
            selected={openedMessage}
            t={t}
            onBack={() => closeMessageDetail(true)}
            onReply={replyToSelected}
            onRetry={retryMessageDetail}
            onToggleFlag={toggleFlag}
          />
        </>
      ) : mailView === "drafts" ? (
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
      ) : (
        <SentList
          error={sentListError ? t(sentListError) : undefined}
          headingRef={sentHeading}
          listBusy={sentListBusy}
          locale={locale}
          page={sentPage}
          t={t}
          onLoadMore={() => void loadMoreSentSubmissions()}
          onRefresh={refreshSentSubmissions}
        />
      )}
      overlays={(
        <>
          {composeOpen && !sessionLocked && (
            <ComposerDialog
          mailbox={activeMailbox}
          t={t}
          locale={locale}
          replyMode={Boolean(replyToMessageId)}
          restoredMode={composerKind === "restored"}
          initialFocus={composerInitialFocus}
          to={to}
          cc={cc}
          bcc={bcc}
          subject={subject}
          textBody={textBody}
          draftStatus={statusText(t, draftStatus)}
          draftStatusTone={composerStatusTone(draftStatus)}
          submissionEvidence={submissionEvidence}
          submissionLocked={submissionLocked}
          sendBusy={sendBusy}
          terminalSubmission={terminalSubmission}
          onToChange={composer.changeTo}
          onCcChange={composer.changeCc}
          onBccChange={composer.changeBcc}
          onSubjectChange={composer.changeSubject}
          onTextBodyChange={composer.changeTextBody}
          onClose={composer.close}
          onSend={composer.send}
            />
          )}
          {pendingDraftRestore && !composeOpen && !sessionLocked && (
            <DraftRestoreConfirmDialog
          busy={draftRestoreBusyId === pendingDraftRestore.draft_id}
          t={t}
          onCancel={cancelRestoreConfirmation}
          onConfirm={confirmRestore}
            />
          )}
          {pendingComposerTransition && !composeOpen && !sessionLocked && (
            <ConfirmDialog
          cancelLabel={t("compose.transitionCancel")}
          closeLabel={t("compose.transitionCloseAria")}
          confirmLabel={t("compose.transitionConfirm")}
          description={t("compose.transitionDescription")}
          returnFocusRef={menuActionReturnFocus}
          role="alertdialog"
          showCloseButton={false}
          title={t("compose.transitionTitle")}
          onCancel={() => {
            setPendingComposerTransition(undefined);
            composer.reopenComposer();
          }}
          onConfirm={confirmComposerTransition}
            />
          )}
          {settingsOpen && !sessionLocked ? (
            <SettingsDialog
          locale={locale}
          returnFocusRef={menuActionReturnFocus}
          t={t}
          onClose={() => setSettingsOpen(false)}
          onLocaleChange={onLocaleChange}
            />
          ) : null}
          <CommandPalette
        closeHint={t("command.closeHint")}
        commands={commandPaletteCommands}
        dismissLabel={t("command.dismissAria")}
        emptyQueryLabel={t("command.emptyQuery")}
        emptyState={t("command.emptyState")}
        noResults={t("command.noResults")}
        open={commandPaletteOpen && !sessionLocked}
        returnFocusTo={commandPaletteReturnFocus.current}
        searchLabel={t("command.searchLabel")}
        searchPlaceholder={t("command.searchPlaceholder")}
        title={t("command.title")}
        onCommand={runCommandPaletteCommand}
        onDismiss={() => setCommandPaletteOpen(false)}
          />
        </>
      )}
    />
  );
}

export default function App() {
  const settings = useMemo(readRuntimeConfig, []);
  const [locale, setLocale] = useState<Locale>(() => detectLocale());
  const t = useMemo(() => createTranslator(locale), [locale]);
  const [auth, setAuth] = useState<AuthState>({ kind: "checking" });
  const [sessionRefreshing, setSessionRefreshing] = useState(false);
  const [sessionVerificationFailed, setSessionVerificationFailed] = useState(false);
  const sessionSequence = useRef(0);
  const sessionRefresh = useRef<Promise<void> | undefined>(undefined);
  const authenticatedWorkspace = useRef(false);
  const verificationFailed = useRef(false);
  const sessionFailureNotice = useRef<HTMLElement>(null);

  const bindVerificationFailure = useCallback((failed: boolean) => {
    verificationFailed.current = failed;
    setSessionVerificationFailed(failed);
  }, []);

  const revalidateSession = useCallback((): Promise<void> => {
    if (sessionRefresh.current) return sessionRefresh.current;
    const sequence = sessionSequence.current + 1;
    sessionSequence.current = sequence;
    bindVerificationFailure(false);
    setSessionRefreshing(true);
    const pending = loadSession(settings)
      .then((session) => {
        if (sequence !== sessionSequence.current) return;
        authenticatedWorkspace.current = session !== null;
        setAuth(session ? {
          kind: "authenticated",
          sessionContextKey: JSON.stringify([
            session.principal_id,
            session.organization_id,
            session.client,
          ]),
          expiresAt: Date.parse(session.expires_at),
        } : { kind: "signed-out" });
      })
      .catch(() => {
        if (sequence === sessionSequence.current) {
          if (authenticatedWorkspace.current) {
            bindVerificationFailure(true);
          } else {
            setAuth({ kind: "error", messageKey: "error.sessionUnavailable" });
          }
        }
      })
      .finally(() => {
        if (sessionRefresh.current === pending) sessionRefresh.current = undefined;
        if (sequence === sessionSequence.current) setSessionRefreshing(false);
      });
    sessionRefresh.current = pending;
    return pending;
  }, [bindVerificationFailure, settings]);

  useEffect(() => {
    document.documentElement.lang = locale;
    persistLocale(locale);
  }, [locale]);
  useEffect(() => {
    void revalidateSession();
  }, [revalidateSession]);
  useEffect(() => {
    if (auth.kind !== "authenticated") return;
    if (sessionVerificationFailed) return;
    const onFocus = () => void revalidateSession();
    window.addEventListener("focus", onFocus);
    const remaining = Math.max(0, auth.expiresAt - Date.now());
    const timeout = window.setTimeout(
      () => void revalidateSession(),
      Math.min(remaining, 2_147_000_000),
    );
    return () => {
      window.removeEventListener("focus", onFocus);
      window.clearTimeout(timeout);
    };
  }, [auth, revalidateSession, sessionVerificationFailed]);
  useEffect(() => {
    if (sessionVerificationFailed) sessionFailureNotice.current?.focus();
  }, [sessionVerificationFailed]);

  const signOut = () => {
    sessionSequence.current += 1;
    sessionRefresh.current = undefined;
    authenticatedWorkspace.current = false;
    bindVerificationFailure(false);
    setSessionRefreshing(false);
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
  const handleUnauthorized = useCallback(() => {
    if (verificationFailed.current) return;
    void revalidateSession();
  }, [revalidateSession]);

  if (auth.kind === "checking") {
    return (
      <MailAccessSurface
        kind="login"
        notice={{ text: t("auth.checking"), role: "status" }}
        title={t("app.name")}
      />
    );
  }
  if (auth.kind !== "authenticated") {
    return (
      <MailAccessSurface
        actions={[{ label: t("auth.signIn"), onPress: signIn }]}
        description={t("auth.tagline")}
        kind="login"
        notice={auth.kind === "error"
          ? { text: t(auth.messageKey), role: "alert" }
          : undefined}
        title={t("app.name")}
      />
    );
  }

  const sessionLocked = sessionRefreshing || sessionVerificationFailed;
  return (
    <>
      <AuthenticatedWorkspace
        key={auth.sessionContextKey}
        locale={locale}
        onLocaleChange={setLocale}
        onSignOut={signOut}
        onUnauthorized={handleUnauthorized}
        runtimeSettings={settings}
        sessionLocked={sessionLocked}
        t={t}
      />
      {sessionRefreshing && !sessionVerificationFailed ? (
        <SessionVerificationNotice
          message={t("auth.checking")}
          state="checking"
        />
      ) : null}
      {sessionVerificationFailed ? (
        <SessionVerificationNotice
          ref={sessionFailureNotice}
          message={t("error.sessionUnavailable")}
          retryLabel={t("common.retry")}
          signOutLabel={t("nav.signOut")}
          state="failed"
          onRetry={() => { void revalidateSession(); }}
          onSignOut={signOut}
        />
      ) : null}
    </>
  );
}
