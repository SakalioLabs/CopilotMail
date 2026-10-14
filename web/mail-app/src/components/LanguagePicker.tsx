import type { Locale, Translator } from "../i18n";

interface LanguagePickerProps {
  locale: Locale;
  /** Render the label visibly instead of screen-reader-only (settings surface). */
  showLabel?: boolean;
  onChange: (locale: Locale) => void;
  t: Translator;
}

export function LanguagePicker({ locale, showLabel = false, onChange, t }: LanguagePickerProps) {
  return (
    <label className="language-picker">
      <span className={showLabel ? "language-picker__label" : "sr-only"}>{t("locale.label")}</span>
      <select
        className="control"
        aria-label={t("locale.label")}
        value={locale}
        onChange={(event) => onChange(event.target.value as Locale)}
      >
        <option value="zh-CN">{t("locale.zhCN")}</option>
        <option value="en">{t("locale.en")}</option>
      </select>
    </label>
  );
}
