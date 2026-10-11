import { Icon } from "@copilotmail/design-system";

export type MailAccessSurfaceKind = "login" | "directory";

export interface MailAccessAction {
  readonly label: string;
  readonly onPress: () => void;
  readonly disabled?: boolean;
  readonly appearance?: "primary" | "quiet";
}

export interface MailAccessNotice {
  readonly text: string;
  /** Leave undefined when a higher-level session notice owns the live region. */
  readonly role?: "alert" | "status";
}

interface MailAccessSurfaceProps {
  /** Only the unauthenticated entry path may show the registered login visual. */
  readonly kind: MailAccessSurfaceKind;
  readonly title: string;
  readonly description?: string;
  readonly notice?: MailAccessNotice;
  readonly actions?: readonly MailAccessAction[];
  /** The controller keeps the existing fail-closed interaction lock. */
  readonly locked?: boolean;
}

const loginHero = new URL(
  "../../../design-system/assets/illustrations/mail-agent-routing-v1.webp",
  import.meta.url,
).href;

/**
 * A pure, field-minimal surface for entering Human Mail or resolving its
 * server-authoritative mailbox scope. It owns no authentication, directory,
 * storage, or request behavior; callers pass only translated facts and their
 * already-authorized actions.
 */
export function MailAccessSurface({
  actions = [],
  description,
  kind,
  locked = false,
  notice,
  title,
}: MailAccessSurfaceProps) {
  return (
    <main
      aria-busy={locked || undefined}
      className={`mail-access mail-access--${kind}`}
      data-mail-access={kind}
      inert={locked || undefined}
    >
      {kind === "login" ? (
        <aside className="mail-access__visual" aria-hidden="true">
          <img src={loginHero} alt="" />
        </aside>
      ) : null}
      <section className="mail-access__content">
        <div className="mail-access__panel">
          <span className="mail-access__mark mark" aria-hidden="true">
            <Icon name="mail" size={19} />
          </span>
          <h1>{title}</h1>
          {description ? <p className="mail-access__description">{description}</p> : null}
          {notice ? (
            <p className="mail-access__notice" role={notice.role}>
              {notice.text}
            </p>
          ) : null}
          {actions.length > 0 ? (
            <div className="mail-access__actions">
              {actions.map((action) => (
                <button
                  className={[
                    "button",
                    "mail-access__action",
                    action.appearance === "quiet" ? "button--quiet" : "",
                  ].filter(Boolean).join(" ")}
                  disabled={action.disabled}
                  key={action.label}
                  type="button"
                  onClick={action.onPress}
                >
                  {action.label}
                </button>
              ))}
            </div>
          ) : null}
        </div>
      </section>
    </main>
  );
}
