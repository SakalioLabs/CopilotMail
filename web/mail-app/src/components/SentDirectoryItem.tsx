import { Icon, StatusIndicator, type BadgeTone } from "@copilotmail/design-system";
import { formatCount, formatDate, type Locale, type Translator } from "../i18n";
import type { HumanSentSubmissionListItem } from "../types";

interface SentDirectoryItemProps {
  locale: Locale;
  submission: HumanSentSubmissionListItem;
  t: Translator;
}

function recipientCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("sent.recipientCount.one", { count })
    : t("sent.recipientCount.other", { count });
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

function deliveryTone(submission: HumanSentSubmissionListItem): BadgeTone {
  const delivery = submission.delivery;
  if (!delivery || !delivery.terminal) return "neutral";
  return delivery.failed_count === 0 ? "positive" : "critical";
}

function deliveryState(submission: HumanSentSubmissionListItem) {
  const delivery = submission.delivery;
  if (!delivery) return "awaiting";
  if (!delivery.terminal) return "progress";
  return delivery.failed_count === 0 ? "delivered" : "completed-with-failures";
}

/**
 * Pure, non-interactive sent-directory projection. It deliberately exposes
 * only accepted-send and aggregate delivery facts; a sent submission never
 * gains a detail affordance merely through presentation.
 */
export function SentDirectoryItem({ locale, submission, t }: SentDirectoryItemProps) {
  const delivery = submission.delivery;

  return (
    <article
      className="sent-item directory-item"
      data-delivery-state={deliveryState(submission)}
    >
      <span className="directory-item__lead">
        <span className="directory-item__title">
          <Icon aria-hidden className="sent-item__accepted-mark" name="sent" size={17} />
          <strong>{t("sent.accepted")}</strong>
        </span>
        <span className="directory-item__facts">
          <span>{recipientCount(locale, submission.recipient_count, t)}</span>
          <time dateTime={submission.accepted_at}>
            {formatDate(locale, submission.accepted_at, {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </time>
        </span>
      </span>
      <span className="sent-item__delivery">
        <StatusIndicator
          className="sent-item__delivery-status"
          label={deliverySummary(submission, locale, t)}
          tone={deliveryTone(submission)}
        />
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
      </span>
    </article>
  );
}
