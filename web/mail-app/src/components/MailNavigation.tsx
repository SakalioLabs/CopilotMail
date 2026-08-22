import type { Locale, Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import { LanguagePicker } from "./LanguagePicker";
import { MailboxPicker } from "./MailboxPicker";

export type MailView = "inbox" | "sent" | "drafts";

interface MailNavigationProps {
  locale: Locale;
  activeMailbox: MailboxDirectoryItem;
  mailboxes: readonly MailboxDirectoryItem[];
  mailboxHasMore: boolean;
  mailboxLoadMoreBusy: boolean;
  mailboxLoadMoreError?: string;
  canCompose: boolean;
  composeBusy?: boolean;
  sessionLocked?: boolean;
  activeView: MailView;
  onLocaleChange: (locale: Locale) => void;
  onMailboxChange: (mailboxId: string) => void;
  onMailboxLoadMore: () => void;
  onCompose: () => void;
  onViewChange: (view: MailView) => void;
  onSignOut: () => void;
  t: Translator;
}

export function DesktopNavigation({
  locale,
  activeMailbox,
  mailboxes,
  mailboxHasMore,
  mailboxLoadMoreBusy,
  mailboxLoadMoreError,
  canCompose,
  composeBusy = false,
  sessionLocked = false,
  activeView,
  onLocaleChange,
  onMailboxChange,
  onMailboxLoadMore,
  onCompose,
  onViewChange,
  onSignOut,
  t,
}: MailNavigationProps) {
  return (
    <aside className="sidebar" data-dialog-background>
      <div className="brand">
        <span className="mark" aria-hidden="true">C</span>
        <strong>{t("app.name")}</strong>
      </div>
      <MailboxPicker
        activeMailboxId={activeMailbox.mailbox_id}
        disabled={sessionLocked}
        mailboxes={mailboxes}
        hasMore={mailboxHasMore}
        loadMoreBusy={mailboxLoadMoreBusy}
        loadMoreError={mailboxLoadMoreError}
        onChange={onMailboxChange}
        onLoadMore={onMailboxLoadMore}
        t={t}
      />
      {!canCompose && (
        <p className="mailbox-read-only" role="status">{t("mailbox.readOnly")}</p>
      )}
      <button
        className="compose"
        disabled={!canCompose || composeBusy}
        title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
        onClick={onCompose}
      >
        <span aria-hidden="true">{t("icon.compose")}</span>{" "}
        {t("nav.compose")}
      </button>
      <nav aria-label={t("nav.mailboxLabel")}>
        <button
          className={activeView === "inbox" ? "active" : undefined}
          aria-current={activeView === "inbox" ? "page" : undefined}
          onClick={() => onViewChange("inbox")}
        >
          {t("nav.inbox")}
        </button>
        <button
          className={activeView === "sent" ? "active" : undefined}
          aria-current={activeView === "sent" ? "page" : undefined}
          disabled={!canCompose}
          title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
          onClick={() => onViewChange("sent")}
        >
          {t("nav.sent")}
        </button>
        <button
          className={activeView === "drafts" ? "active" : undefined}
          aria-current={activeView === "drafts" ? "page" : undefined}
          disabled={!canCompose}
          title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
          onClick={() => onViewChange("drafts")}
        >
          {t("nav.drafts")}
        </button>
      </nav>
      <div className="sidebar-footer">
        <LanguagePicker locale={locale} onChange={onLocaleChange} t={t} />
        <button className="sidebar-sign-out" onClick={onSignOut}>
          {t("nav.signOut")}
        </button>
      </div>
    </aside>
  );
}

export function MobileToolbar({
  locale,
  activeMailbox,
  mailboxes,
  mailboxHasMore,
  mailboxLoadMoreBusy,
  mailboxLoadMoreError,
  canCompose,
  composeBusy = false,
  sessionLocked = false,
  activeView,
  onLocaleChange,
  onMailboxChange,
  onMailboxLoadMore,
  onCompose,
  onViewChange,
  onSignOut,
  t,
}: MailNavigationProps) {
  return (
    <header
      className="mobile-toolbar"
      aria-label={t("nav.mobileActions")}
      data-dialog-background
    >
      <div className="mobile-toolbar-main">
        <div className="mobile-brand">
          <span className="mark" aria-hidden="true">C</span>
          <strong>{t("app.name")}</strong>
        </div>
        {!canCompose && (
          <span className="sr-only" role="status">{t("mailbox.readOnly")}</span>
        )}
        <div className="mobile-actions">
          <MailboxPicker
            activeMailboxId={activeMailbox.mailbox_id}
            disabled={sessionLocked}
            className="mobile-mailbox-picker"
            mailboxes={mailboxes}
            hasMore={mailboxHasMore}
            loadMoreBusy={mailboxLoadMoreBusy}
            loadMoreError={mailboxLoadMoreError}
            onChange={onMailboxChange}
            onLoadMore={onMailboxLoadMore}
            t={t}
          />
          <button
            className="mobile-compose"
            aria-label={t("nav.compose")}
            disabled={!canCompose || composeBusy}
            title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
            onClick={onCompose}
          >
            <span aria-hidden="true">{t("icon.compose")}</span>
            <span className="mobile-action-label">{t("nav.compose")}</span>
          </button>
          <LanguagePicker locale={locale} onChange={onLocaleChange} t={t} />
          <button
            className="mobile-sign-out"
            aria-label={t("nav.signOut")}
            onClick={onSignOut}
          >
            <span aria-hidden="true">{t("icon.signOut")}</span>
            <span className="mobile-action-label">{t("nav.signOut")}</span>
          </button>
        </div>
      </div>
      <nav className="mobile-view-tabs" aria-label={t("nav.mobileViews")}>
        <button
          className={activeView === "inbox" ? "active" : undefined}
          aria-current={activeView === "inbox" ? "page" : undefined}
          onClick={() => onViewChange("inbox")}
        >
          {t("nav.inbox")}
        </button>
        <button
          className={activeView === "sent" ? "active" : undefined}
          aria-current={activeView === "sent" ? "page" : undefined}
          disabled={!canCompose}
          title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
          onClick={() => onViewChange("sent")}
        >
          {t("nav.sent")}
        </button>
        <button
          className={activeView === "drafts" ? "active" : undefined}
          aria-current={activeView === "drafts" ? "page" : undefined}
          disabled={!canCompose}
          onClick={() => onViewChange("drafts")}
        >
          {t("nav.drafts")}
        </button>
      </nav>
    </header>
  );
}
