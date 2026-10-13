import { Icon } from "@copilotmail/design-system";
import type { Translator } from "../i18n";
import type { MailboxDirectoryItem, MailboxKind } from "../types";

const MAILBOX_KIND_ORDER = ["human", "agent", "shared", "group"] as const satisfies
  readonly MailboxKind[];

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
  const mailboxGroups = MAILBOX_KIND_ORDER.map((kind) => ({
    kind,
    items: mailboxes.filter((mailbox) => mailbox.kind === kind),
  })).filter((group) => group.items.length > 0);

  return (
    <div className={["mailbox-picker", className].filter(Boolean).join(" ")}>
      <label>
        <span>{t("mailbox.selectorLabel")}</span>
        <select
          className="control"
          aria-label={t("mailbox.selectorAria")}
          disabled={disabled}
          value={activeMailboxId}
          onChange={(event) => onChange(event.target.value)}
        >
          {mailboxGroups.map(({ kind, items }) => (
            <optgroup key={kind} label={t(`mailbox.group.${kind}`)}>
              {items.map((mailbox) => (
                <option key={mailbox.mailbox_id} value={mailbox.mailbox_id}>
                  {t("mailbox.option", {
                    address: mailbox.primary_address,
                    kind: t(`mailbox.kind.${mailbox.kind}`),
                  })}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </label>
      {hasMore && (
        <button
          type="button"
          className="button button--secondary mailbox-load-more"
          aria-label={loadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
          disabled={disabled || loadMoreBusy}
          title={loadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
          onClick={onLoadMore}
        >
          <Icon aria-hidden name="add" size={18} />
          <span className="mailbox-load-more__label">
            {loadMoreBusy ? t("mailbox.loadingMore") : t("mailbox.loadMore")}
          </span>
        </button>
      )}
      {loadMoreError && <span className="mailbox-page-error" role="alert">{loadMoreError}</span>}
    </div>
  );
}
