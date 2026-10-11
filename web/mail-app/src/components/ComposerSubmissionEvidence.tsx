import { EvidenceInspector } from "@copilotmail/design-system";
import { formatCount, formatDate, type Locale, type Translator } from "../i18n";
import type { ComposerSubmissionEvidence as SubmissionEvidence } from "../useComposerSession";

const humanReviewIllustration = new URL(
  "../../../design-system/assets/illustrations/agent-human-review-v1.webp",
  import.meta.url,
).href;

const submissionStateKeys = {
  pending: "evidence.submissionState.pending",
  reconciling: "evidence.submissionState.reconciling",
  accepted: "evidence.submissionState.accepted",
  rejected: "evidence.submissionState.rejected",
  manual_review: "evidence.submissionState.manualReview",
} as const;

/**
 * A deliberately narrow, display-only view of the exact submission evidence
 * already authorized for the Human Mail composer. The source projection
 * excludes mailbox/draft bindings, per-recipient data and provider replies;
 * this component must not infer or add them.
 */
export function ComposerSubmissionEvidence({
  evidence,
  locale,
  t,
}: {
  evidence: SubmissionEvidence;
  locale: Locale;
  t: Translator;
}) {
  const submissionStateKey = submissionStateKeys[evidence.status];
  const delivery = evidence.delivery;

  return (
    <EvidenceInspector
      className="composer-evidence"
      summary={t("evidence.submissionSummary")}
    >
      {evidence.status === "manual_review" ? (
        <div className="composer-evidence__manual-review">
          <img
            alt={t("delivery.manualReviewIllustrationAlt")}
            src={humanReviewIllustration}
          />
          <div>
            <strong>{t("delivery.manualReviewTitle")}</strong>
            <p>{t("delivery.manualReviewNextStep")}</p>
          </div>
        </div>
      ) : null}
      <dl className="composer-evidence__facts">
        <div>
          <dt>{t("evidence.submissionId")}</dt>
          <dd><code>{evidence.submissionId}</code></dd>
        </div>
        <div>
          <dt>{t("evidence.submissionState")}</dt>
          <dd>{t(submissionStateKey)}</dd>
        </div>
        <div>
          <dt>{t("evidence.stateVersion")}</dt>
          <dd><code>{formatCount(locale, evidence.stateVersion)}</code></dd>
        </div>
        <div>
          <dt>{t("evidence.lastConfirmed")}</dt>
          <dd>
            <time dateTime={evidence.updatedAt}>
              {formatDate(locale, evidence.updatedAt, {
                dateStyle: "medium",
                timeStyle: "short",
              })}
            </time>
          </dd>
        </div>
        {delivery ? (
          <>
            <div>
              <dt>{t("evidence.deliveryObservedAt")}</dt>
              <dd>
                <time dateTime={delivery.observed_at}>
                  {formatDate(locale, delivery.observed_at, {
                    dateStyle: "medium",
                    timeStyle: "short",
                  })}
                </time>
              </dd>
            </div>
            <div>
              <dt>{t("evidence.deliveryTerminal")}</dt>
              <dd>{t(delivery.terminal
                ? "evidence.deliveryTerminal.yes"
                : "evidence.deliveryTerminal.no")}</dd>
            </div>
            <div>
              <dt>{t("evidence.deliveryRecipientCount")}</dt>
              <dd>{formatCount(locale, delivery.recipient_count)}</dd>
            </div>
            <div>
              <dt>{t("evidence.deliveryDelivered")}</dt>
              <dd>{formatCount(locale, delivery.delivered_count)}</dd>
            </div>
            <div>
              <dt>{t("evidence.deliveryFailed")}</dt>
              <dd>{formatCount(locale, delivery.failed_count)}</dd>
            </div>
            <div>
              <dt>{t("evidence.deliveryQueued")}</dt>
              <dd>{formatCount(locale, delivery.queued_count)}</dd>
            </div>
            <div>
              <dt>{t("evidence.deliveryUnknown")}</dt>
              <dd>{formatCount(locale, delivery.unknown_count)}</dd>
            </div>
          </>
        ) : null}
      </dl>
    </EvidenceInspector>
  );
}
