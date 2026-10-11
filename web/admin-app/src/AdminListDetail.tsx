import { ArrowLeftIcon, Button, Panel } from "@copilotmail/design-system";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefCallback,
  type RefObject,
} from "react";

export const ADMIN_LIST_DETAIL_MEDIA_QUERY = "(max-width: 1080px)";

export type RegisterAdminListDetailTrigger = (
  selectionKey: string,
) => RefCallback<HTMLElement>;

export interface AdminListDetailProps {
  /** The selected record identity. `null` means the detail has no selection. */
  selectionKey: string | null;
  /** Localized visible label for the narrow-layout return action. */
  backLabel: string;
  /** Locks Back and Escape while a detail operation is in flight. */
  navigationLocked?: boolean;
  /** Optional context rendered immediately above the selected detail title. */
  detailHeader?: ReactNode;
  /** Stable, focusable h2 content for the selected record. */
  detailTitle: ReactNode;
  /** Optional owner ref for explicit post-mutation focus recovery. */
  detailTitleRef?: RefObject<HTMLHeadingElement | null>;
  /** Status or actions aligned with the selected detail title. */
  detailTrailing?: ReactNode;
  /** Selected record content. */
  detailBody: ReactNode;
  /** Truthful no-selection state shown on desktop. */
  detailEmpty: ReactNode;
  /**
   * Renders the list. Register each unique row trigger so Back/Escape can
   * restore focus to the exact record that opened the narrow detail view.
   *
   * @example
   * renderList={(registerTrigger) => items.map((item) => (
   *   <button ref={registerTrigger(item.id)} onClick={() => select(item.id)}>
   *     {item.name}
   *   </button>
   * ))}
   */
  renderList: (registerTrigger: RegisterAdminListDetailTrigger) => ReactNode;
  /** Clears the controlled selection. */
  onBack: () => void;
  className?: string;
  listClassName?: string;
  detailClassName?: string;
  listLabel?: string;
}

interface TriggerRegistration {
  node: HTMLElement | null;
  ref: RefCallback<HTMLElement>;
}

function isNarrowViewport(): boolean {
  return typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia(ADMIN_LIST_DETAIL_MEDIA_QUERY).matches;
}

function hasOpenModalDialog(): boolean {
  return Boolean(document.querySelector("[role='dialog'][aria-modal='true']"));
}

/**
 * Admin-only master/detail navigation shell. CSS owns the same 1080px
 * breakpoint as the JavaScript media query; JavaScript is used only for
 * accessibility behavior (visibility state, focus, and Escape navigation).
 */
export function AdminListDetail({
  selectionKey,
  backLabel,
  navigationLocked = false,
  detailHeader,
  detailTitle,
  detailTitleRef: ownerDetailTitleRef,
  detailTrailing,
  detailBody,
  detailEmpty,
  renderList,
  onBack,
  className = "",
  listClassName = "",
  detailClassName = "",
  listLabel,
}: AdminListDetailProps) {
  const [narrow, setNarrow] = useState(isNarrowViewport);
  const listPanelId = useId();
  const detailTitleId = useId();
  const internalDetailTitleRef = useRef<HTMLHeadingElement>(null);
  const triggerRegistrations = useRef(new Map<string, TriggerRegistration>());
  const pendingFocusRestore = useRef<string | null>(null);
  const selected = selectionKey !== null;
  const setDetailTitleRef = useCallback((node: HTMLHeadingElement | null) => {
    internalDetailTitleRef.current = node;
    if (ownerDetailTitleRef) ownerDetailTitleRef.current = node;
  }, [ownerDetailTitleRef]);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;

    const mediaQuery = window.matchMedia(ADMIN_LIST_DETAIL_MEDIA_QUERY);
    const update = () => setNarrow(mediaQuery.matches);
    update();
    mediaQuery.addEventListener("change", update);
    return () => mediaQuery.removeEventListener("change", update);
  }, []);

  const registerTrigger = useCallback<RegisterAdminListDetailTrigger>((key) => {
    const existing = triggerRegistrations.current.get(key);
    if (existing) return existing.ref;

    const registration: TriggerRegistration = {
      node: null,
      ref: () => undefined,
    };
    registration.ref = (node) => {
      registration.node = node;
      if (node !== null) {
        triggerRegistrations.current.set(key, registration);
      } else if (triggerRegistrations.current.get(key) === registration) {
        triggerRegistrations.current.delete(key);
      }
    };
    triggerRegistrations.current.set(key, registration);
    return registration.ref;
  }, []);

  const returnToList = useCallback(() => {
    if (!narrow || !selected || navigationLocked || selectionKey === null) return;
    pendingFocusRestore.current = selectionKey;
    onBack();
  }, [narrow, navigationLocked, onBack, selected, selectionKey]);

  useLayoutEffect(() => {
    if (!narrow) {
      pendingFocusRestore.current = null;
      return;
    }

    if (selectionKey !== null) {
      if (
        pendingFocusRestore.current !== null
        && pendingFocusRestore.current !== selectionKey
      ) {
        pendingFocusRestore.current = null;
      }
      if (!hasOpenModalDialog()) internalDetailTitleRef.current?.focus();
      return;
    }

    const restoreKey = pendingFocusRestore.current;
    if (restoreKey === null) return;
    pendingFocusRestore.current = null;
    const trigger = triggerRegistrations.current.get(restoreKey)?.node;
    const fallback = document.getElementById(listPanelId);
    if (trigger?.isConnected) trigger.focus();
    else fallback?.focus();
  }, [listPanelId, narrow, selectionKey]);

  useEffect(() => {
    if (!narrow || !selected || navigationLocked) return;

    const handleEscape = (event: KeyboardEvent) => {
      if (
        event.key !== "Escape"
        || event.defaultPrevented
        || hasOpenModalDialog()
      ) return;
      event.preventDefault();
      event.stopPropagation();
      returnToList();
    };

    // Bubble phase allows a portalled modal's capture handler to consume
    // Escape first. The explicit modal check also protects synthetic events
    // dispatched directly on document in tests and host integrations.
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [narrow, navigationLocked, returnToList, selected]);

  return (
    <div
      aria-busy={navigationLocked || undefined}
      className={`master-detail admin-list-detail ${className}`.trim()}
      data-detail-open={selected ? "true" : "false"}
      data-navigation-locked={navigationLocked ? "true" : "false"}
    >
      <Panel
        aria-label={listLabel}
        className={`list-panel admin-list-detail__list ${listClassName}`.trim()}
        hidden={narrow && selected}
        id={listPanelId}
        tabIndex={-1}
      >
        {renderList(registerTrigger)}
      </Panel>
      <Panel
        aria-labelledby={selected ? detailTitleId : undefined}
        className={`detail-panel admin-list-detail__detail ${detailClassName}`.trim()}
        hidden={narrow && !selected}
      >
        {selected ? (
          <>
            <Button
              className="admin-list-detail__back"
              disabled={navigationLocked}
              hidden={!narrow}
              type="button"
              variant="quiet"
              onClick={returnToList}
            >
              <ArrowLeftIcon aria-hidden size={17} />
              {backLabel}
            </Button>
            <div className="detail-panel__heading admin-list-detail__heading">
              <div>
                {detailHeader ? (
                  <div className="admin-list-detail__context">{detailHeader}</div>
                ) : null}
                <h2
                  ref={setDetailTitleRef}
                  className="admin-list-detail__title"
                  id={detailTitleId}
                  tabIndex={-1}
                >
                  {detailTitle}
                </h2>
              </div>
              {detailTrailing ? (
                <div className="admin-list-detail__trailing">{detailTrailing}</div>
              ) : null}
            </div>
            <div className="admin-list-detail__body">{detailBody}</div>
          </>
        ) : detailEmpty}
      </Panel>
    </div>
  );
}
