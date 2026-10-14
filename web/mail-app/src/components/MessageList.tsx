import type { RefCallback, RefObject } from "react";
import {
  EmptyInboxArt,
  EmptySearchArt,
  EmptyState,
  IconButton,
  RefreshIcon,
} from "@copilotmail/design-system";
import type { Locale, Translator } from "../i18n";
import { formatMessageCount } from "../i18n";
import type { InboxPage, MessageSummary } from "../types";
import { MessageQueueItem } from "./MessageQueueItem";

interface MessageListProps {
  activeSearch?: string;
  error?: string;
  headingRef?: RefObject<HTMLHeadingElement | null>;
  locale: Locale;
  page?: InboxPage;
  /** Callback ref on the scrollable queue; used to restore scroll on return. */
  scrollerRef?: RefCallback<HTMLDivElement>;
  searchBusy: boolean;
  searchText: string;
  selectedMessageId?: string;
  t: Translator;
  onLoadMore: () => void;
  onOpenMessage: (message: MessageSummary) => void;
  onRefresh: () => void;
  onSearch: () => void;
  onToggleFlag: (message: MessageSummary) => void;
  messageButtonRef: (messageId: string) => RefCallback<HTMLButtonElement>;
}

export function MessageList({
  activeSearch,
  error,
  headingRef,
  locale,
  page,
  scrollerRef,
  searchBusy,
  searchText,
  selectedMessageId,
  t,
  onLoadMore,
  onOpenMessage,
  onRefresh,
  onSearch,
  onToggleFlag,
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
          icon={<RefreshIcon />}
          label={t("inbox.refreshAria")}
          onClick={onRefresh}
        />
      </header>
      {error && (
        <div className="error error--actionable" role="alert">
          <span>{error}</span>
          {canRetry && (
            <button className="button button--secondary" disabled={searchBusy} type="button" onClick={retry}>
              {t("common.retry")}
            </button>
          )}
        </div>
      )}
      <div ref={scrollerRef} className="messages" aria-busy={inboxBusy || undefined}>
        {page?.items.map((message) => (
          <MessageQueueItem
            key={message.message_id}
            locale={locale}
            message={message}
            selected={selectedMessageId === message.message_id}
            t={t}
            messageButtonRef={messageButtonRef}
            onOpenMessage={onOpenMessage}
            onToggleFlag={onToggleFlag}
          />
        ))}
        {page && page.items.length === 0 && (activeSearch ? (
          <EmptyState
            art={<EmptySearchArt />}
            className="empty empty--contextual"
            headingLevel={2}
            title={t("inbox.noSearchResults")}
            description={t("inbox.noSearchResultsHint")}
          />
        ) : (
          <EmptyState
            art={<EmptyInboxArt />}
            className="empty empty--contextual"
            headingLevel={2}
            title={t("inbox.empty")}
            description={t("inbox.emptyHint")}
          />
        ))}
        {page?.has_more && (
          <button className="button button--secondary load-more" disabled={searchBusy} onClick={onLoadMore}>
            {searchBusy ? t("inbox.loadingMore") : t("inbox.loadMore")}
          </button>
        )}
      </div>
    </section>
  );
}
