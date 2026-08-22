import type { Locale, Translator } from "../i18n";

interface LanguagePickerProps {
  locale: Locale;
  onChange: (locale: Locale) => void;
  t: Translator;
}

export function LanguagePicker({ locale, onChange, t }: LanguagePickerProps) {
  return (
    <label className="language-picker">
      <span className="sr-only">{t("locale.label")}</span>
      <select
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
