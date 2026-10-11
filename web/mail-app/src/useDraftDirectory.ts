import { useCallback, useEffect, useRef, useState } from "react";
import { listDrafts, restoreDraft } from "./api";
import type {
  HumanDraftListItem,
  HumanDraftPage,
  HumanDraftRestore,
  MailboxScopedConfig,
} from "./types";

/**
 * UI-safe error keys deliberately kept at the draft-directory boundary. The
 * hook never exposes transport errors or server response details to callers.
 */
export type DraftDirectoryError =
  | "error.draftsUnavailable"
  | "error.draftsPageUnavailable"
  | "error.draftRestoreUnavailable"
  | "error.draftRestoreConflict"
  | "error.draftSubmissionLocked";

export interface UseDraftDirectoryOptions {
  /** Whether the current mailbox view is the draft directory. */
  readonly active: boolean;
  readonly authenticated: boolean;
  readonly canSend: boolean;
  readonly sessionLocked?: boolean;
  readonly mailboxSettings?: MailboxScopedConfig;
  /** Narrow composer queries/actions keep this directory mailbox-scoped. */
  readonly hasComposerContent: () => boolean;
  readonly hasUnresolvedSubmission: () => boolean;
  readonly matchesRestoredDraft: (draft: HumanDraftListItem) => boolean;
  readonly reopenRestoredDraft: () => void;
  readonly restoreIntoComposer: (draft: HumanDraftRestore) => void;
}

interface DirectoryScope {
  readonly active: boolean;
  readonly authenticated: boolean;
  readonly canSend: boolean;
  readonly sessionLocked: boolean;
  readonly mailboxSettings?: MailboxScopedConfig;
}

interface RestoreAdapter {
  readonly hasComposerContent: () => boolean;
  readonly hasUnresolvedSubmission: () => boolean;
  readonly matchesRestoredDraft: (draft: HumanDraftListItem) => boolean;
  readonly reopenRestoredDraft: () => void;
  readonly restoreIntoComposer: (draft: HumanDraftRestore) => void;
}

function canReadDraftDirectory(scope: DirectoryScope): scope is DirectoryScope & {
  readonly mailboxSettings: MailboxScopedConfig;
} {
  return scope.active
    && scope.authenticated
    && scope.canSend
    && !scope.sessionLocked
    && scope.mailboxSettings !== undefined;
}

/**
 * Draft pages are ordered by draft id descending. A continuation must start
 * strictly after the accumulated tail and must never repeat a draft identity.
 */
export function mergeDraftDirectoryPages(
  current: readonly HumanDraftListItem[],
  next: readonly HumanDraftListItem[],
): HumanDraftListItem[] {
  const currentTail = current.at(-1)?.draft_id;
  if (currentTail && next[0] && next[0].draft_id >= currentTail) {
    throw new Error("draft_list_order_conflict");
  }
  const identities = new Set(current.map((draft) => draft.draft_id));
  for (const draft of next) {
    if (identities.has(draft.draft_id)) {
      throw new Error("draft_list_identity_conflict");
    }
    identities.add(draft.draft_id);
  }
  return [...current, ...next];
}

/**
 * Owns the cancellable, cursor-safe draft directory plus the recovery-only
 * restore protocol. `deactivate()` is public so App can synchronously abort
 * both list and restore work before it publishes a new mailbox or view.
 */
export function useDraftDirectory({
  active,
  authenticated,
  canSend,
  sessionLocked = false,
  mailboxSettings,
  hasComposerContent,
  hasUnresolvedSubmission,
  matchesRestoredDraft,
  reopenRestoredDraft,
  restoreIntoComposer,
}: UseDraftDirectoryOptions) {
  const [page, setPageState] = useState<HumanDraftPage>();
  const [listBusy, setListBusyState] = useState(false);
  const [listError, setListErrorState] = useState<DraftDirectoryError>();
  const [restoreBusyId, setRestoreBusyId] = useState<string>();
  const [restoreError, setRestoreError] = useState<DraftDirectoryError>();
  const [pendingRestore, setPendingRestoreState] = useState<HumanDraftListItem>();
  const [refreshGeneration, setRefreshGeneration] = useState(0);

  const pageRef = useRef<HumanDraftPage | undefined>(undefined);
  const listBusyRef = useRef(false);
  const scopeRef = useRef<DirectoryScope>({
    active,
    authenticated,
    canSend,
    sessionLocked,
    mailboxSettings,
  });
  const restoreAdapterRef = useRef<RestoreAdapter>({
    hasComposerContent,
    hasUnresolvedSubmission,
    matchesRestoredDraft,
    reopenRestoredDraft,
    restoreIntoComposer,
  });
  const listGeneration = useRef(0);
  const listAbort = useRef<AbortController | undefined>(undefined);
  const pageRequest = useRef<{
    cursor: string;
    controller: AbortController;
  } | undefined>(undefined);
  const restoreGeneration = useRef(0);
  const restoreAbort = useRef<AbortController | undefined>(undefined);
  const restoreInFlight = useRef<string | undefined>(undefined);
  const pendingRestoreRef = useRef<HumanDraftListItem | undefined>(undefined);
  const focusAfterRefresh = useRef<string | undefined>(undefined);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const draftButtons = useRef(new Map<string, HTMLButtonElement>());

  // These refs intentionally update during render. Event callbacks remain
  // stable while always observing the latest mailbox scope and composer state.
  scopeRef.current = {
    active,
    authenticated,
    canSend,
    sessionLocked,
    mailboxSettings,
  };
  restoreAdapterRef.current = {
    hasComposerContent,
    hasUnresolvedSubmission,
    matchesRestoredDraft,
    reopenRestoredDraft,
    restoreIntoComposer,
  };

  const replacePage = useCallback((next?: HumanDraftPage) => {
    pageRef.current = next;
    setPageState(next);
  }, []);
  const setListBusy = useCallback((next: boolean) => {
    listBusyRef.current = next;
    setListBusyState(next);
  }, []);
  const setPendingRestore = useCallback((next?: HumanDraftListItem) => {
    pendingRestoreRef.current = next;
    setPendingRestoreState(next);
  }, []);

  const invalidateListRequest = useCallback(() => {
    listGeneration.current += 1;
    listAbort.current?.abort();
    listAbort.current = undefined;
    pageRequest.current = undefined;
  }, []);
  const invalidateRestoreRequest = useCallback(() => {
    restoreGeneration.current += 1;
    restoreAbort.current?.abort();
    restoreAbort.current = undefined;
    restoreInFlight.current = undefined;
  }, []);

  const listRequestIsCurrent = useCallback((
    generation: number,
    controller: AbortController,
    mailboxId: string,
  ) => {
    const scope = scopeRef.current;
    return generation === listGeneration.current
      && !controller.signal.aborted
      && canReadDraftDirectory(scope)
      && scope.mailboxSettings.mailboxId === mailboxId;
  }, []);
  const restoreRequestIsCurrent = useCallback((
    generation: number,
    controller: AbortController,
    mailboxId: string,
  ) => {
    const scope = scopeRef.current;
    return generation === restoreGeneration.current
      && !controller.signal.aborted
      && canReadDraftDirectory(scope)
      && scope.mailboxSettings.mailboxId === mailboxId;
  }, []);

  const draftButtonRef = useCallback((draftId: string) => (node: HTMLButtonElement | null) => {
    if (node) draftButtons.current.set(draftId, node);
    else draftButtons.current.delete(draftId);
  }, []);

  const deactivate = useCallback(() => {
    invalidateListRequest();
    invalidateRestoreRequest();
    focusAfterRefresh.current = undefined;
    replacePage(undefined);
    setListBusy(false);
    setListErrorState(undefined);
    setRestoreBusyId(undefined);
    setRestoreError(undefined);
    setPendingRestore(undefined);
  }, [
    invalidateListRequest,
    invalidateRestoreRequest,
    replacePage,
    setListBusy,
    setPendingRestore,
  ]);

  useEffect(() => {
    invalidateListRequest();
    if (sessionLocked) {
      setListBusy(false);
      return;
    }
    replacePage(undefined);
    setListErrorState(undefined);
    setListBusy(false);

    const scope = scopeRef.current;
    if (!canReadDraftDirectory(scope)) {
      if (!scope.active) focusAfterRefresh.current = undefined;
      return;
    }

    const requestMailboxId = scope.mailboxSettings.mailboxId;
    const requestSettings = scope.mailboxSettings;
    const controller = new AbortController();
    const generation = ++listGeneration.current;
    listAbort.current = controller;
    setListBusy(true);

    void listDrafts(requestSettings, undefined, controller.signal)
      .then((nextPage) => {
        if (!listRequestIsCurrent(generation, controller, requestMailboxId)) return;
        replacePage(nextPage);
        const focusDraftId = focusAfterRefresh.current;
        if (!focusDraftId) return;
        focusAfterRefresh.current = undefined;
        window.requestAnimationFrame(() => {
          if (!listRequestIsCurrent(generation, controller, requestMailboxId)) return;
          (draftButtons.current.get(focusDraftId) ?? headingRef.current)?.focus();
        });
      })
      .catch(() => {
        if (!listRequestIsCurrent(generation, controller, requestMailboxId)) return;
        setListErrorState("error.draftsUnavailable");
      })
      .finally(() => {
        if (listRequestIsCurrent(generation, controller, requestMailboxId)) {
          setListBusy(false);
        }
      });

    // A continuation can replace the initial controller. Go through the
    // shared invalidator so a scope change also stops that newer request.
    return () => invalidateListRequest();
  }, [
    active,
    authenticated,
    canSend,
    invalidateListRequest,
    listRequestIsCurrent,
    mailboxSettings,
    refreshGeneration,
    replacePage,
    sessionLocked,
    setListBusy,
  ]);

  // A Session lock cancels a restore that has not completed, but keeps the
  // user's pending confirmation and directory memory available for an
  // explicit retry after verification succeeds.
  useEffect(() => {
    if (!sessionLocked) return;
    invalidateRestoreRequest();
    setRestoreBusyId(undefined);
  }, [invalidateRestoreRequest, sessionLocked]);

  // `deactivate()` is the synchronous path App uses before a scope change.
  // This effect is the defensive path for unmounts or future callers that
  // update the scope without first invoking that public boundary method.
  useEffect(() => {
    invalidateRestoreRequest();
    setRestoreBusyId(undefined);
    setPendingRestore(undefined);
    if (!canReadDraftDirectory(scopeRef.current)) {
      focusAfterRefresh.current = undefined;
      setRestoreError(undefined);
    }
  }, [
    active,
    authenticated,
    canSend,
    invalidateRestoreRequest,
    mailboxSettings,
    setPendingRestore,
  ]);

  useEffect(() => () => {
    invalidateListRequest();
    invalidateRestoreRequest();
  }, [invalidateListRequest, invalidateRestoreRequest]);

  const isRestoreInFlight = useCallback(
    () => restoreInFlight.current !== undefined,
    [],
  );

  const refresh = useCallback(() => {
    if (
      !canReadDraftDirectory(scopeRef.current)
      || restoreInFlight.current !== undefined
    ) return false;
    setRestoreError(undefined);
    // Abort now, not in the next effect, so a stale page cannot win the
    // interval between a recovery action and React's next commit.
    invalidateListRequest();
    setRefreshGeneration((generation) => generation + 1);
    return true;
  }, [invalidateListRequest]);

  const loadMore = useCallback(async () => {
    const scope = scopeRef.current;
    const currentPage = pageRef.current;
    if (
      !canReadDraftDirectory(scope)
      || !currentPage?.has_more
      || !currentPage.next_cursor
      || listBusyRef.current
      || restoreInFlight.current !== undefined
      || pageRequest.current !== undefined
    ) return false;

    const generation = listGeneration.current;
    const requestMailboxId = scope.mailboxSettings.mailboxId;
    const requestSettings = scope.mailboxSettings;
    const cursor = currentPage.next_cursor;
    const controller = new AbortController();
    listAbort.current?.abort();
    listAbort.current = controller;
    pageRequest.current = { cursor, controller };
    setListBusy(true);
    setListErrorState(undefined);

    try {
      const nextPage = await listDrafts(requestSettings, cursor, controller.signal);
      if (!listRequestIsCurrent(generation, controller, requestMailboxId)) return false;
      if (nextPage.next_cursor !== null && nextPage.next_cursor >= cursor) {
        throw new Error("draft_list_cursor_did_not_advance");
      }
      const accumulatedPage = pageRef.current;
      if (!accumulatedPage) return false;
      replacePage({
        ...nextPage,
        items: mergeDraftDirectoryPages(accumulatedPage.items, nextPage.items),
      });
      return true;
    } catch {
      if (listRequestIsCurrent(generation, controller, requestMailboxId)) {
        setListErrorState("error.draftsPageUnavailable");
      }
      return false;
    } finally {
      if (pageRequest.current?.controller === controller) {
        pageRequest.current = undefined;
      }
      if (listRequestIsCurrent(generation, controller, requestMailboxId)) {
        setListBusy(false);
      }
    }
  }, [listRequestIsCurrent, replacePage, setListBusy]);

  const performRestore = useCallback(async (draft: HumanDraftListItem) => {
    const scope = scopeRef.current;
    if (
      !canReadDraftDirectory(scope)
      || restoreInFlight.current !== undefined
      || restoreAdapterRef.current.hasUnresolvedSubmission()
    ) return false;

    const generation = ++restoreGeneration.current;
    const requestMailboxId = scope.mailboxSettings.mailboxId;
    const requestSettings = scope.mailboxSettings;
    const controller = new AbortController();
    restoreAbort.current?.abort();
    restoreAbort.current = controller;
    restoreInFlight.current = draft.draft_id;
    setRestoreBusyId(draft.draft_id);
    setRestoreError(undefined);

    try {
      const restored = await restoreDraft(requestSettings, draft, controller.signal);
      if (!restoreRequestIsCurrent(generation, controller, requestMailboxId)) return false;
      restoreAdapterRef.current.restoreIntoComposer(restored);
      setPendingRestore(undefined);
      return true;
    } catch (cause) {
      if (!restoreRequestIsCurrent(generation, controller, requestMailboxId)) return false;
      if (cause instanceof Error && cause.message === "draft_restore_409") {
        focusAfterRefresh.current = draft.draft_id;
        setRestoreError("error.draftRestoreConflict");
        invalidateListRequest();
        setRefreshGeneration((current) => current + 1);
      } else {
        setRestoreError("error.draftRestoreUnavailable");
        window.requestAnimationFrame(() => {
          if (restoreRequestIsCurrent(generation, controller, requestMailboxId)) {
            draftButtons.current.get(draft.draft_id)?.focus();
          }
        });
      }
      setPendingRestore(undefined);
      return false;
    } finally {
      if (restoreRequestIsCurrent(generation, controller, requestMailboxId)) {
        restoreInFlight.current = undefined;
        setRestoreBusyId(undefined);
      }
    }
  }, [
    invalidateListRequest,
    restoreRequestIsCurrent,
    setPendingRestore,
  ]);

  const openDraft = useCallback((draft: HumanDraftListItem) => {
    if (!canReadDraftDirectory(scopeRef.current)) return false;
    const adapter = restoreAdapterRef.current;
    if (adapter.hasUnresolvedSubmission()) {
      setRestoreError("error.draftSubmissionLocked");
      return false;
    }
    if (adapter.matchesRestoredDraft(draft)) {
      adapter.reopenRestoredDraft();
      return true;
    }
    if (adapter.hasComposerContent()) {
      setPendingRestore(draft);
      return true;
    }
    void performRestore(draft);
    return true;
  }, [performRestore, setPendingRestore]);

  const cancelRestoreConfirmation = useCallback(() => {
    setPendingRestore(undefined);
  }, [setPendingRestore]);
  const confirmRestore = useCallback(() => {
    const draft = pendingRestoreRef.current;
    if (!draft) return false;
    void performRestore(draft);
    return true;
  }, [performRestore]);

  return {
    cancelRestoreConfirmation,
    confirmRestore,
    deactivate,
    draftButtonRef,
    headingRef,
    isRestoreInFlight,
    listBusy,
    listError,
    loadMore,
    openDraft,
    page,
    pendingRestore,
    refresh,
    restoreBusyId,
    restoreError,
  };
}
