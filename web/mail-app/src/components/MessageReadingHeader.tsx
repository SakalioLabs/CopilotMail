import type { RefObject } from "react";
import { Icon, IconButton } from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { formatDate } from "../i18n";
import type { MessageDetail, MessageSummary } from "../types";

interface MessageReadingHeaderProps {
  canReply: boolean;
  detail?: MessageDetail;
  headingRef: RefObject<HTMLHeadingElement | null>;
  locale: Locale;
  selected: MessageSummary;
  t: Translator;
  onReply: () => void;
  onToggleFlag: () => void;
}

function addresses(
  items: MessageSummary["from"],
  t: Translator,
  emptyValue: string,
) {
  return items.map((item) => item.name
    ? `${item.name} <${item.email}>`
    : item.email)
    .join(t("common.recipientSeparator")) || emptyValue;
}

/**
 * Pure Reading header. The selected queue summary remains authoritative while
 * detail content is loading; detail-only fields are deliberately gated below.
 */
export function MessageReadingHeader({
  canReply,
  detail,
  headingRef,
  locale,
  selected,
  t,
  onReply,
  onToggleFlag,
}: MessageReadingHeaderProps) {
  return (
    <header className="reading-header">
      <p className="eyebrow">{t("detail.eyebrow")}</p>
      <div className="detail-title">
        <h2 ref={headingRef} tabIndex={-1}>
          {selected.subject || t("subject.none")}
        </h2>
      </div>
      <dl className="message-metadata">
        <div>
          <dt>{t("detail.fromLabel")}</dt>
          <dd>{addresses(selected.from, t, t("sender.unknown"))}</dd>
        </div>
        <div>
          <dt>{t("detail.receivedLabel")}</dt>
          <dd>
            <time dateTime={selected.received_at}>
              {formatDate(locale, selected.received_at, {
                hour: "2-digit",
                minute: "2-digit",
              })}
            </time>
          </dd>
        </div>
        {detail ? (
          <div>
            <dt>{t("detail.toLabel")}</dt>
            <dd>{addresses(detail.to, t, t("common.emptyValue"))}</dd>
          </div>
        ) : null}
        {detail && detail.cc.length > 0 ? (
          <div>
            <dt>{t("detail.ccLabel")}</dt>
            <dd>{addresses(detail.cc, t, t("common.emptyValue"))}</dd>
          </div>
        ) : null}
      </dl>
      <div className="reading-actions" role="group" aria-label={t("detail.actionsAria")}>
        <IconButton
          icon={selected.is_flagged ? "star-filled" : "star"}
          label={
            selected.is_flagged
              ? t("detail.removeStarAria")
              : t("detail.addStarAria")
          }
          onClick={onToggleFlag}
        />
        {detail?.reply_to_message_id ? (
          <button
            className="reply-action"
            disabled={!canReply}
            title={!canReply ? t("mailbox.composeUnavailable") : undefined}
            onClick={onReply}
          >
            <Icon aria-hidden name="reply" size={17} />
            {t("detail.reply")}
          </button>
        ) : null}
      </div>
    </header>
  );
}
