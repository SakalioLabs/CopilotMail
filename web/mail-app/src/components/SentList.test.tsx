// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { HumanSentSubmissionPage } from "../types";
import { SentList } from "./SentList";

const t = createTranslator("en");
const page: HumanSentSubmissionPage = {
  items: [{
    submission_id: "019b0000-0000-7000-8000-000000000090",
    accepted_at: "2026-07-27T10:00:00Z",
    recipient_count: 2,
    delivery: {
      observed_at: "2026-07-27T10:01:00Z",
      terminal: true,
      recipient_count: 2,
      delivered_count: 1,
      failed_count: 1,
      queued_count: 0,
      unknown_count: 0,
    },
  }, {
    submission_id: "019b0000-0000-7000-8000-000000000089",
    accepted_at: "2026-07-27T09:00:00Z",
    recipient_count: 1,
  }],
  next_cursor: "v1.019b0000-0000-7000-8000-000000000089",
  has_more: true,
};

afterEach(cleanup);

describe("SentList", () => {
  it("renders accepted-send and aggregate delivery metadata without sensitive evidence", () => {
    const onLoadMore = vi.fn();
    render(<SentList
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={page}
      t={t}
      onLoadMore={onLoadMore}
      onRefresh={vi.fn()}
    />);

    expect(screen.getAllByText("Accepted for delivery")).toHaveLength(2);
    expect(screen.getByText("Delivery complete: 1 succeeded, 1 failed")).toBeInTheDocument();
    expect(screen.getByText("Accepted for delivery; waiting for a delivery status update"))
      .toBeInTheDocument();
    expect(screen.getByText("2 recipients")).toBeInTheDocument();
    expect(screen.queryByText(page.items[0].submission_id)).not.toBeInTheDocument();
    expect(screen.queryByText(/smtp_reply|recipient_ref|draft_id/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Load more sent mail" }));
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it("keeps sent records noninteractive while exposing only aggregate delivery evidence", () => {
    render(<SentList
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={page}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={vi.fn()}
    />);

    expect(screen.getByText(
      "The sent directory shows delivery summaries only, never message content or recipient details",
    )).toBeInTheDocument();
    const records = screen.getAllByRole("article");
    expect(records).toHaveLength(2);
    records.forEach((record) => {
      expect(within(record).queryByRole("button")).not.toBeInTheDocument();
      expect(within(record).queryByRole("link")).not.toBeInTheDocument();
      expect(record).not.toHaveAttribute("tabindex");
    });
    expect(document.querySelector('time[datetime="2026-07-27T10:00:00Z"]')).not.toBeNull();
    expect(screen.getByText(/^Updated /)).toBeInTheDocument();

    [
      page.items[0].submission_id,
      page.items[0].accepted_at,
      page.items[0].delivery!.observed_at,
      page.next_cursor!,
    ].forEach((internalValue) => {
      expect(screen.queryByText(internalValue, { exact: false })).not.toBeInTheDocument();
    });
  });

  it("states a terminal all-delivered result without adding recipient-level detail", () => {
    render(<SentList
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={{
        ...page,
        items: [{
          ...page.items[0],
          delivery: {
            ...page.items[0].delivery!,
            delivered_count: 2,
            failed_count: 0,
          },
        }],
        has_more: false,
        next_cursor: null,
      }}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={vi.fn()}
    />);

    expect(screen.getByText("Delivered to all 2 recipient servers")).toBeInTheDocument();
  });

  it("locks refresh and pagination while the bounded page request is in flight", () => {
    render(<SentList
      headingRef={{ current: null }}
      listBusy
      locale="en"
      page={page}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={vi.fn()}
    />);

    expect(screen.getByRole("button", { name: "Refresh sent mail" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Loading…" })).toBeDisabled();
  });

  it("shows all safe aggregate counters while delivery remains nonterminal", () => {
    render(<SentList
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={{
        ...page,
        items: [{
          ...page.items[0],
          delivery: {
            ...page.items[0].delivery!,
            terminal: false,
            delivered_count: 0,
            failed_count: 1,
            queued_count: 1,
          },
        }],
        has_more: false,
        next_cursor: null,
      }}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={vi.fn()}
    />);

    expect(screen.getByText(
      "Delivery update: 0/2 delivered, 1 failed, 1 queued, 0 unknown",
    )).toBeInTheDocument();
  });

  it("announces refresh state and offers real recovery actions for errors and an empty list", () => {
    const onRefresh = vi.fn();
    const { container, rerender } = render(<SentList
      error="Sent mail is temporarily unavailable"
      headingRef={{ current: null }}
      listBusy
      locale="en"
      page={page}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={onRefresh}
    />);

    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Syncing…");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveAttribute("aria-atomic", "true");
    expect(container.querySelector(".sent-items")).toHaveAttribute("aria-busy", "true");

    rerender(<SentList
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={{ ...page, items: [], has_more: false, next_cursor: null }}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={onRefresh}
    />);

    const empty = container.querySelector<HTMLElement>(".empty--actionable");
    expect(empty).not.toBeNull();
    fireEvent.click(within(empty!).getByRole("button", { name: "Refresh sent mail" }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("retries a sent directory error in place", () => {
    const onRefresh = vi.fn();
    render(<SentList
      error="Sent mail is temporarily unavailable"
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={page}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={onRefresh}
    />);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });
});
