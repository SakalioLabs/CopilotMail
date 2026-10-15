import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import {
  Icon,
  IdentityBadge,
  StatusIndicator,
} from "@copilotmail/design-system";
import type { Translator } from "../i18n";
import type { MailboxDirectoryItem, MailboxKind } from "../types";

const MAILBOX_KIND_ORDER = ["human", "agent", "shared", "group"] as const satisfies
  readonly MailboxKind[];

interface IdentityCardMenuProps {
  activeMailbox: MailboxDirectoryItem;
  mailboxes: readonly MailboxDirectoryItem[];
  mailboxHasMore: boolean;
  mailboxLoadMoreBusy: boolean;
  mailboxLoadMoreError?: string;
  sessionLocked?: boolean;
  t: Translator;
  onMailboxChange: (mailboxId: string) => void;
  onMailboxLoadMore: () => void;
  onOpenSettings: (trigger: HTMLElement | null) => void;
  onSignOut: (trigger: HTMLElement | null) => void;
}

function menuItems(menu: HTMLElement | null) {
  return Array.from(
    menu?.querySelectorAll<HTMLElement>("[role='menuitem'], [role='menuitemradio']") ?? [],
  );
}

/**
 * Account identity card at the bottom of the navigation rail with the
 * Apple-style popover menu. The mailbox directory remains the only identity
 * authority: the display name is the primary address local part and the
 * address itself is rendered verbatim. The popover is a real `menu` pattern —
 * Enter/Space opens it, arrows cycle items, Escape closes and returns focus,
 * and an outside press dismisses it.
 *
 * Mailbox switching lives inside this menu as grouped `menuitemradio` rows so
 * the identity, sendability, and directory facts stay attached to the account
 * surface instead of a permanent rail section.
 */
export function IdentityCardMenu({
  activeMailbox,
  mailboxes,
  mailboxHasMore,
  mailboxLoadMoreBusy,
  mailboxLoadMoreError,
  sessionLocked = false,
  t,
  onMailboxChange,
  onMailboxLoadMore,
  onOpenSettings,
  onSignOut,
}: IdentityCardMenuProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const initialFocus = useRef<"first" | "last">("first");
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const address = activeMailbox.primary_address;
  const displayName = address.split("@", 1)[0] || address;
  const mailboxGroups = MAILBOX_KIND_ORDER.map((kind) => ({
    kind,
    items: mailboxes.filter((mailbox) => mailbox.kind === kind),
  })).filter((group) => group.items.length > 0);

  const closeMenu = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const items = menuItems(menuRef.current);
    const target = initialFocus.current === "last" ? items.at(-1) : items[0];
    target?.focus();

    const onPointerDown = (event: PointerEvent) => {
      if (
        event.target instanceof Node
        && !rootRef.current?.contains(event.target)
      ) {
        setOpen(false);
      }
    };
    const onEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onEscape, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onEscape, true);
    };
  }, [open]);

  const onTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      initialFocus.current = "first";
      setOpen(true);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      initialFocus.current = "last";
      setOpen(true);
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    const items = menuItems(menuRef.current);
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement as HTMLElement);
    let next: number | undefined;
    if (event.key === "ArrowDown") {
      next = index < 0 ? 0 : (index + 1) % items.length;
    } else if (event.key === "ArrowUp") {
      next = index < 0 ? items.length - 1 : (index - 1 + items.length) % items.length;
    } else if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = items.length - 1;
    }
    if (next === undefined) return;
    event.preventDefault();
    items[next].focus();
  };

  const activate = (action: (trigger: HTMLElement | null) => void) => {
    const trigger = triggerRef.current;
    setOpen(false);
    action(trigger);
  };

  const switchMailbox = (mailboxId: string) => {
    setOpen(false);
    triggerRef.current?.focus();
    onMailboxChange(mailboxId);
  };

  return (
    <div ref={rootRef} className="identity-menu-wrap">
      <button
        ref={triggerRef}
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="menu"
        className="identity-card"
        disabled={sessionLocked}
        type="button"
        onClick={() => {
          initialFocus.current = "first";
          setOpen((current) => !current);
        }}
        onKeyDown={onTriggerKeyDown}
      >
        <span aria-hidden="true" className="identity-card__avatar">
          {displayName.slice(0, 1).toUpperCase()}
        </span>
        <span className="identity-card__text">
          <b className="identity-card__name">{displayName}</b>
          <span className="identity-card__email" title={address}>{address}</span>
        </span>
        <Icon
          aria-hidden
          className="identity-card__chevron"
          name="chevron-down"
          size={16}
        />
      </button>
      {open ? (
        <div
          ref={menuRef}
          aria-label={t("identity.menuLabel")}
          aria-orientation="vertical"
          className="identity-menu"
          id={menuId}
          role="menu"
          onKeyDown={onMenuKeyDown}
        >
          <div
            aria-label={t("mailbox.selectorAria")}
            className="identity-menu__mailbox-section"
            role="group"
          >
            <div className="identity-menu__mailbox-status">
              <IdentityBadge
                kind={activeMailbox.kind}
                label={t(`mailbox.kind.${activeMailbox.kind}`)}
              />
              <StatusIndicator
                className="identity-menu__mailbox-sendable"
                data-sendable={activeMailbox.can_send}
                label={activeMailbox.can_send
                  ? t("mailbox.canSend")
                  : t("mailbox.readOnly")}
                tone={activeMailbox.can_send ? "positive" : "warning"}
              />
            </div>
            {mailboxGroups.map(({ kind, items }) => (
              <div
                aria-label={t(`mailbox.group.${kind}`)}
                key={kind}
                role="group"
              >
                <div aria-hidden="true" className="identity-menu__group-label">
                  {t(`mailbox.group.${kind}`)}
                </div>
                {items.map((mailbox) => {
                  const active = mailbox.mailbox_id === activeMailbox.mailbox_id;
                  return (
                    <button
                      key={mailbox.mailbox_id}
                      aria-checked={active}
                      className="identity-menu__item identity-menu__mailbox"
                      disabled={sessionLocked}
                      role="menuitemradio"
                      type="button"
                      onClick={() => switchMailbox(mailbox.mailbox_id)}
                    >
                      {active ? (
                        <Icon
                          aria-hidden
                          className="identity-menu__mailbox-check"
                          name="check"
                          size={14}
                        />
                      ) : (
                        <span aria-hidden className="identity-menu__mailbox-check" />
                      )}
                      <span className="identity-menu__mailbox-address">
                        {mailbox.primary_address}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))}
            {mailboxHasMore ? (
              <button
                className="identity-menu__item"
                disabled={sessionLocked || mailboxLoadMoreBusy}
                role="menuitem"
                type="button"
                onClick={onMailboxLoadMore}
              >
                {mailboxLoadMoreBusy
                  ? t("mailbox.loadingMore")
                  : t("mailbox.loadMore")}
              </button>
            ) : null}
            {mailboxLoadMoreError ? (
              <div className="identity-menu__error" role="alert">
                {mailboxLoadMoreError}
              </div>
            ) : null}
          </div>
          <div className="identity-menu__separator" role="separator" />
          <button
            className="identity-menu__item"
            role="menuitem"
            type="button"
            onClick={() => activate(onOpenSettings)}
          >
            {t("identity.settings")}
          </button>
          <button
            className="identity-menu__item"
            role="menuitem"
            type="button"
            onClick={() => activate(onSignOut)}
          >
            {t("nav.signOut")}
          </button>
        </div>
      ) : null}
    </div>
  );
}
