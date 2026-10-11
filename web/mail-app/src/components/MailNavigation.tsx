import type { Locale, Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import {
  ContextBar,
  Icon,
  IdentityBadge,
  StatusIndicator,
} from "@copilotmail/design-system";
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
  onOpenCommandPalette: (trigger: HTMLElement) => void;
  onViewChange: (view: MailView) => void;
  onSignOut: () => void;
  t: Translator;
}

/**
 * The directory is the sole authority for mailbox identity.  Keep this
 * summary adjacent to navigation instead of deriving identity or sendability
 * from client state (for example, whether the compose button happens to be
 * enabled).
 */
function MailboxIdentity({
  activeMailbox,
  className = "",
  t,
}: Pick<MailNavigationProps, "activeMailbox" | "t"> & { className?: string }) {
  const mailboxKind = t(`mailbox.kind.${activeMailbox.kind}`);
  const sendability = activeMailbox.can_send
    ? t("mailbox.canSend")
    : t("mailbox.readOnly");

  return (
    <ContextBar
      className={["mailbox-identity", className].filter(Boolean).join(" ")}
      facts={[
        {
          id: "scope",
          label: t("mailbox.scopeLabel"),
          value: (
            <span className="mailbox-identity__address" title={activeMailbox.primary_address}>
              {activeMailbox.primary_address}
            </span>
          ),
        },
        {
          id: "identity",
          label: t("mailbox.identityFactLabel"),
          value: <IdentityBadge kind={activeMailbox.kind} label={mailboxKind} />,
        },
        {
          id: "send-availability",
          label: t("mailbox.serverStatus"),
          value: (
            <StatusIndicator
              className="mailbox-identity__status-value"
              data-sendable={activeMailbox.can_send}
              label={sendability}
              tone={activeMailbox.can_send ? "positive" : "warning"}
            />
          ),
        },
      ]}
      headingLevel={3}
      title={t("mailbox.identityLabel")}
    />
  );
}

/**
 * The compact mobile scope is factual even while its detailed controls remain
 * closed. Its address, identity label, and sendability all come from the
 * selected server-backed directory item; it never infers them from compose
 * availability or from the last interaction on this device.
 */
function MobileMailboxScopeSummary({
  activeMailbox,
  t,
}: Pick<MailNavigationProps, "activeMailbox" | "t">) {
  const mailboxKind = t(`mailbox.kind.${activeMailbox.kind}`);
  const sendability = activeMailbox.can_send
    ? t("mailbox.canSend")
    : t("mailbox.readOnly");

  return (
    <summary className="mobile-mailbox-scope__summary">
      <span className="mobile-mailbox-scope__summary-address" title={activeMailbox.primary_address}>
        {activeMailbox.primary_address}
      </span>
      <IdentityBadge
        className="mobile-mailbox-scope__summary-identity"
        kind={activeMailbox.kind}
        label={mailboxKind}
      />
      <StatusIndicator
        className="mobile-mailbox-scope__summary-status"
        data-sendable={activeMailbox.can_send}
        label={sendability}
        tone={activeMailbox.can_send ? "positive" : "warning"}
      />
      <Icon
        aria-hidden
        className="mobile-mailbox-scope__summary-indicator"
        name="chevron-down"
        size={16}
      />
    </summary>
  );
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
  onOpenCommandPalette,
  onViewChange,
  onSignOut,
  t,
}: MailNavigationProps) {
  return (
    <aside className="sidebar" data-dialog-background>
      <div className="brand">
        <span className="mark" aria-hidden="true"><Icon name="mail" size={19} /></span>
        <strong>{t("app.name")}</strong>
      </div>
      <MailboxIdentity activeMailbox={activeMailbox} t={t} />
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
      <button
        className="button compose"
        disabled={!canCompose || composeBusy}
        title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
        onClick={onCompose}
      >
        <Icon aria-hidden name="compose" size={18} />
        {t("nav.compose")}
      </button>
      <button
        className="command-trigger"
        aria-label={t("command.open")}
        onClick={(event) => onOpenCommandPalette(event.currentTarget)}
      >
        <Icon aria-hidden name="command" size={17} />
        <span>{t("command.title")}</span>
        <kbd aria-hidden="true">{t("command.triggerShortcut")}</kbd>
      </button>
      <nav aria-label={t("nav.mailboxLabel")}>
        <button
          className={activeView === "inbox" ? "active" : undefined}
          aria-current={activeView === "inbox" ? "page" : undefined}
          onClick={() => onViewChange("inbox")}
        >
          <Icon aria-hidden name="inbox" size={17} />
          {t("nav.inbox")}
        </button>
        <button
          className={activeView === "sent" ? "active" : undefined}
          aria-current={activeView === "sent" ? "page" : undefined}
          disabled={!canCompose}
          title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
          onClick={() => onViewChange("sent")}
        >
          <Icon aria-hidden name="sent" size={17} />
          {t("nav.sent")}
        </button>
        <button
          className={activeView === "drafts" ? "active" : undefined}
          aria-current={activeView === "drafts" ? "page" : undefined}
          disabled={!canCompose}
          title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
          onClick={() => onViewChange("drafts")}
        >
          <Icon aria-hidden name="draft" size={17} />
          {t("nav.drafts")}
        </button>
      </nav>
      <div className="sidebar-footer">
        <LanguagePicker locale={locale} onChange={onLocaleChange} t={t} />
        <button className="sidebar-sign-out" onClick={onSignOut}>
          <Icon aria-hidden name="sign-out" size={17} />
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
  onOpenCommandPalette,
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
          <span className="mark" aria-hidden="true"><Icon name="mail" size={18} /></span>
          <strong>{t("app.name")}</strong>
        </div>
        <div className="mobile-actions">
          <button
            className="mobile-compose"
            aria-label={t("nav.compose")}
            disabled={!canCompose || composeBusy}
            title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
            onClick={onCompose}
          >
            <Icon aria-hidden name="compose" size={18} />
            <span className="mobile-action-label">{t("nav.compose")}</span>
          </button>
          <button
            className="mobile-command-trigger"
            aria-label={t("command.open")}
            onClick={(event) => onOpenCommandPalette(event.currentTarget)}
          >
            <Icon aria-hidden name="command" size={18} />
            <span className="mobile-action-label">{t("command.title")}</span>
          </button>
        </div>
      </div>
      <details className="mobile-mailbox-scope">
        <MobileMailboxScopeSummary activeMailbox={activeMailbox} t={t} />
        <div className="mobile-mailbox-scope__content">
          <MailboxIdentity
            activeMailbox={activeMailbox}
            className="mobile-mailbox-identity"
            t={t}
          />
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
          <div className="mobile-mailbox-scope__session-controls">
            <LanguagePicker locale={locale} onChange={onLocaleChange} t={t} />
            <button
              className="mobile-sign-out"
              aria-label={t("nav.signOut")}
              onClick={onSignOut}
            >
              <Icon aria-hidden name="sign-out" size={18} />
              <span className="mobile-action-label">{t("nav.signOut")}</span>
            </button>
          </div>
        </div>
      </details>
      <nav className="mobile-view-tabs" aria-label={t("nav.mobileViews")}>
        <button
          className={activeView === "inbox" ? "active" : undefined}
          aria-current={activeView === "inbox" ? "page" : undefined}
          onClick={() => onViewChange("inbox")}
        >
          <Icon aria-hidden name="inbox" size={16} />
          {t("nav.inbox")}
        </button>
        <button
          className={activeView === "sent" ? "active" : undefined}
          aria-current={activeView === "sent" ? "page" : undefined}
          disabled={!canCompose}
          title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
          onClick={() => onViewChange("sent")}
        >
          <Icon aria-hidden name="sent" size={16} />
          {t("nav.sent")}
        </button>
        <button
          className={activeView === "drafts" ? "active" : undefined}
          aria-current={activeView === "drafts" ? "page" : undefined}
          disabled={!canCompose}
          onClick={() => onViewChange("drafts")}
        >
          <Icon aria-hidden name="draft" size={16} />
          {t("nav.drafts")}
        </button>
      </nav>
    </header>
  );
}
