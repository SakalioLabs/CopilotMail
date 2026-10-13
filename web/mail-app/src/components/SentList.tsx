import type { RefObject } from "react";
import { EmptyState, IconButton } from "@copilotmail/design-system";
import { formatCount, type Locale, type Translator } from "../i18n";
import type { HumanSentSubmissionPage } from "../types";
import { SentDirectoryItem } from "./SentDirectoryItem";

interface SentListProps {
  error?: string;
  headingRef: RefObject<HTMLHeadingElement | null>;
  listBusy: boolean;
  locale: Locale;
  page?: HumanSentSubmissionPage;
  t: Translator;
  onLoadMore: () => void;
  onRefresh: () => void;
}

function sentCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("sent.count.one", { count })
    : t("sent.count.other", { count });
}

export function SentList({
  error,
  headingRef,
  listBusy,
  locale,
  page,
  t,
  onLoadMore,
  onRefresh,
}: SentListProps) {
  const loading = listBusy || (!page && !error);
  const status = loading
    ? t("sent.syncing")
    : page
      ? sentCount(locale, page.items.length, t)
      : "";

  return (
    <section className="inbox sent-pane" data-dialog-background>
      <header className="inbox-header">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>{t("sent.heading")}</h1>
          <p aria-atomic="true" aria-live="polite" role="status">{status}</p>
        </div>
        <IconButton
          aria-busy={loading || undefined}
          disabled={listBusy}
          icon="refresh"
          label={t("sent.refreshAria")}
          onClick={onRefresh}
        />
      </header>
      {error && (
        <div className="error error--actionable" role="alert">
          <span>{error}</span>
          <button className="button button--secondary" disabled={listBusy} type="button" onClick={onRefresh}>
            {t("common.retry")}
          </button>
        </div>
      )}
      <div className="messages sent-items" aria-busy={loading || undefined}>
        {page?.items.length ? <p className="directory-hint">{t("sent.detailHint")}</p> : null}
        {page?.items.map((submission) => (
          <SentDirectoryItem
            key={submission.submission_id}
            locale={locale}
            submission={submission}
            t={t}
          />
        ))}
        {page && page.items.length === 0 && !listBusy && (
          <EmptyState
            className="empty empty--actionable"
            headingLevel={2}
            icon="sent"
            title={t("sent.empty")}
            action={(
              <button className="button button--secondary empty-action" type="button" onClick={onRefresh}>
                {t("sent.refreshAria")}
              </button>
            )}
          />
        )}
        {page?.has_more && (
          <button
            className="button button--secondary load-more"
            disabled={listBusy}
            onClick={onLoadMore}
          >
            {listBusy ? t("sent.loadingMore") : t("sent.loadMore")}
          </button>
        )}
      </div>
    </section>
  );
}
