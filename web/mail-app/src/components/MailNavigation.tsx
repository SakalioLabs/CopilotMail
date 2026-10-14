import { useEffect } from "react";
import type { Locale, Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import {
  ContextBar,
  Dialog,
  Icon,
  IdentityBadge,
  StatusIndicator,
  type IconName,
} from "@copilotmail/design-system";
import { IdentityCardMenu } from "./IdentityCardMenu";
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
  unreadInboxCount?: number;
  onMailboxChange: (mailboxId: string) => void;
  onMailboxLoadMore: () => void;
  onCompose: () => void;
  onOpenCommandPalette: (trigger: HTMLElement) => void;
  onOpenSettings: (trigger: HTMLElement | null) => void;
  onViewChange: (view: MailView) => void;
  onSignOut: (trigger: HTMLElement | null) => void;
  t: Translator;
}

interface ViewNavigationProps {
  activeView: MailView;
  canCompose: boolean;
  locale: Locale;
  onViewChange: (view: MailView) => void;
  t: Translator;
  unreadInboxCount?: number;
}

const VIEW_ICONS: Record<MailView, IconName> = {
  inbox: "inbox",
  sent: "sent",
  drafts: "draft",
};

/**
 * Gmail-style view list shared by the desktop rail and the mobile drawer.
 * The inbox item announces its unread total through the accessible name and
 * repeats it as a decorative badge so the row stays a single scannable line.
 */
function MailViewNavigation({
  activeView,
  canCompose,
  locale,
  onViewChange,
  t,
  unreadInboxCount = 0,
}: ViewNavigationProps) {
  const views: Array<{ view: MailView; label: string; disabled: boolean }> = [
    { view: "inbox", label: t("nav.inbox"), disabled: false },
    { view: "sent", label: t("nav.sent"), disabled: !canCompose },
    { view: "drafts", label: t("nav.drafts"), disabled: !canCompose },
  ];

  return (
    <nav aria-label={t("nav.mailboxLabel")}>
      {views.map(({ view, label, disabled }) => {
        const active = activeView === view;
        return (
          <button
            key={view}
            className={["app-nav-item", active ? "active" : ""]
              .filter(Boolean)
              .join(" ")}
            aria-current={active ? "page" : undefined}
            aria-label={
              view === "inbox" && unreadInboxCount > 0
                ? t("nav.inboxUnread", { count: unreadInboxCount })
                : undefined
            }
            disabled={disabled}
            title={disabled ? t("mailbox.composeUnavailable") : undefined}
            onClick={() => onViewChange(view)}
          >
            <Icon aria-hidden name={VIEW_ICONS[view]} size={18} />
            <span className="app-nav-item__label">{label}</span>
            {view === "inbox" && unreadInboxCount > 0 ? (
              <span aria-hidden="true" className="app-nav-item__badge">
                {unreadInboxCount.toLocaleString(locale)}
              </span>
            ) : null}
          </button>
        );
      })}
    </nav>
  );
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

function MailboxScopeControls({
  activeMailbox,
  mailboxes,
  mailboxHasMore,
  mailboxLoadMoreBusy,
  mailboxLoadMoreError,
  sessionLocked = false,
  onMailboxChange,
  onMailboxLoadMore,
  t,
}: MailNavigationProps) {
  return (
    <>
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
    </>
  );
}

export function DesktopNavigation(props: MailNavigationProps) {
  const {
    activeMailbox,
    mailboxes,
    mailboxHasMore,
    mailboxLoadMoreBusy,
    mailboxLoadMoreError,
    canCompose,
    sessionLocked = false,
    activeView,
    locale,
    unreadInboxCount,
    onMailboxChange,
    onMailboxLoadMore,
    onOpenCommandPalette,
    onOpenSettings,
    onSignOut,
    onViewChange,
    t,
  } = props;

  return (
    <aside className="sidebar" data-dialog-background>
      <MailViewNavigation
        activeView={activeView}
        canCompose={canCompose}
        locale={locale}
        unreadInboxCount={unreadInboxCount}
        onViewChange={onViewChange}
        t={t}
      />
      <div className="sidebar__mailbox">
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
        <MailboxIdentity activeMailbox={activeMailbox} t={t} />
      </div>
      <button
        className="command-trigger"
        aria-label={t("command.open")}
        onClick={(event) => onOpenCommandPalette(event.currentTarget)}
      >
        <Icon aria-hidden name="command" size={17} />
        <span>{t("command.title")}</span>
        <kbd aria-hidden="true">{t("command.triggerShortcut")}</kbd>
      </button>
      <IdentityCardMenu
        activeMailbox={activeMailbox}
        sessionLocked={sessionLocked}
        t={t}
        onOpenSettings={onOpenSettings}
        onSignOut={onSignOut}
      />
    </aside>
  );
}

interface MobileNavigationProps extends MailNavigationProps {
  drawerOpen: boolean;
  onDrawerClose: () => void;
}

/**
 * Floating compose action plus the narrow-viewport left drawer that carries
 * the same view list, mailbox scope, and session controls as the desktop
 * rail. The drawer closes itself if the viewport grows past the mobile
 * breakpoint while it is open.
 */
export function MobileNavigation(props: MobileNavigationProps) {
  const {
    canCompose,
    composeBusy = false,
    activeView,
    locale,
    unreadInboxCount,
    drawerOpen,
    onDrawerClose,
    onCompose,
    onOpenCommandPalette,
    onViewChange,
    t,
  } = props;

  useEffect(() => {
    if (!drawerOpen || typeof window.matchMedia !== "function") return;
    const media = window.matchMedia("(min-width: 1008px)");
    if (media.matches) {
      onDrawerClose();
      return;
    }
    const closeWhenWide = (event: MediaQueryListEvent) => {
      if (event.matches) onDrawerClose();
    };
    media.addEventListener("change", closeWhenWide);
    return () => media.removeEventListener("change", closeWhenWide);
  }, [drawerOpen, onDrawerClose]);

  const selectView = (view: MailView) => {
    onViewChange(view);
    onDrawerClose();
  };

  return (
    <>
      <button
        className="compose-fab"
        aria-label={t("nav.compose")}
        disabled={!canCompose || composeBusy}
        title={!canCompose ? t("mailbox.composeUnavailable") : undefined}
        onClick={onCompose}
      >
        <Icon aria-hidden name="compose" size={22} />
      </button>
      {drawerOpen ? (
        <Dialog
          backdropClassName="mail-nav-backdrop"
          className="mail-nav-drawer"
          closeLabel={t("nav.closeMenu")}
          title={t("nav.drawerTitle")}
          onClose={onDrawerClose}
        >
          <div className="mail-nav-drawer__body">
            <MailViewNavigation
              activeView={activeView}
              canCompose={canCompose}
              locale={locale}
              unreadInboxCount={unreadInboxCount}
              onViewChange={selectView}
              t={t}
            />
            <details className="mobile-mailbox-scope">
              <MobileMailboxScopeSummary
                activeMailbox={props.activeMailbox}
                t={t}
              />
              <div className="mobile-mailbox-scope__content">
                <MailboxScopeControls {...props} />
              </div>
            </details>
            <button
              className="command-trigger mobile-command-trigger"
              aria-label={t("command.open")}
              onClick={(event) => {
                onOpenCommandPalette(event.currentTarget);
                onDrawerClose();
              }}
            >
              <Icon aria-hidden name="command" size={17} />
              <span>{t("command.title")}</span>
              <kbd aria-hidden="true">{t("command.triggerShortcut")}</kbd>
            </button>
            <IdentityCardMenu
              activeMailbox={props.activeMailbox}
              sessionLocked={props.sessionLocked}
              t={t}
              onOpenSettings={(trigger) => {
                // The drawer unmounts as the settings dialog opens, so focus
                // returns to the always-mounted top bar menu button instead
                // of the about-to-be-removed card.
                const home = document.querySelector<HTMLElement>(".mail-topbar__menu");
                onDrawerClose();
                props.onOpenSettings(home ?? trigger);
              }}
              onSignOut={(trigger) => {
                const home = document.querySelector<HTMLElement>(".mail-topbar__menu");
                onDrawerClose();
                props.onSignOut(home ?? trigger);
              }}
            />
          </div>
        </Dialog>
      ) : null}
    </>
  );
}
