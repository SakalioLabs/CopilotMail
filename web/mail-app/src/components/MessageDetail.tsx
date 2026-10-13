import type { RefObject } from "react";
import { EmptyState, Icon } from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { formatKilobytes } from "../i18n";
import type { MessageDetail as MessageDetailValue, MessageSummary } from "../types";
import { MessageReadingHeader } from "./MessageReadingHeader";

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
        <div className="reading">
          <button className="button button--secondary detail-back" onClick={onBack}>
            <Icon aria-hidden name="arrow-left" size={17} />
            {t("detail.backToInbox")}
          </button>
          <MessageReadingHeader
            canReply={canReply}
            detail={detail}
            headingRef={headingRef}
            locale={locale}
            selected={selected}
            t={t}
            onReply={onReply}
            onToggleFlag={onToggleFlag}
          />
          {actionError && <p className="detail-action-error" role="alert">{actionError}</p>}
          {loadError ? (
            <div className="detail-load-error">
              <p role="alert">{loadError}</p>
              <button className="button button--secondary" type="button" onClick={onRetry}>{t("detail.retry")}</button>
            </div>
          ) : !detail ? (
            <p className="detail-preview" role="status" aria-live="polite">
              {t("detail.loading")}
            </p>
          ) : (
            <>
              <div className="detail-body">
                {detail.text_body || t("detail.noSafeText")}
              </div>
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
        <EmptyState
          className="empty-detail"
          headingLevel={2}
          icon="mail"
          title={t("detail.selectMessage")}
        />
      )}
    </section>
  );
}
