import type { RefCallback } from "react";
import { Icon } from "@copilotmail/design-system";
import { formatCount, formatDate, type Locale, type Translator } from "../i18n";
import type { HumanDraftListItem } from "../types";

interface DraftDirectoryItemProps {
  draft: HumanDraftListItem;
  draftButtonRef: (draftId: string) => RefCallback<HTMLButtonElement>;
  locale: Locale;
  restoring: boolean;
  restoreLocked: boolean;
  t: Translator;
  onRestore: (draft: HumanDraftListItem) => void;
}

function recipientCount(locale: Locale, value: number, t: Translator) {
  const count = formatCount(locale, value);
  const category = new Intl.PluralRules(locale).select(value);
  return category === "one"
    ? t("drafts.recipientCount.one", { count })
    : t("drafts.recipientCount.other", { count });
}

/**
 * Pure metadata projection for one recoverable draft. The outer native button
 * deliberately remains the existing restore target: directory ownership,
 * confirmation, CAS recovery, and focus return remain in the parent hook.
 */
export function DraftDirectoryItem({
  draft,
  draftButtonRef,
  locale,
  restoring,
  restoreLocked,
  t,
  onRestore,
}: DraftDirectoryItemProps) {
  const kind = draft.reply_to_message_id
    ? t("drafts.replyDraft")
    : t("drafts.newDraft");

  return (
    <button
      ref={draftButtonRef(draft.draft_id)}
      className="draft-item directory-item"
      disabled={restoreLocked}
      type="button"
      onClick={() => onRestore(draft)}
    >
      <span className="directory-item__lead">
        <span className="directory-item__title">
          <Icon aria-hidden name="draft" size={17} />
          <strong>{kind}</strong>
        </span>
        <span className="directory-item__facts">
          <span>{recipientCount(locale, draft.recipient_count, t)}</span>
          <time dateTime={draft.updated_at}>
            {formatDate(locale, draft.updated_at, {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </time>
        </span>
      </span>
      <span className="directory-item__action draft-restore-label">
        <Icon aria-hidden name="compose" size={16} />
        {restoring ? t("drafts.restoring") : t("drafts.restore")}
      </span>
    </button>
  );
}
