// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { InboxPage, MessageSummary } from "../types";
import { MessageList } from "./MessageList";

const t = createTranslator("en");
const message: MessageSummary = {
  message_id: "019b0000-0000-7000-8000-000000000001",
  subject: "Quarterly review",
  from: [{ name: "Alice", email: "alice@example.com" }],
  received_at: "2026-07-27T12:00:00.000Z",
  preview: "A short preview",
  is_read: true,
  is_flagged: false,
  has_attachment: false,
};
const page: InboxPage = {
  items: [message],
  next_cursor: null,
  has_more: false,
};

function renderList(overrides: Partial<React.ComponentProps<typeof MessageList>> = {}) {
  const props: React.ComponentProps<typeof MessageList> = {
    locale: "en",
    page,
    searchBusy: false,
    searchText: "",
    t,
    onClearSearch: vi.fn(),
    onLoadMore: vi.fn(),
    onOpenMessage: vi.fn(),
    onRefresh: vi.fn(),
    onSearch: vi.fn(),
    onSearchTextChange: vi.fn(),
    messageButtonRef: () => () => undefined,
    ...overrides,
  };
  return { props, ...render(<MessageList {...props} />) };
}

afterEach(cleanup);

describe("MessageList", () => {
  it("announces syncing and marks the message collection busy during its initial load and refresh", () => {
    const { container, rerender, props } = renderList({ page: undefined });

    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Syncing…");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveAttribute("aria-atomic", "true");
    expect(container.querySelector(".messages")).toHaveAttribute("aria-busy", "true");

    rerender(<MessageList {...props} page={page} searchBusy />);

    expect(screen.getByRole("status")).toHaveTextContent("Syncing…");
    expect(container.querySelector(".messages")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveAttribute(
      "aria-busy",
      "true",
    );

    rerender(<MessageList {...props} page={page} />);

    expect(screen.getByRole("status")).toHaveTextContent("1 message");
  });

  it("retries an inbox failure in place", () => {
    const onRefresh = vi.fn();
    const { container } = renderList({
      error: "The inbox is temporarily unavailable",
      onRefresh,
      page: undefined,
    });

    expect(screen.getByRole("alert")).toHaveTextContent("temporarily unavailable");
    expect(container.querySelector(".messages")).not.toHaveAttribute("aria-busy", "true");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("retries a failed valid search instead of discarding the entered query", () => {
    const onRefresh = vi.fn();
    const onSearch = vi.fn();
    renderList({
      error: "Search is temporarily unavailable",
      onRefresh,
      onSearch,
      searchText: "alice",
    });

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onSearch).toHaveBeenCalledTimes(1);
    expect(onRefresh).not.toHaveBeenCalled();
  });

  it("keeps queue scanning bound to the existing summary fields and opens only on activation", () => {
    const onOpenMessage = vi.fn();
    const queueMessage: MessageSummary = {
      ...message,
      is_read: false,
      is_flagged: true,
      has_attachment: true,
    };
    const { container } = renderList({
      page: { ...page, items: [queueMessage] },
      selectedMessageId: queueMessage.message_id,
      onOpenMessage,
    });

    const row = screen.getByRole("button", { name: /Quarterly review/ });
    expect(row).toHaveClass("message", "selected", "unread");
    expect(row).toHaveAttribute("aria-current", "true");
    expect(row.querySelector("time")).toHaveAttribute("datetime", queueMessage.received_at);
    expect(row).toHaveTextContent("Alice");
    expect(row).toHaveTextContent("A short preview");
    expect(row).toHaveTextContent("Unread");
    expect(row).toHaveTextContent("Starred");
    expect(row).toHaveTextContent("Has attachments");
    expect(container.textContent).not.toContain(queueMessage.message_id);

    fireEvent.click(row);
    expect(onOpenMessage).toHaveBeenCalledWith(queueMessage);
  });
});
