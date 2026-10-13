import { useRef, type ChangeEvent } from "react";
import {
  ContextBar,
  Dialog,
  Icon,
  IdentityBadge,
  Input,
  StatusIndicator,
  Textarea,
  type BadgeTone,
} from "@copilotmail/design-system";
import { type Locale, type Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import type { ComposerSubmissionEvidence } from "../useComposerSession";
import { ComposerSubmissionEvidence as ComposerSubmissionEvidencePanel } from "./ComposerSubmissionEvidence";

const hasRecipient = (value: string) =>
  value.split(/[;,]/).some((recipient) => recipient.trim().length > 0);

export interface ComposerDialogProps {
  t: Translator;
  locale: Locale;
  replyMode: boolean;
  restoredMode?: boolean;
  initialFocus?: "recipient" | "body";
  /**
   * The authenticated mailbox directory is the sole source of the mail
   * identity shown in the composer. Do not add client-inferred actor,
   * delegation, owner, or authorization fields here.
   */
  mailbox: MailboxDirectoryItem;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  textBody: string;
  draftStatus: string;
  draftStatusTone: BadgeTone;
  /** Safe projection from an exact, server-returned submission state. */
  submissionEvidence?: ComposerSubmissionEvidence;
  submissionLocked: boolean;
  sendBusy: boolean;
  terminalSubmission: boolean;
  onToChange: (value: string) => void;
  onCcChange: (value: string) => void;
  onBccChange: (value: string) => void;
  onSubjectChange: (value: string) => void;
  onTextBodyChange: (value: string) => void;
  onClose: () => void;
  onSend: () => void | Promise<void>;
}

export function ComposerDialog({
  t,
  replyMode,
  locale,
  restoredMode = false,
  initialFocus = "recipient",
  mailbox,
  to,
  cc,
  bcc,
  subject,
  textBody,
  draftStatus,
  draftStatusTone,
  submissionEvidence,
  submissionLocked,
  sendBusy,
  terminalSubmission,
  onToChange,
  onCcChange,
  onBccChange,
  onSubjectChange,
  onTextBodyChange,
  onClose,
  onSend,
}: ComposerDialogProps) {
  const recipientRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const title = t(restoredMode
    ? "compose.editDraftTitle"
    : replyMode
      ? "compose.replyTitle"
      : "compose.newTitle");

  const valueFrom = (callback: (value: string) => void) =>
    (event: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      callback(event.target.value);
    };

  return (
    <Dialog
      backdropClassName="dialog-layer"
      busy={sendBusy}
      className="composer"
      closeLabel={t("compose.closeAria")}
      dismissDisabled={sendBusy}
      initialFocusRef={initialFocus === "body" ? bodyRef : recipientRef}
      title={title}
      onClose={onClose}
    >
      {replyMode && <p className="reply-context">{t("compose.replyContext")}</p>}
      <ContextBar
        className="composer-context"
        facts={[
          {
            id: "scope",
            label: t("compose.scopeLabel"),
            value: (
              <span className="composer-context__address" title={mailbox.primary_address}>
                {mailbox.primary_address}
              </span>
            ),
          },
          {
            id: "identity",
            label: t("compose.identityFactLabel"),
            value: (
              <IdentityBadge
                kind={mailbox.kind}
                label={t(`mailbox.kind.${mailbox.kind}`)}
              />
            ),
          },
        ]}
        headingLevel={3}
        status={(
          <StatusIndicator
            data-sendable={mailbox.can_send ? "true" : "false"}
            label={t(mailbox.can_send
              ? "compose.serverSendAllowed"
              : "compose.serverReadOnly")}
            tone={mailbox.can_send ? "positive" : "warning"}
          />
        )}
        title={t("compose.contextTitle")}
      />
      <label>
        {t("compose.to")}
        <Input
          ref={recipientRef}
          disabled={submissionLocked}
          value={to}
          onChange={valueFrom(onToChange)}
          placeholder={t("compose.toPlaceholder")}
        />
      </label>
      <details className="composer-recipients">
        <summary>{t("compose.ccBcc")}</summary>
        <label>
          {t("compose.cc")}
          <Input
            disabled={submissionLocked}
            value={cc}
            onChange={valueFrom(onCcChange)}
          />
        </label>
        <label>
          {t("compose.bcc")}
          <Input
            disabled={submissionLocked}
            value={bcc}
            onChange={valueFrom(onBccChange)}
          />
        </label>
      </details>
      <label>
        {t("compose.subject")}
        <Input
          disabled={submissionLocked}
          value={subject}
          maxLength={998}
          onChange={valueFrom(onSubjectChange)}
        />
      </label>
      <label className="body-label">
        <span className="sr-only">{t("compose.body")}</span>
        <Textarea
          ref={bodyRef}
          disabled={submissionLocked}
          value={textBody}
          maxLength={262_144}
          onChange={valueFrom(onTextBodyChange)}
          placeholder={t("compose.bodyPlaceholder")}
        />
      </label>
      {submissionEvidence ? (
        <ComposerSubmissionEvidencePanel
          evidence={submissionEvidence}
          locale={locale}
          t={t}
        />
      ) : null}
      <footer>
        <StatusIndicator
          className="composer-status"
          label={draftStatus}
          live="polite"
          tone={draftStatusTone}
        />
        <button
          type="button"
          className="button"
          disabled={sendBusy || terminalSubmission || !mailbox.can_send || !hasRecipient(to)}
          onClick={() => {
            void onSend();
          }}
        >
          <Icon aria-hidden name="send" size={17} />
          {sendBusy
            ? t("compose.sending")
            : terminalSubmission
              ? t("compose.sendLocked")
              : submissionLocked
              ? t("compose.retrySend")
              : t("compose.send")}
        </button>
      </footer>
    </Dialog>
  );
}
