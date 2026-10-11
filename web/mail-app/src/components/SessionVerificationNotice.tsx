import { forwardRef } from "react";
import { Icon } from "@copilotmail/design-system";

type SessionVerificationNoticeProps =
  | {
      readonly state: "checking";
      readonly message: string;
    }
  | {
      readonly state: "failed";
      readonly message: string;
      readonly retryLabel: string;
      readonly signOutLabel: string;
      readonly onRetry: () => void;
      readonly onSignOut: () => void;
    };

/**
 * A non-modal session-trust notice. The controller owns verification and
 * focus restoration; this component only presents the current fact and the
 * two existing recovery actions after verification fails.
 */
export const SessionVerificationNotice = forwardRef<HTMLElement, SessionVerificationNoticeProps>(
  function SessionVerificationNotice(props, ref) {
    const failed = props.state === "failed";
    return (
      <section
        ref={ref}
        aria-live={failed ? "assertive" : "polite"}
        className={`session-check session-check--${props.state}`}
        data-session-verification={props.state}
        role={failed ? "alert" : "status"}
        tabIndex={failed ? -1 : undefined}
      >
        <Icon
          aria-hidden="true"
          name={failed ? "status-critical" : "status-neutral"}
          size={20}
        />
        <p>{props.message}</p>
        {failed ? (
          <div className="session-check__actions">
            <button className="button" type="button" onClick={props.onRetry}>
              {props.retryLabel}
            </button>
            <button className="button button--quiet" type="button" onClick={props.onSignOut}>
              {props.signOutLabel}
            </button>
          </div>
        ) : null}
      </section>
    );
  },
);

SessionVerificationNotice.displayName = "SessionVerificationNotice";
