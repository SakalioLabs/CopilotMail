import type { RefObject } from "react";
import { formatCount, formatDate, type Locale, type Translator } from "../i18n";
import type { HumanSentSubmissionListItem, HumanSentSubmissionPage } from "../types";

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

function recipientCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("sent.recipientCount.one", { count })
    : t("sent.recipientCount.other", { count });
}

function sentCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("sent.count.one", { count })
    : t("sent.count.other", { count });
}

function deliverySummary(
  submission: HumanSentSubmissionListItem,
  locale: Locale,
  t: Translator,
) {
  const delivery = submission.delivery;
  if (!delivery) return t("sent.awaitingDelivery");
  const total = formatCount(locale, delivery.recipient_count);
  if (delivery.terminal && delivery.failed_count === 0) {
    return t("sent.deliveryDelivered", { count: total });
  }
  if (delivery.terminal) {
    return t("sent.deliveryCompleted", {
      delivered: formatCount(locale, delivery.delivered_count),
      failed: formatCount(locale, delivery.failed_count),
    });
  }
  return t("sent.deliveryProgress", {
    delivered: formatCount(locale, delivery.delivered_count),
    total,
    failed: formatCount(locale, delivery.failed_count),
    queued: formatCount(locale, delivery.queued_count),
    unknown: formatCount(locale, delivery.unknown_count),
  });
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
  return (
    <section className="inbox sent-pane" data-dialog-background>
      <header className="inbox-header">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>{t("sent.heading")}</h1>
          <p>{page ? sentCount(locale, page.items.length, t) : t("sent.syncing")}</p>
        </div>
        <button
          aria-label={t("sent.refreshAria")}
          disabled={listBusy}
          onClick={onRefresh}
        >
          <span aria-hidden="true">{t("icon.refresh")}</span>
        </button>
      </header>
      {error && <p className="error" role="alert">{error}</p>}
      <div className="messages sent-items" aria-busy={listBusy || undefined}>
        {page?.items.map((submission) => {
          const delivery = submission.delivery;
          const acceptedAt = formatDate(locale, submission.accepted_at, {
            hour: "2-digit",
            minute: "2-digit",
          });
          return (
            <article className="sent-item" key={submission.submission_id}>
              <strong>{t("sent.accepted")}</strong>
              <time dateTime={submission.accepted_at}>{acceptedAt}</time>
              <span>{recipientCount(locale, submission.recipient_count, t)}</span>
              <p>{deliverySummary(submission, locale, t)}</p>
              {delivery && (
                <small>
                  {t("sent.deliveryUpdated", {
                    date: formatDate(locale, delivery.observed_at, {
                      hour: "2-digit",
                      minute: "2-digit",
                    }),
                  })}
                </small>
              )}
            </article>
          );
        })}
        {page && page.items.length === 0 && !listBusy && (
          <div className="empty">{t("sent.empty")}</div>
        )}
        {page?.has_more && (
          <button
            className="load-more"
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
