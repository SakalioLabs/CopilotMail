import type { RefCallback, RefObject } from "react";
import { formatCount, formatDate, type Locale, type Translator } from "../i18n";
import type { HumanDraftListItem, HumanDraftPage } from "../types";

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

function recipientCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("drafts.recipientCount.one", { count })
    : t("drafts.recipientCount.other", { count });
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
  return (
    <section className="inbox drafts-pane" data-dialog-background>
      <header className="inbox-header">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>{t("drafts.heading")}</h1>
          <p>
            {page
              ? draftCount(locale, page.items.length, t)
              : t("drafts.syncing")}
          </p>
        </div>
        <button
          aria-label={t("drafts.refreshAria")}
          disabled={listBusy || restoreBusyId !== undefined}
          onClick={onRefresh}
        >
          <span aria-hidden="true">{t("icon.refresh")}</span>
        </button>
      </header>
      {error && <p className="error" role="alert">{error}</p>}
      {restoreError && <p className="error" role="alert">{restoreError}</p>}
      <div className="messages draft-items" aria-busy={listBusy || undefined}>
        {page?.items.map((draft) => {
          const restoring = restoreBusyId === draft.draft_id;
          const kind = draft.reply_to_message_id
            ? t("drafts.replyDraft")
            : t("drafts.newDraft");
          return (
            <button
              key={draft.draft_id}
              ref={draftButtonRef(draft.draft_id)}
              className="draft-item"
              disabled={restoreBusyId !== undefined}
              onClick={() => onRestore(draft)}
            >
              <strong>{kind}</strong>
              <time dateTime={draft.updated_at}>
                {formatDate(locale, draft.updated_at, {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </time>
              <span>{recipientCount(locale, draft.recipient_count, t)}</span>
              <span className="draft-restore-label">
                {restoring ? t("drafts.restoring") : t("drafts.restore")}
              </span>
            </button>
          );
        })}
        {page && page.items.length === 0 && !listBusy && (
          <div className="empty">{t("drafts.empty")}</div>
        )}
        {page?.has_more && (
          <button
            className="load-more"
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
