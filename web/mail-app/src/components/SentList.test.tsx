// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
});
