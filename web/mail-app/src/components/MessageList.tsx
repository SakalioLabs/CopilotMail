import type { RefCallback, RefObject } from "react";
import type { Locale, Translator } from "../i18n";
import { formatDate, formatMessageCount } from "../i18n";
import type { InboxPage, MessageSummary } from "../types";

interface MessageListProps {
  activeSearch?: string;
  error?: string;
  headingRef?: RefObject<HTMLHeadingElement | null>;
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

function sender(message: MessageSummary, t: Translator) {
  const first = message.from[0];
  return first?.name || first?.email || t("sender.unknown");
}

export function MessageList({
  activeSearch,
  error,
  headingRef,
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
  return (
    <section className="inbox" data-dialog-background>
      <header className="inbox-header">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>
            {activeSearch ? t("inbox.searchResults") : t("nav.inbox")}
          </h1>
          <p>
            {page
              ? formatMessageCount(locale, page.items.length)
              : t("inbox.syncing")}
          </p>
        </div>
        <button
          aria-label={t("inbox.refreshAria")}
          disabled={searchBusy}
          onClick={onRefresh}
        >
          <span aria-hidden="true">{t("icon.refresh")}</span>
        </button>
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
          aria-label={t("inbox.searchAria")}
          maxLength={128}
          placeholder={t("inbox.searchPlaceholder")}
          value={searchText}
          onChange={(event) => onSearchTextChange(event.target.value)}
        />
        {activeSearch && (
          <button
            type="button"
            aria-label={t("inbox.clearSearchAria")}
            onClick={onClearSearch}
          >
            <span aria-hidden="true">{t("icon.close")}</span>
          </button>
        )}
        <button
          type="submit"
          disabled={searchBusy}
          aria-label={t("inbox.submitSearchAria")}
        >
          <span aria-hidden="true">{t("icon.search")}</span>
        </button>
      </form>
      {error && <p className="error" role="alert">{error}</p>}
      <div className="messages">
        {page?.items.map((message) => {
          const selected = selectedMessageId === message.message_id;
          const className = [
            "message",
            selected ? "selected" : "",
            message.is_read ? "" : "unread",
          ].filter(Boolean).join(" ");
          return (
            <button
              key={message.message_id}
              ref={messageButtonRef(message.message_id)}
              className={className}
              aria-current={selected ? "true" : undefined}
              onClick={() => onOpenMessage(message)}
            >
              <span className="sender">{sender(message, t)}</span>
              <time dateTime={message.received_at}>
                {formatDate(locale, message.received_at)}
              </time>
              <strong>{message.subject || t("subject.none")}</strong>
              <span className="preview">{message.preview}</span>
              <span className="message-icons" aria-hidden="true">
                {message.is_flagged ? t("icon.starred") : ""}
                {message.has_attachment ? t("icon.attachment") : ""}
              </span>
              {!message.is_read && <span className="sr-only">{t("message.unread")}</span>}
              {message.is_flagged && <span className="sr-only">{t("message.starred")}</span>}
              {message.has_attachment && (
                <span className="sr-only">{t("message.hasAttachment")}</span>
              )}
            </button>
          );
        })}
        {page && page.items.length === 0 && (
          <div className="empty">
            {activeSearch ? t("inbox.noSearchResults") : t("inbox.empty")}
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
