import type { RefObject } from "react";
import { Dialog } from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { LanguagePicker } from "./LanguagePicker";

interface SettingsDialogProps {
  locale: Locale;
  returnFocusRef: RefObject<HTMLElement | null>;
  t: Translator;
  onClose: () => void;
  onLocaleChange: (locale: Locale) => void;
}

/**
 * Settings surface reached from the identity card menu. Locale selection
 * lives here; the dialog owns modal focus trapping and returns focus to the
 * identity card that opened it.
 */
export function SettingsDialog({
  locale,
  returnFocusRef,
  t,
  onClose,
  onLocaleChange,
}: SettingsDialogProps) {
  return (
    <Dialog
      className="settings-dialog"
      closeLabel={t("settings.closeAria")}
      returnFocusRef={returnFocusRef}
      title={t("settings.title")}
      onClose={onClose}
    >
      <div className="settings-dialog__body">
        <LanguagePicker showLabel locale={locale} onChange={onLocaleChange} t={t} />
      </div>
    </Dialog>
  );
}
