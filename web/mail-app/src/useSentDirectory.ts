import { useCallback, useEffect, useRef, useState } from "react";
import {
  compareHumanSentSubmissionItems,
  listSentSubmissions,
} from "./api";
import type {
  HumanSentSubmissionListItem,
  HumanSentSubmissionPage,
  MailboxScopedConfig,
} from "./types";

/**
 * UI-safe error keys deliberately kept at the directory boundary. The hook
 * never exposes transport errors or server response details to its caller.
 */
export type SentDirectoryError =
  | "error.sentUnavailable"
  | "error.sentPageUnavailable";

export interface UseSentDirectoryOptions {
  /** Whether the current mailbox view is the sent directory. */
  readonly active: boolean;
  readonly authenticated: boolean;
  readonly canSend: boolean;
  readonly mailboxSettings?: MailboxScopedConfig;
}

interface DirectoryScope {
  readonly active: boolean;
  readonly authenticated: boolean;
  readonly canSend: boolean;
  readonly mailboxSettings?: MailboxScopedConfig;
}

function canReadSentDirectory(scope: DirectoryScope): scope is DirectoryScope & {
  readonly mailboxSettings: MailboxScopedConfig;
} {
  return scope.active
    && scope.authenticated
    && scope.canSend
    && scope.mailboxSettings !== undefined;
}

/**
 * Sent pages are newest first. A cursor continuation must therefore begin
 * strictly after the accumulated tail and may never repeat an identity.
 */
export function mergeSentDirectoryPages(
  current: readonly HumanSentSubmissionListItem[],
  next: readonly HumanSentSubmissionListItem[],
): HumanSentSubmissionListItem[] {
  const currentTail = current.at(-1);
  if (
    currentTail
    && next[0]
    && compareHumanSentSubmissionItems(currentTail, next[0]) !== -1
  ) {
    throw new Error("sent_list_order_conflict");
  }

  const identities = new Set(current.map((submission) => submission.submission_id));
  for (const submission of next) {
    if (identities.has(submission.submission_id)) {
      throw new Error("sent_list_identity_conflict");
    }
    identities.add(submission.submission_id);
  }
  return [...current, ...next];
}

/**
 * Owns the sent-mail directory's cancellable, cursor-safe read model.
 *
 * `deactivate()` is intentionally public: App calls it synchronously before
 * changing a mailbox or view so a late response cannot briefly bind to the
 * new UI while React schedules the next effect.
 */
export function useSentDirectory({
  active,
  authenticated,
  canSend,
  mailboxSettings,
}: UseSentDirectoryOptions) {
  const [page, setPageState] = useState<HumanSentSubmissionPage>();
  const [listBusy, setListBusyState] = useState(false);
  const [error, setErrorState] = useState<SentDirectoryError>();
  const [refreshGeneration, setRefreshGeneration] = useState(0);

  const pageRef = useRef<HumanSentSubmissionPage | undefined>(undefined);
  const listBusyRef = useRef(false);
  const scopeRef = useRef<DirectoryScope>({
    active,
    authenticated,
    canSend,
    mailboxSettings,
  });
  const requestGeneration = useRef(0);
  const requestAbort = useRef<AbortController | undefined>(undefined);
  const pageRequest = useRef<string | undefined>(undefined);
  const focusAfterRefresh = useRef(false);
  const headingRef = useRef<HTMLHeadingElement>(null);

  // Keep request callbacks independent from an App-sized closure. This is
  // also consulted during the commit-to-effect window after a scope change.
  scopeRef.current = { active, authenticated, canSend, mailboxSettings };

  const replacePage = useCallback((next?: HumanSentSubmissionPage) => {
    pageRef.current = next;
    setPageState(next);
  }, []);
  const setListBusy = useCallback((next: boolean) => {
    listBusyRef.current = next;
    setListBusyState(next);
  }, []);
  const setError = useCallback((next?: SentDirectoryError) => {
    setErrorState(next);
  }, []);

  const invalidateRequest = useCallback(() => {
    requestGeneration.current += 1;
    requestAbort.current?.abort();
    requestAbort.current = undefined;
    pageRequest.current = undefined;
  }, []);

  const requestIsCurrent = useCallback((
    generation: number,
    controller: AbortController,
    mailboxId: string,
  ) => {
    const scope = scopeRef.current;
    return generation === requestGeneration.current
      && !controller.signal.aborted
      && canReadSentDirectory(scope)
      && scope.mailboxSettings.mailboxId === mailboxId;
  }, []);

  const deactivate = useCallback(() => {
    invalidateRequest();
    focusAfterRefresh.current = false;
    replacePage(undefined);
    setError(undefined);
    setListBusy(false);
  }, [invalidateRequest, replacePage, setError, setListBusy]);

  useEffect(() => {
    invalidateRequest();
    replacePage(undefined);
    setError(undefined);
    setListBusy(false);

    const scope = scopeRef.current;
    if (!canReadSentDirectory(scope)) {
      if (!scope.active) focusAfterRefresh.current = false;
      return;
    }

    const requestMailboxId = scope.mailboxSettings.mailboxId;
    const requestSettings = scope.mailboxSettings;
    const controller = new AbortController();
    const generation = ++requestGeneration.current;
    requestAbort.current = controller;
    setListBusy(true);

    void listSentSubmissions(requestSettings, undefined, controller.signal)
      .then((nextPage) => {
        if (!requestIsCurrent(generation, controller, requestMailboxId)) return;
        replacePage(nextPage);
        if (!focusAfterRefresh.current) return;
        focusAfterRefresh.current = false;
        window.requestAnimationFrame(() => {
          if (requestIsCurrent(generation, controller, requestMailboxId)) {
            headingRef.current?.focus();
          }
        });
      })
      .catch(() => {
        if (!requestIsCurrent(generation, controller, requestMailboxId)) return;
        focusAfterRefresh.current = false;
        setError("error.sentUnavailable");
      })
      .finally(() => {
        if (requestIsCurrent(generation, controller, requestMailboxId)) {
          setListBusy(false);
        }
      });

    // `requestAbort` may have been replaced by a cursor continuation. Abort
    // through the shared invalidator so an unmount or scope change also stops
    // that newer request instead of only this effect's initial controller.
    return () => invalidateRequest();
  }, [
    active,
    authenticated,
    canSend,
    invalidateRequest,
    mailboxSettings,
    refreshGeneration,
    replacePage,
    requestIsCurrent,
    setError,
    setListBusy,
  ]);

  const refresh = useCallback(() => {
    if (!canReadSentDirectory(scopeRef.current)) return false;
    // Invalidate immediately, rather than waiting for React to run the
    // refresh effect, so the old page cannot win a race with the refresh.
    invalidateRequest();
    focusAfterRefresh.current = true;
    setRefreshGeneration((generation) => generation + 1);
    return true;
  }, [invalidateRequest]);

  const loadMore = useCallback(async () => {
    const scope = scopeRef.current;
    const currentPage = pageRef.current;
    if (
      !canReadSentDirectory(scope)
      || !currentPage?.has_more
      || !currentPage.next_cursor
      || listBusyRef.current
      || pageRequest.current !== undefined
    ) return false;

    const generation = requestGeneration.current;
    const requestMailboxId = scope.mailboxSettings.mailboxId;
    const requestSettings = scope.mailboxSettings;
    const cursor = currentPage.next_cursor;
    const controller = new AbortController();
    requestAbort.current?.abort();
    requestAbort.current = controller;
    pageRequest.current = cursor;
    setListBusy(true);
    setError(undefined);

    try {
      const nextPage = await listSentSubmissions(requestSettings, cursor, controller.signal);
      if (!requestIsCurrent(generation, controller, requestMailboxId)) return false;
      const accumulatedPage = pageRef.current;
      if (!accumulatedPage) return false;
      replacePage({
        ...nextPage,
        items: mergeSentDirectoryPages(accumulatedPage.items, nextPage.items),
      });
      return true;
    } catch {
      if (requestIsCurrent(generation, controller, requestMailboxId)) {
        setError("error.sentPageUnavailable");
      }
      return false;
    } finally {
      if (pageRequest.current === cursor) pageRequest.current = undefined;
      if (requestIsCurrent(generation, controller, requestMailboxId)) {
        setListBusy(false);
      }
    }
  }, [replacePage, requestIsCurrent, setError, setListBusy]);

  return {
    deactivate,
    error,
    headingRef,
    listBusy,
    loadMore,
    page,
    refresh,
  };
}
