import type { RefObject } from "react";
import type { Locale, Translator } from "../i18n";
import { formatKilobytes } from "../i18n";
import type { MessageDetail as MessageDetailValue, MessageSummary } from "../types";

interface MessageDetailProps {
  actionError?: string;
  detail?: MessageDetailValue;
  headingRef: RefObject<HTMLHeadingElement | null>;
  loadError?: string;
  locale: Locale;
  canReply: boolean;
  selected?: MessageSummary;
  t: Translator;
  onBack: () => void;
  onReply: () => void;
  onRetry: () => void;
  onToggleFlag: () => void;
}

function sender(message: MessageSummary, t: Translator) {
  const first = message.from[0];
  return first?.name || first?.email || t("sender.unknown");
}

export function MessageDetail({
  actionError,
  detail,
  headingRef,
  loadError,
  locale,
  canReply,
  selected,
  t,
  onBack,
  onReply,
  onRetry,
  onToggleFlag,
}: MessageDetailProps) {
  return (
    <section className="detail" data-dialog-background>
      {selected ? (
        <div>
          <button className="detail-back" onClick={onBack}>
            <span aria-hidden="true">←</span> {t("detail.backToInbox")}
          </button>
          <p className="eyebrow">{t("detail.eyebrow")}</p>
          <div className="detail-title">
            <h2 ref={headingRef} tabIndex={-1}>
              {selected.subject || t("subject.none")}
            </h2>
            <button
              aria-label={
                selected.is_flagged
                  ? t("detail.removeStarAria")
                  : t("detail.addStarAria")
              }
              onClick={onToggleFlag}
            >
              <span aria-hidden="true">
                {selected.is_flagged ? t("icon.starred") : t("icon.notStarred")}
              </span>
            </button>
          </div>
          <p>{sender(selected, t)}</p>
          {actionError && <p className="detail-action-error" role="alert">{actionError}</p>}
          {loadError ? (
            <div className="detail-load-error">
              <p role="alert">{loadError}</p>
              <button type="button" onClick={onRetry}>{t("detail.retry")}</button>
            </div>
          ) : !detail ? (
            <p className="detail-preview" role="status" aria-live="polite">
              {t("detail.loading")}
            </p>
          ) : (
            <>
              <p className="recipients">
                {t("detail.recipients", {
                  recipients: detail.to
                    .map((item) => item.name || item.email)
                    .join(t("common.recipientSeparator")) || t("common.emptyValue"),
                })}
              </p>
              <div className="detail-body">
                {detail.text_body || t("detail.noSafeText")}
              </div>
              {detail.reply_to_message_id && (
                <button
                  className="reply-action"
                  disabled={!canReply}
                  title={!canReply ? t("mailbox.composeUnavailable") : undefined}
                  onClick={onReply}
                >
                  {t("detail.reply")}
                </button>
              )}
              {detail.text_body_truncated && (
                <p className="notice">{t("detail.bodyTruncated")}</p>
              )}
              {detail.attachments.length > 0 && (
                <section className="attachments">
                  <h3>{t("detail.attachments")}</h3>
                  {detail.attachments.map((item, index) => (
                    <div key={`${item.name}-${index}`}>
                      <span>{item.name || t("detail.unnamedAttachment")}</span>
                      <small>
                        {t("detail.attachmentMeta", {
                          mediaType: item.media_type,
                          size: formatKilobytes(locale, item.size_bytes),
                        })}
                      </small>
                    </div>
                  ))}
                </section>
              )}
            </>
          )}
        </div>
      ) : (
        <div className="empty-detail">
          <span aria-hidden="true">{t("icon.emptyDetail")}</span>
          <p>{t("detail.selectMessage")}</p>
        </div>
      )}
    </section>
  );
}
