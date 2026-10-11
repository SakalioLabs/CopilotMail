// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import { MailboxPicker } from "./MailboxPicker";

afterEach(cleanup);

describe("MailboxPicker", () => {
  it("exposes mailbox identity, switching, and bounded pagination to assistive technology", () => {
    const onChange = vi.fn();
    const onLoadMore = vi.fn();
    render(
      <MailboxPicker
        activeMailboxId="018f0000-0000-7000-8000-000000000001"
        hasMore
        loadMoreBusy={false}
        mailboxes={[
          {
            mailbox_id: "018f0000-0000-7000-8000-000000000003",
            kind: "shared",
            primary_address: "operations@example.test",
            can_send: true,
          },
          {
            mailbox_id: "018f0000-0000-7000-8000-000000000004",
            kind: "group",
            primary_address: "risk-review@example.test",
            can_send: false,
          },
          {
            mailbox_id: "018f0000-0000-7000-8000-000000000001",
            kind: "human",
            primary_address: "owner@example.test",
            can_send: true,
          },
          {
            mailbox_id: "018f0000-0000-7000-8000-000000000005",
            kind: "agent",
            primary_address: "triage-agent@example.test",
            can_send: true,
          },
          {
            mailbox_id: "018f0000-0000-7000-8000-000000000002",
            kind: "shared",
            primary_address: "support@example.test",
            can_send: false,
          },
        ]}
        onChange={onChange}
        onLoadMore={onLoadMore}
        t={createTranslator("en")}
      />,
    );

    const selector = screen.getByRole("combobox", { name: "Switch mailbox" });
    expect(selector).toHaveAccessibleName("Switch mailbox");
    const groups = Array.from(selector.querySelectorAll("optgroup"));
    expect(groups.map((group) => group.label)).toEqual([
      "Human mailboxes",
      "Agent mailboxes",
      "Shared mailboxes",
      "Group mailboxes",
    ]);
    expect(Array.from(groups[2].querySelectorAll("option"), (option) => option.value)).toEqual([
      "018f0000-0000-7000-8000-000000000003",
      "018f0000-0000-7000-8000-000000000002",
    ]);
    const loadMore = screen.getByRole("button", { name: "Load more mailboxes" });
    expect(loadMore.querySelector('[data-icon="add"]')).toBeInTheDocument();
    expect(loadMore).toHaveTextContent("Load more mailboxes");
    expect(screen.getByRole("option", { name: "support@example.test · Shared" }))
      .toBeInTheDocument();
    fireEvent.change(selector, {
      target: { value: "018f0000-0000-7000-8000-000000000002" },
    });
    fireEvent.click(loadMore);

    expect(onChange).toHaveBeenCalledWith("018f0000-0000-7000-8000-000000000002");
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });
});
