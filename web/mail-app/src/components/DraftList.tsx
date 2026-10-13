import type { RefCallback, RefObject } from "react";
import { EmptyState, IconButton } from "@copilotmail/design-system";
import { formatCount, type Locale, type Translator } from "../i18n";
import type { HumanDraftListItem, HumanDraftPage } from "../types";
import { DraftDirectoryItem } from "./DraftDirectoryItem";

interface DraftListProps {
  error?: string;
  headingRef: RefObject<HTMLHeadingElement | null>;
  listBusy: boolean;
  locale: Locale;
  page?: HumanDraftPage;
  restoreBusyId?: string;
  restoreError?: string;
  t: Translator;
  onLoadMore: () => void;
  onRefresh: () => void;
  onRestore: (draft: HumanDraftListItem) => void;
  draftButtonRef: (draftId: string) => RefCallback<HTMLButtonElement>;
}

function draftCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("drafts.count.one", { count })
    : t("drafts.count.other", { count });
}

export function DraftList({
  error,
  headingRef,
  listBusy,
  locale,
  page,
  restoreBusyId,
  restoreError,
  t,
  onLoadMore,
  onRefresh,
  onRestore,
  draftButtonRef,
}: DraftListProps) {
  const loading = listBusy || (!page && !error);
  const status = restoreBusyId !== undefined
    ? t("drafts.restoring")
    : loading
      ? t("drafts.syncing")
      : page
        ? draftCount(locale, page.items.length, t)
        : "";
  const retryDisabled = loading || restoreBusyId !== undefined;

  return (
    <section className="inbox drafts-pane" data-dialog-background>
      <header className="inbox-header">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>{t("drafts.heading")}</h1>
          <p aria-atomic="true" aria-live="polite" role="status">{status}</p>
        </div>
        <IconButton
          aria-busy={loading || undefined}
          disabled={retryDisabled}
          icon="refresh"
          label={t("drafts.refreshAria")}
          onClick={onRefresh}
        />
      </header>
      {error && (
        <div className="error error--actionable" role="alert">
          <span>{error}</span>
          <button className="button button--secondary" disabled={retryDisabled} type="button" onClick={onRefresh}>
            {t("common.retry")}
          </button>
        </div>
      )}
      {restoreError && (
        <div className="error error--actionable" role="alert">
          <span>{restoreError}</span>
          <button className="button button--secondary" disabled={retryDisabled} type="button" onClick={onRefresh}>
            {t("common.retry")}
          </button>
        </div>
      )}
      <div
        className="messages draft-items"
        aria-busy={loading || restoreBusyId !== undefined || undefined}
      >
        {page?.items.length ? <p className="directory-hint">{t("drafts.detailHint")}</p> : null}
        {page?.items.map((draft) => {
          const restoring = restoreBusyId === draft.draft_id;
          return (
            <DraftDirectoryItem
              key={draft.draft_id}
              draft={draft}
              draftButtonRef={draftButtonRef}
              locale={locale}
              restoring={restoring}
              restoreLocked={restoreBusyId !== undefined}
              t={t}
              onRestore={onRestore}
            />
          );
        })}
        {page && page.items.length === 0 && !listBusy && (
          <EmptyState
            className="empty empty--actionable"
            headingLevel={2}
            icon="draft"
            title={t("drafts.empty")}
            action={(
              <button className="button button--secondary empty-action" type="button" onClick={onRefresh}>
                {t("drafts.refreshAria")}
              </button>
            )}
          />
        )}
        {page?.has_more && (
          <button
            className="button button--secondary load-more"
            disabled={listBusy || restoreBusyId !== undefined}
            onClick={onLoadMore}
          >
            {listBusy ? t("drafts.loadingMore") : t("drafts.loadMore")}
          </button>
        )}
      </div>
    </section>
  );
}
