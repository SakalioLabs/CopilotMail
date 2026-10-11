import { useCallback, useEffect, useRef, useState } from "react";
import { getSubmission, requestSubmission, saveDraft } from "./api";
import type { MessageKey, TranslationValue } from "./i18n";
import type {
  DraftInput,
  HumanDraftListItem,
  HumanDraftRestore,
  MailDelivery,
  MailboxScopedConfig,
  MailSubmission,
} from "./types";

const recipients = (value: string) =>
  value.split(/[;,]/).map((item) => item.trim()).filter(Boolean);

const b64 = (bytes: Uint8Array) => {
  let value = "";
  bytes.forEach((byte) => { value += String.fromCharCode(byte); });
  return btoa(value).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};

const random = (size: number) => b64(crypto.getRandomValues(new Uint8Array(size)));

export interface ComposerStatus {
  key: MessageKey;
  params?: Record<string, TranslationValue>;
}

export interface ComposerReply {
  replyToMessageId: string;
  to: string;
  subject: string;
}

/**
 * The narrow, display-safe projection of a server-returned submission.
 *
 * It intentionally excludes the draft and mailbox bindings plus per-recipient
 * references and SMTP replies. Those fields are required for server-side
 * reconciliation, but are neither needed nor authorized as Human Mail UI
 * evidence. This projection is populated only from a parsed MailSubmission;
 * no identity, delegation, policy, or delivery state is inferred by the
 * browser.
 */
export interface ComposerSubmissionEvidence {
  submissionId: string;
  status: MailSubmission["status"];
  stateVersion: number;
  updatedAt: string;
  delivery: ComposerDeliveryEvidence | null;
}

export type ComposerDeliveryEvidence = Pick<
  MailDelivery,
  | "observed_at"
  | "terminal"
  | "recipient_count"
  | "delivered_count"
  | "failed_count"
  | "queued_count"
  | "unknown_count"
>;

function toComposerSubmissionEvidence(
  submission: MailSubmission,
): ComposerSubmissionEvidence {
  const delivery = submission.delivery;
  return {
    submissionId: submission.submission_id,
    status: submission.status,
    stateVersion: submission.state_version,
    updatedAt: submission.updated_at,
    delivery: delivery ? {
      observed_at: delivery.observed_at,
      terminal: delivery.terminal,
      recipient_count: delivery.recipient_count,
      delivered_count: delivery.delivered_count,
      failed_count: delivery.failed_count,
      queued_count: delivery.queued_count,
      unknown_count: delivery.unknown_count,
    } : null,
  };
}

export interface UseComposerSessionOptions {
  authenticated: boolean;
  mailboxSettings?: MailboxScopedConfig;
  canSend: boolean;
  /** Prevent new writes and polling while the browser Session is unverified. */
  sessionLocked?: boolean;
}

/**
 * Owns one human-composer session and the server-side draft/submission state
 * bound to it.  The parent view retains draft-directory pagination and restore
 * request cancellation, because those operations are scoped to a visible
 * mailbox/list rather than to the composer itself.
 */
export function useComposerSession({
  authenticated,
  mailboxSettings,
  canSend,
  sessionLocked = false,
}: UseComposerSessionOptions) {
  const [composeOpen, setComposeOpen] = useState(false);
  const [to, setTo] = useState("");
  const [cc, setCc] = useState("");
  const [bcc, setBcc] = useState("");
  const [subject, setSubject] = useState("");
  const [textBody, setTextBody] = useState("");
  const [replyToMessageId, setReplyToMessageId] = useState<string | null>(null);
  const [draftStatus, setDraftStatus] = useState<ComposerStatus>({
    key: "draft.autosaveHint",
  });
  const [composerKind, setComposerKind] = useState<"new" | "reply" | "restored">("new");
  const [composerInitialFocus, setComposerInitialFocus] = useState<"recipient" | "body">(
    "recipient",
  );
  const [sendBusy, setSendBusy] = useState(false);
  const [submissionLocked, setSubmissionLocked] = useState(false);
  const [terminalSubmission, setTerminalSubmission] = useState(false);
  const [manualReviewSubmissionId, setManualReviewSubmissionId] = useState<string>();
  const [submissionEvidence, setSubmissionEvidence] = useState<ComposerSubmissionEvidence>();

  const draftId = useRef<string | undefined>(undefined);
  const draftVersion = useRef<number | undefined>(undefined);
  const saveChain = useRef<Promise<void>>(Promise.resolve());
  const latestQueuedSave = useRef(0);
  const pendingSaveTimeout = useRef<number | undefined>(undefined);
  const deliveryCloseTimeout = useRef<number | undefined>(undefined);
  const submissionKey = useRef<string | undefined>(undefined);
  const submissionId = useRef<string | undefined>(undefined);
  const lastSubmission = useRef<MailSubmission | undefined>(undefined);
  const submissionRequestAttempted = useRef(false);
  const submissionLockedRef = useRef(false);
  const submissionTerminalRef = useRef(false);
  const manualReviewSubmissionIdRef = useRef<string | undefined>(undefined);
  const skipNextAutosave = useRef(false);
  const composerGeneration = useRef(0);
  // State updates do not synchronously guard a second call in the same frame.
  // Own one send/confirmation operation per composer generation; an old
  // operation's finally block must never release the next generation's lock.
  const sendFlightGeneration = useRef<number | undefined>(undefined);
  const sessionLockedRef = useRef(sessionLocked);
  const sessionUnlockWaiters = useRef(new Set<() => void>());
  sessionLockedRef.current = sessionLocked;

  const clearPendingSave = useCallback(() => {
    if (pendingSaveTimeout.current !== undefined) {
      window.clearTimeout(pendingSaveTimeout.current);
      pendingSaveTimeout.current = undefined;
    }
  }, []);

  const clearDeliveryClose = useCallback(() => {
    if (deliveryCloseTimeout.current !== undefined) {
      window.clearTimeout(deliveryCloseTimeout.current);
      deliveryCloseTimeout.current = undefined;
    }
  }, []);

  const releaseSessionUnlockWaiters = useCallback(() => {
    const waiters = [...sessionUnlockWaiters.current];
    sessionUnlockWaiters.current.clear();
    waiters.forEach((resolve) => resolve());
  }, []);

  const waitForSessionUnlock = useCallback(async (generation: number) => {
    while (
      sessionLockedRef.current
      && generation === composerGeneration.current
    ) {
      await new Promise<void>((resolve) => {
        sessionUnlockWaiters.current.add(resolve);
      });
    }
  }, []);

  const resetComposerPersistence = useCallback(() => {
    composerGeneration.current += 1;
    sendFlightGeneration.current = undefined;
    releaseSessionUnlockWaiters();
    clearPendingSave();
    clearDeliveryClose();
    draftId.current = undefined;
    draftVersion.current = undefined;
    submissionKey.current = undefined;
    submissionId.current = undefined;
    lastSubmission.current = undefined;
    submissionRequestAttempted.current = false;
    submissionLockedRef.current = false;
    submissionTerminalRef.current = false;
    manualReviewSubmissionIdRef.current = undefined;
    skipNextAutosave.current = false;
    setSubmissionLocked(false);
    setManualReviewSubmissionId(undefined);
    setSubmissionEvidence(undefined);
    latestQueuedSave.current = 0;
    saveChain.current = Promise.resolve();
  }, [clearDeliveryClose, clearPendingSave, releaseSessionUnlockWaiters]);

  const resetForNewMessage = useCallback(() => {
    resetComposerPersistence();
    setTo("");
    setCc("");
    setBcc("");
    setSubject("");
    setTextBody("");
    setReplyToMessageId(null);
    setDraftStatus({ key: "draft.autosaveHint" });
    setComposerKind("new");
    setComposerInitialFocus("recipient");
    setSendBusy(false);
    setTerminalSubmission(false);
  }, [resetComposerPersistence]);

  const clear = useCallback(() => {
    resetForNewMessage();
    setComposeOpen(false);
  }, [resetForNewMessage]);

  const queueDraftSave = useCallback((
    snapshot: Omit<DraftInput, "draft_id" | "expected_version">,
    allowSubmissionLock = false,
  ) => {
    if (
      !authenticated
      || !mailboxSettings
      || !canSend
      || sessionLockedRef.current
      || (submissionLockedRef.current && !allowSubmissionLock)
    ) {
      return Promise.reject(new Error("mailbox_not_writable"));
    }

    const config = mailboxSettings;
    const generation = composerGeneration.current;
    const sequence = ++latestQueuedSave.current;
    setDraftStatus({ key: "draft.saving" });
    const operation = saveChain.current
      .catch(() => undefined)
      .then(() => {
        if (generation !== composerGeneration.current) {
          throw new Error("stale_composer_session");
        }
        if (sessionLockedRef.current) {
          throw new Error("session_revalidation_pending");
        }
        return saveDraft(config, {
          draft_id: draftId.current ?? null,
          expected_version: draftVersion.current ?? null,
          ...snapshot,
        });
      })
      .then((result) => {
        if (generation !== composerGeneration.current) {
          throw new Error("stale_composer_session");
        }
        draftId.current = result.draft_id;
        draftVersion.current = result.version;
        if (sequence === latestQueuedSave.current) {
          setDraftStatus({
            key: result.previous_draft_destroyed
              ? "draft.saved"
              : "draft.savedCleanupFailed",
          });
        }
        return result;
      });
    saveChain.current = operation.then(() => undefined, () => undefined);
    return operation;
  }, [authenticated, canSend, mailboxSettings]);

  const hasAutosaveRecipient = recipients(to).length > 0;

  useEffect(() => {
    if (
      !composeOpen
      || !authenticated
      || !mailboxSettings
      || !canSend
      || sessionLocked
      || submissionLockedRef.current
      || hasAutosaveRecipient
    ) return;

    // Removing the last recipient makes the current editor snapshot
    // ineligible for persistence. Invalidate any in-flight status update so
    // that an older save cannot leave a misleading "saved" signal behind.
    latestQueuedSave.current += 1;
    setDraftStatus({ key: "draft.autosaveHint" });
  }, [
    authenticated,
    canSend,
    composeOpen,
    hasAutosaveRecipient,
    mailboxSettings,
    sessionLocked,
  ]);

  useEffect(() => {
    if (
      !composeOpen
      || !authenticated
      || !mailboxSettings
      || !canSend
      || sessionLocked
      || submissionLockedRef.current
      || !hasAutosaveRecipient
    ) return;
    if (skipNextAutosave.current) {
      skipNextAutosave.current = false;
      return;
    }

    const generation = composerGeneration.current;
    setDraftStatus({ key: "draft.waitingForAutosave" });
    const timeout = window.setTimeout(() => {
      if (pendingSaveTimeout.current === timeout) {
        pendingSaveTimeout.current = undefined;
      }
      if (submissionLockedRef.current) return;
      void queueDraftSave({
        reply_to_message_id: replyToMessageId,
        to: recipients(to),
        cc: recipients(cc),
        bcc: recipients(bcc),
        subject,
        text_body: textBody,
      }).catch(() => {
        if (
          generation === composerGeneration.current
          && !submissionLockedRef.current
          && !sessionLockedRef.current
        ) {
          setDraftStatus({ key: "draft.autosaveFailed" });
        }
      });
    }, 1200);
    pendingSaveTimeout.current = timeout;
    return () => {
      window.clearTimeout(timeout);
      if (pendingSaveTimeout.current === timeout) {
        pendingSaveTimeout.current = undefined;
      }
    };
  }, [
    authenticated,
    bcc,
    canSend,
    cc,
    composeOpen,
    hasAutosaveRecipient,
    mailboxSettings,
    queueDraftSave,
    replyToMessageId,
    sessionLocked,
    subject,
    textBody,
    to,
  ]);

  useEffect(() => {
    if (!sessionLocked) releaseSessionUnlockWaiters();
  }, [releaseSessionUnlockWaiters, sessionLocked]);

  useEffect(() => () => {
    composerGeneration.current += 1;
    sendFlightGeneration.current = undefined;
    clearPendingSave();
    clearDeliveryClose();
    releaseSessionUnlockWaiters();
  }, [clearDeliveryClose, clearPendingSave, releaseSessionUnlockWaiters]);

  const changeTo = useCallback((value: string) => {
    if (!submissionLockedRef.current) setTo(value);
  }, []);
  const changeCc = useCallback((value: string) => {
    if (!submissionLockedRef.current) setCc(value);
  }, []);
  const changeBcc = useCallback((value: string) => {
    if (!submissionLockedRef.current) setBcc(value);
  }, []);
  const changeSubject = useCallback((value: string) => {
    if (!submissionLockedRef.current) setSubject(value);
  }, []);
  const changeTextBody = useCallback((value: string) => {
    if (!submissionLockedRef.current) setTextBody(value);
  }, []);

  const hasContent = useCallback(() => Boolean(
    draftId.current
    || replyToMessageId
    || to
    || cc
    || bcc
    || subject
    || textBody,
  ), [bcc, cc, replyToMessageId, subject, textBody, to]);

  const hasUnresolvedSubmission = useCallback(
    () => submissionLockedRef.current && (
      !submissionTerminalRef.current
      || manualReviewSubmissionIdRef.current !== undefined
    ),
    [],
  );

  const restoreFromDraft = useCallback((restored: HumanDraftRestore) => {
    resetComposerPersistence();
    draftId.current = restored.draft_id;
    draftVersion.current = restored.version;
    skipNextAutosave.current = true;
    setTo(restored.to.join("; "));
    setCc(restored.cc.join("; "));
    setBcc(restored.bcc.join("; "));
    setSubject(restored.subject);
    setTextBody(restored.text_body);
    setReplyToMessageId(restored.reply_to_message_id);
    setComposerKind("restored");
    setComposerInitialFocus("body");
    setDraftStatus({ key: "draft.restored" });
    setSendBusy(false);
    submissionTerminalRef.current = false;
    setTerminalSubmission(false);
    setComposeOpen(true);
  }, [resetComposerPersistence]);

  const matchesRestoredDraft = useCallback((draft: HumanDraftListItem) =>
    draftId.current === draft.draft_id
      && draftVersion.current !== undefined
      && draftVersion.current >= draft.version, []);

  const reopenRestoredDraft = useCallback(() => {
    setComposerKind("restored");
    setComposerInitialFocus("body");
    setDraftStatus({ key: "draft.restored" });
    setComposeOpen(true);
  }, []);

  const observeSubmission = useCallback(async (initial: MailSubmission, generation: number) => {
    const config = mailboxSettings;
    if (!config || generation !== composerGeneration.current) return;
    let current = initial;
    if (
      current.mailbox_id !== config.mailboxId
      || current.draft_id !== draftId.current
      || current.draft_version !== draftVersion.current
    ) {
      throw new Error("submission_mailbox_mismatch");
    }

    submissionId.current = current.submission_id;
    lastSubmission.current = current;
    setSubmissionEvidence(toComposerSubmissionEvidence(current));
    for (let attempt = 0; attempt < 40; attempt += 1) {
      await waitForSessionUnlock(generation);
      if (generation !== composerGeneration.current) return;
      if (current.status === "accepted") {
        if (current.delivery?.terminal) {
          submissionTerminalRef.current = true;
          setTerminalSubmission(true);
          if (current.delivery.failed_count === 0) {
            setDraftStatus({
              key: "delivery.allDelivered",
              params: { count: current.delivery.delivered_count },
            });
            clearDeliveryClose();
            deliveryCloseTimeout.current = window.setTimeout(() => {
              if (generation === composerGeneration.current) {
                setComposeOpen(false);
              }
            }, 900);
          } else {
            setDraftStatus({
              key: "delivery.completed",
              params: {
                delivered: current.delivery.delivered_count,
                failed: current.delivery.failed_count,
              },
            });
          }
          return;
        }
        setDraftStatus(current.delivery
          ? {
              key: "delivery.acceptedProgress",
              params: {
                delivered: current.delivery.delivered_count,
                total: current.delivery.recipient_count,
              },
            }
          : { key: "delivery.acceptedPending" });
      } else if (current.status === "rejected") {
        setDraftStatus({ key: "delivery.rejected" });
        submissionTerminalRef.current = true;
        setTerminalSubmission(true);
        return;
      } else if (current.status === "manual_review") {
        setDraftStatus({ key: "delivery.manualReview" });
        manualReviewSubmissionIdRef.current = current.submission_id;
        setManualReviewSubmissionId(current.submission_id);
        submissionTerminalRef.current = true;
        setTerminalSubmission(true);
        return;
      } else {
        setDraftStatus({
          key: current.status === "pending"
            ? "delivery.queued"
            : "delivery.confirming",
        });
      }

      await new Promise<void>((resolve) => window.setTimeout(resolve, 1500));
      await waitForSessionUnlock(generation);
      if (generation !== composerGeneration.current) return;
      current = await getSubmission(config, current);
      if (generation !== composerGeneration.current) return;
      if (
        current.mailbox_id !== config.mailboxId
        || current.draft_id !== draftId.current
        || current.draft_version !== draftVersion.current
      ) {
        throw new Error("submission_mailbox_mismatch");
      }
      lastSubmission.current = current;
      setSubmissionEvidence(toComposerSubmissionEvidence(current));
    }
    setDraftStatus({ key: "delivery.stillProcessing" });
  }, [clearDeliveryClose, mailboxSettings, waitForSessionUnlock]);

  const send = useCallback(async () => {
    const config = mailboxSettings;
    if (
      !authenticated
      || !config
      || !canSend
      || sessionLockedRef.current
      || sendFlightGeneration.current !== undefined
      || submissionTerminalRef.current
      || recipients(to).length === 0
    ) return;

    const generation = composerGeneration.current;
    sendFlightGeneration.current = generation;
    const submissionSnapshot = {
      reply_to_message_id: replyToMessageId,
      to: recipients(to),
      cc: recipients(cc),
      bcc: recipients(bcc),
      subject,
      text_body: textBody,
    };
    submissionLockedRef.current = true;
    setSubmissionLocked(true);
    setSendBusy(true);
    clearPendingSave();
    try {
      let current: MailSubmission;
      if (submissionId.current) {
        if (!lastSubmission.current) throw new Error("submission_state_missing");
        if (sessionLockedRef.current) throw new Error("session_revalidation_pending");
        current = await getSubmission(config, lastSubmission.current);
      } else {
        if (!submissionRequestAttempted.current) {
          await queueDraftSave(submissionSnapshot, true);
        }
        if (!draftId.current || !draftVersion.current) throw new Error("draft_not_saved");
        if (sessionLockedRef.current) throw new Error("session_revalidation_pending");
        submissionKey.current ??= random(32);
        submissionRequestAttempted.current = true;
        current = await requestSubmission(
          config,
          draftId.current,
          draftVersion.current,
          submissionKey.current,
        );
      }
      await observeSubmission(current, generation);
    } catch (error) {
      if (generation === composerGeneration.current) {
        const submissionCouldExist = submissionRequestAttempted.current
          || submissionId.current !== undefined;
        if (!submissionCouldExist) {
          submissionLockedRef.current = false;
          submissionTerminalRef.current = false;
          setSubmissionLocked(false);
        }
        if (
          error instanceof Error
          && error.message === "session_revalidation_pending"
        ) {
          setDraftStatus({ key: draftId.current ? "draft.saved" : "draft.autosaveHint" });
          return;
        }
        setDraftStatus({
          key: submissionCouldExist
            ? "delivery.requestUnknown"
            : "delivery.draftSaveFailed",
        });
      }
    } finally {
      if (sendFlightGeneration.current === generation) sendFlightGeneration.current = undefined;
      if (generation === composerGeneration.current) setSendBusy(false);
    }
  }, [
    authenticated,
    bcc,
    canSend,
    cc,
    clearPendingSave,
    mailboxSettings,
    observeSubmission,
    queueDraftSave,
    replyToMessageId,
    subject,
    textBody,
    to,
  ]);

  const openComposer = useCallback(() => {
    if (!canSend) return false;
    if (hasUnresolvedSubmission()) {
      setComposeOpen(true);
      return true;
    }
    if (terminalSubmission || composerKind === "reply") {
      resetForNewMessage();
    }
    setComposeOpen(true);
    return true;
  }, [canSend, composerKind, hasUnresolvedSubmission, resetForNewMessage, terminalSubmission]);

  const reopenComposer = useCallback(() => {
    if (!canSend) return false;
    setComposeOpen(true);
    return true;
  }, [canSend]);

  const openReply = useCallback((reply: ComposerReply) => {
    if (!canSend) return false;
    if (hasUnresolvedSubmission()) {
      setComposeOpen(true);
      return true;
    }
    resetComposerPersistence();
    setTo(reply.to);
    setCc("");
    setBcc("");
    setSubject(reply.subject);
    setTextBody("");
    setReplyToMessageId(reply.replyToMessageId);
    setDraftStatus({ key: "draft.replyAutosaveHint" });
    setComposerKind("reply");
    setComposerInitialFocus("recipient");
    submissionTerminalRef.current = false;
    setTerminalSubmission(false);
    setComposeOpen(true);
    return true;
  }, [canSend, hasUnresolvedSubmission, resetComposerPersistence]);

  return {
    composeOpen,
    to,
    cc,
    bcc,
    subject,
    textBody,
    replyToMessageId,
    draftStatus,
    composerKind,
    composerInitialFocus,
    sendBusy,
    submissionLocked,
    terminalSubmission,
    manualReviewSubmissionId,
    submissionEvidence,
    submissionUnresolved: submissionLocked && (
      !terminalSubmission
      || manualReviewSubmissionId !== undefined
    ),
    changeTo,
    changeCc,
    changeBcc,
    changeSubject,
    changeTextBody,
    close: () => setComposeOpen(false),
    clear,
    hasContent,
    hasUnresolvedSubmission,
    restoreFromDraft,
    matchesRestoredDraft,
    reopenRestoredDraft,
    reopenComposer,
    openComposer,
    openReply,
    send,
  };
}
