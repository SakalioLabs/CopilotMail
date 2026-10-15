import { useEffect } from "react";
import type { Locale, Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import {
  Dialog,
  Icon,
  type IconName,
} from "@copilotmail/design-system";
import { IdentityCardMenu } from "./IdentityCardMenu";

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
        mailboxes={mailboxes}
        mailboxHasMore={mailboxHasMore}
        mailboxLoadMoreBusy={mailboxLoadMoreBusy}
        mailboxLoadMoreError={mailboxLoadMoreError}
        sessionLocked={sessionLocked}
        t={t}
        onMailboxChange={onMailboxChange}
        onMailboxLoadMore={onMailboxLoadMore}
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
              mailboxes={props.mailboxes}
              mailboxHasMore={props.mailboxHasMore}
              mailboxLoadMoreBusy={props.mailboxLoadMoreBusy}
              mailboxLoadMoreError={props.mailboxLoadMoreError}
              sessionLocked={props.sessionLocked}
              t={t}
              onMailboxChange={(mailboxId) => {
                // Selecting a mailbox reloads the workspace; close the drawer
                // and park focus on the top bar menu button, like the other
                // drawer-bound account actions.
                document.querySelector<HTMLElement>(".mail-topbar__menu")?.focus();
                onDrawerClose();
                props.onMailboxChange(mailboxId);
              }}
              onMailboxLoadMore={props.onMailboxLoadMore}
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
