import type { RefCallback } from "react";
import { Icon, IconButton, StarFilledIcon, StarIcon } from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { formatDate } from "../i18n";
import type { MessageSummary } from "../types";

interface MessageQueueItemProps {
  locale: Locale;
  message: MessageSummary;
  selected: boolean;
  t: Translator;
  messageButtonRef: (messageId: string) => RefCallback<HTMLButtonElement>;
  onOpenMessage: (message: MessageSummary) => void;
  onToggleFlag: (message: MessageSummary) => void;
}

function sender(message: MessageSummary, t: Translator) {
  const first = message.from[0];
  return first?.name || first?.email || t("sender.unknown");
}

/**
 * Pure queue presentation for one existing Inbox/Search summary.
 *
 * It intentionally receives no detail model and owns no request state: selecting a
 * row remains a controller decision in App, so scanning or keyboard selection
 * cannot turn into a content read. The star control is a sibling of the row
 * button so flagging never triggers an open.
 */
export function MessageQueueItem({
  locale,
  message,
  selected,
  t,
  messageButtonRef,
  onOpenMessage,
  onToggleFlag,
}: MessageQueueItemProps) {
  const className = [
    "message",
    selected ? "selected" : "",
    message.is_read ? "" : "unread",
  ].filter(Boolean).join(" ");

  return (
    <div
      className={[
        "message-row",
        selected ? "message-row--selected" : "",
        message.is_read ? "" : "message-row--unread",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <IconButton
        className="message__flag"
        icon={message.is_flagged ? <StarFilledIcon /> : <StarIcon />}
        label={
          message.is_flagged ? t("detail.removeStarAria") : t("detail.addStarAria")
        }
        onClick={() => onToggleFlag(message)}
      />
      <button
        ref={messageButtonRef(message.message_id)}
        className={className}
        aria-current={selected ? "true" : undefined}
        onClick={() => onOpenMessage(message)}
      >
        <span className="message__sender sender">{sender(message, t)}</span>
        <span className="message__trail">
          <strong className="message__subject">
            {message.subject || t("subject.none")}
          </strong>
          <span className="message__preview preview">
            {" — "}
            {message.preview}
          </span>
        </span>
        <span className="message-icons message__indicators" aria-hidden="true">
          {message.is_flagged ? <Icon name="star-filled" size={15} /> : null}
          {message.has_attachment ? <Icon name="attachment" size={15} /> : null}
        </span>
        <time className="message__received" dateTime={message.received_at}>
          {formatDate(locale, message.received_at)}
        </time>
        {!message.is_read && <span className="sr-only">{t("message.unread")}</span>}
        {message.is_flagged && <span className="sr-only">{t("message.starred")}</span>}
        {message.has_attachment && <span className="sr-only">{t("message.hasAttachment")}</span>}
      </button>
    </div>
  );
}
