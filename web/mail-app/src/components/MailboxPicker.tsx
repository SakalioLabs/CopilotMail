import type { Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";

interface MailboxPickerProps {
  activeMailboxId: string;
  className?: string;
  disabled?: boolean;
  mailboxes: readonly MailboxDirectoryItem[];
  hasMore: boolean;
  loadMoreBusy: boolean;
  loadMoreError?: string;
  onChange: (mailboxId: string) => void;
  onLoadMore: () => void;
  t: Translator;
}

export function MailboxPicker({
  activeMailboxId,
  className = "",
  disabled = false,
  mailboxes,
  hasMore,
  loadMoreBusy,
  loadMoreError,
  onChange,
  onLoadMore,
  t,
}: MailboxPickerProps) {
  return (
    <div className={["mailbox-picker", className].filter(Boolean).join(" ")}>
      <label>
        <span>{t("mailbox.selectorLabel")}</span>
        <select
          aria-label={t("mailbox.selectorAria")}
          disabled={disabled}
          value={activeMailboxId}
          onChange={(event) => onChange(event.target.value)}
        >
          {mailboxes.map((mailbox) => (
            <option key={mailbox.mailbox_id} value={mailbox.mailbox_id}>
              {t("mailbox.option", {
                address: mailbox.primary_address,
                kind: t(`mailbox.kind.${mailbox.kind}`),
              })}
            </option>
          ))}
        </select>
      </label>
      {hasMore && (
        <button
          type="button"
          className="mailbox-load-more"
          aria-label={loadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
          disabled={disabled || loadMoreBusy}
          title={loadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
          onClick={onLoadMore}
        >
          {loadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
        </button>
      )}
      {loadMoreError && <span className="mailbox-page-error" role="alert">{loadMoreError}</span>}
    </div>
  );
}
