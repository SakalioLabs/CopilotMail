import { ConfirmDialog } from "@copilotmail/design-system";
import type { Translator } from "../i18n";

export interface DraftRestoreConfirmDialogProps {
  busy?: boolean;
  t: Translator;
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * Mail-specific copy over the shared, fail-closed confirmation primitive.
 * Focus isolation, busy dismissal locking, and opener restoration are owned by
 * the design system so this workflow cannot silently drift from Admin dialogs.
 */
export function DraftRestoreConfirmDialog({
  busy = false,
  t,
  onCancel,
  onConfirm,
}: DraftRestoreConfirmDialogProps) {
  return (
    <ConfirmDialog
      busy={busy}
      cancelLabel={t("drafts.confirmCancel")}
      closeLabel={t("drafts.confirmCloseAria")}
      confirmLabel={t(busy ? "drafts.restoring" : "drafts.confirmAction")}
      confirmVariant="primary"
      description={t("drafts.confirmDescription")}
      role="alertdialog"
      showCloseButton={false}
      title={t("drafts.confirmTitle")}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />
  );
}
