import type { RefCallback, RefObject } from "react";
import { IconButton } from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { formatMessageCount } from "../i18n";
import type { InboxPage, MessageSummary } from "../types";
import { MessageQueueItem } from "./MessageQueueItem";

const inboxZeroIllustration = new URL(
  "../../../design-system/assets/illustrations/inbox-zero-v1.webp",
  import.meta.url,
).href;

interface MessageListProps {
  activeSearch?: string;
  error?: string;
  headingRef?: RefObject<HTMLHeadingElement | null>;
  searchInputRef?: RefObject<HTMLInputElement | null>;
  locale: Locale;
  page?: InboxPage;
  searchBusy: boolean;
  searchText: string;
  selectedMessageId?: string;
  t: Translator;
  onClearSearch: () => void;
  onLoadMore: () => void;
  onOpenMessage: (message: MessageSummary) => void;
  onRefresh: () => void;
  onSearch: () => void;
  onSearchTextChange: (value: string) => void;
  messageButtonRef: (messageId: string) => RefCallback<HTMLButtonElement>;
}

export function MessageList({
  activeSearch,
  error,
  headingRef,
  searchInputRef,
  locale,
  page,
  searchBusy,
  searchText,
  selectedMessageId,
  t,
  onClearSearch,
  onLoadMore,
  onOpenMessage,
  onRefresh,
  onSearch,
  onSearchTextChange,
  messageButtonRef,
}: MessageListProps) {
  const inboxBusy = searchBusy || (!page && !error);
  const searchLength = [...searchText.trim()].length;
  const canRetry = activeSearch !== undefined || searchLength === 0 || searchLength >= 2;
  const retry = () => {
    if (activeSearch !== undefined || searchLength >= 2) onSearch();
    else onRefresh();
  };
  const listStatus = inboxBusy
    ? t("inbox.syncing")
    : page
      ? formatMessageCount(locale, page.items.length)
      : "";

  return (
    <section className="inbox" data-dialog-background>
      <header className="inbox-header">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>
            {activeSearch ? t("inbox.searchResults") : t("nav.inbox")}
          </h1>
          <p aria-atomic="true" aria-live="polite" role="status">{listStatus}</p>
        </div>
        <IconButton
          className="inbox-refresh"
          aria-busy={searchBusy || undefined}
          disabled={searchBusy}
          icon="refresh"
          label={t("inbox.refreshAria")}
          onClick={onRefresh}
        />
      </header>
      <form
        className="mail-search"
        role="search"
        onSubmit={(event) => {
          event.preventDefault();
          onSearch();
        }}
      >
        <input
          ref={searchInputRef}
          aria-label={t("inbox.searchAria")}
          maxLength={128}
          placeholder={t("inbox.searchPlaceholder")}
          value={searchText}
          onChange={(event) => onSearchTextChange(event.target.value)}
        />
        {activeSearch && (
          <IconButton
            className="mail-search__clear"
            icon="close"
            label={t("inbox.clearSearchAria")}
            onClick={onClearSearch}
          />
        )}
        <IconButton
          className="mail-search__submit"
          disabled={searchBusy}
          icon="search"
          label={t("inbox.submitSearchAria")}
          type="submit"
        />
      </form>
      {error && (
        <div className="error error--actionable" role="alert">
          <span>{error}</span>
          {canRetry && (
            <button disabled={searchBusy} type="button" onClick={retry}>
              {t("common.retry")}
            </button>
          )}
        </div>
      )}
      <div className="messages" aria-busy={inboxBusy || undefined}>
        {page?.items.map((message) => (
          <MessageQueueItem
            key={message.message_id}
            locale={locale}
            message={message}
            selected={selectedMessageId === message.message_id}
            t={t}
            messageButtonRef={messageButtonRef}
            onOpenMessage={onOpenMessage}
          />
        ))}
        {page && page.items.length === 0 && (
          <div className="empty empty--contextual">
            {!activeSearch && (
              <img
                src={inboxZeroIllustration}
                alt={t("inbox.emptyIllustrationAlt")}
              />
            )}
            <strong>{activeSearch ? t("inbox.noSearchResults") : t("inbox.empty")}</strong>
            <p>{activeSearch ? t("inbox.noSearchResultsHint") : t("inbox.emptyHint")}</p>
          </div>
        )}
        {page?.has_more && (
          <button className="load-more" disabled={searchBusy} onClick={onLoadMore}>
            {searchBusy ? t("inbox.loadingMore") : t("inbox.loadMore")}
          </button>
        )}
      </div>
    </section>
  );
}
