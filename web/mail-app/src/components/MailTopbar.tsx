import type { FormEvent, RefObject } from "react";
import { BrandMark, CloseIcon, Icon, IconButton, MenuIcon, SearchIcon } from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { LanguagePicker } from "./LanguagePicker";

interface MailTopbarProps {
  locale: Locale;
  navigationOpen: boolean;
  activeSearch?: string;
  searchBusy: boolean;
  searchText: string;
  searchInputRef?: RefObject<HTMLInputElement | null>;
  t: Translator;
  onClearSearch: () => void;
  onLocaleChange: (locale: Locale) => void;
  onOpenMenu: () => void;
  onSearch: () => void;
  onSearchTextChange: (value: string) => void;
  onSignOut: () => void;
}

/**
 * Gmail-style top bar: text wordmark, a rounded search omnibox, and the
 * session controls (language + sign out). The hamburger only appears at the
 * mobile breakpoint via CSS; the search stays reachable because it owns the
 * same input the command palette focuses.
 */
export function MailTopbar({
  locale,
  navigationOpen,
  activeSearch,
  searchBusy,
  searchText,
  searchInputRef,
  t,
  onClearSearch,
  onLocaleChange,
  onOpenMenu,
  onSearch,
  onSearchTextChange,
  onSignOut,
}: MailTopbarProps) {
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    onSearch();
  };

  return (
    <header className="app-topbar mail-topbar" data-dialog-background>
      <div className="mail-topbar__start">
        <IconButton
          aria-expanded={navigationOpen}
          aria-haspopup="dialog"
          className="mail-topbar__menu"
          icon={<MenuIcon />}
          label={t("nav.openMenu")}
          onClick={onOpenMenu}
        />
        <span className="app-wordmark"><BrandMark />{t("app.name")}</span>
      </div>
      <form className="mail-search search-omnibox" role="search" onSubmit={submit}>
        <IconButton
          className="mail-search__submit"
          disabled={searchBusy}
          icon={<SearchIcon />}
          label={t("inbox.submitSearchAria")}
          type="submit"
        />
        <input
          ref={searchInputRef}
          aria-label={t("inbox.searchAria")}
          maxLength={128}
          placeholder={t("inbox.searchPlaceholder")}
          value={searchText}
          onChange={(event) => onSearchTextChange(event.target.value)}
        />
        {activeSearch ? (
          <IconButton
            className="mail-search__clear"
            disabled={searchBusy}
            icon={<CloseIcon />}
            label={t("inbox.clearSearchAria")}
            onClick={onClearSearch}
          />
        ) : null}
      </form>
      <div
        aria-label={t("topbar.sessionRegion")}
        className="mail-topbar__session"
        role="group"
      >
        <LanguagePicker locale={locale} onChange={onLocaleChange} t={t} />
        <button className="topbar-sign-out" type="button" onClick={onSignOut}>
          <Icon aria-hidden name="sign-out" size={17} />
          {t("nav.signOut")}
        </button>
      </div>
    </header>
  );
}
