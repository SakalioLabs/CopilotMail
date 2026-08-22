// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { HumanDraftPage } from "../types";
import { DraftList } from "./DraftList";

const t = createTranslator("en");
const page: HumanDraftPage = {
  items: [{
    draft_id: "019b0000-0000-7000-8000-000000000090",
    version: 3,
    reply_to_message_id: null,
    recipient_count: 2,
    created_at: "2026-07-27T10:00:00Z",
    updated_at: "2026-07-27T11:00:00Z",
  }, {
    draft_id: "019b0000-0000-7000-8000-000000000089",
    version: 1,
    reply_to_message_id: "019b0000-0000-7000-8000-000000000001",
    recipient_count: 1,
    created_at: "2026-07-27T09:00:00Z",
    updated_at: "2026-07-27T09:30:00Z",
  }],
  next_cursor: "v1.019b0000-0000-7000-8000-000000000089",
  has_more: true,
};

afterEach(cleanup);

describe("DraftList", () => {
  it("renders only recovery metadata and exposes restore and pagination actions", () => {
    const onLoadMore = vi.fn();
    const onRestore = vi.fn();
    render(<DraftList
      draftButtonRef={() => () => undefined}
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={page}
      t={t}
      onLoadMore={onLoadMore}
      onRefresh={vi.fn()}
      onRestore={onRestore}
    />);

    const rows = screen.getAllByRole("button", { name: /Resume editing/ });
    expect(rows[0]).toHaveAccessibleName(/New message draft/);
    expect(rows[0]).toHaveAccessibleName(/2 recipients/);
    expect(rows[1]).toHaveAccessibleName(/Reply draft/);
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    fireEvent.click(rows[1]);
    expect(onRestore).toHaveBeenCalledWith(page.items[1]);
    fireEvent.click(screen.getByRole("button", { name: "Load more drafts" }));
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it("locks refresh, rows, and pagination while one restore is in flight", () => {
    render(<DraftList
      draftButtonRef={() => () => undefined}
      headingRef={{ current: null }}
      listBusy={false}
      locale="en"
      page={page}
      restoreBusyId={page.items[0].draft_id}
      t={t}
      onLoadMore={vi.fn()}
      onRefresh={vi.fn()}
      onRestore={vi.fn()}
    />);

    expect(screen.getByRole("button", { name: "Refresh drafts" })).toBeDisabled();
    screen.getAllByRole("button", { name: /editing|Restoring/ })
      .forEach((row) => expect(row).toBeDisabled());
    expect(screen.getByRole("button", { name: "Load more drafts" })).toBeDisabled();
  });
});
