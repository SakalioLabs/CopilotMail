// @vitest-environment jsdom

import { cleanup, render, screen, within } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { MessageDetail as MessageDetailValue } from "../types";
import { MessageDetail } from "./MessageDetail";

const t = createTranslator("en");
const detail: MessageDetailValue = {
  message: {
    message_id: "019b0000-0000-7000-8000-000000000001",
    subject: "A long, real operational message",
    from: [{
      name: "Jordan Li",
      email: "jordan.li+operational-review@a-very-long-partner-domain.example",
    }],
    received_at: "2026-08-24T03:04:00Z",
    preview: "Review requested",
    is_read: true,
    is_flagged: false,
    has_attachment: false,
  },
  reply_to_message_id: null,
  to: [
    { name: "Priya Shah", email: "priya.shah@copilotmail.test" },
    { name: null, email: "long-routing-destination@operations.copilotmail.test" },
  ],
  cc: [{ name: "Operations Review", email: "ops-review@copilotmail.test" }],
  text_body: "Safe plain text.",
  text_body_truncated: false,
  attachments: [],
};

function renderDetail(
  value: MessageDetailValue = detail,
  overrides: Partial<React.ComponentProps<typeof MessageDetail>> = {},
) {
  return render(
    <MessageDetail
      canReply
      detail={value}
      headingRef={createRef<HTMLHeadingElement>()}
      locale="en"
      selected={value.message}
      t={t}
      onBack={vi.fn()}
      onReply={vi.fn()}
      onRetry={vi.fn()}
      onToggleFlag={vi.fn()}
      {...overrides}
    />,
  );
}

afterEach(cleanup);

describe("MessageDetail truthful metadata", () => {
  it("shows the exact sender address, received time, every To recipient, and Cc", () => {
    const { container } = renderDetail();
    const metadata = container.querySelector<HTMLElement>(".message-metadata");
    expect(metadata).not.toBeNull();
    expect(within(metadata!).getByText("From").nextElementSibling).toHaveTextContent(
      "Jordan Li <jordan.li+operational-review@a-very-long-partner-domain.example>",
    );
    expect(within(metadata!).getByText("Received").nextElementSibling)
      .toHaveTextContent(/2026/);
    expect(within(metadata!).getByText("To").nextElementSibling).toHaveTextContent(
      "Priya Shah <priya.shah@copilotmail.test>, long-routing-destination@operations.copilotmail.test",
    );
    expect(within(metadata!).getByText("Cc").nextElementSibling).toHaveTextContent(
      "Operations Review <ops-review@copilotmail.test>",
    );
    expect(screen.getByText("Safe plain text.")).toBeInTheDocument();
  });

  it("does not invent a Cc row when the server returns no Cc recipients", () => {
    renderDetail({ ...detail, cc: [] });
    expect(screen.queryByText("Cc")).not.toBeInTheDocument();
  });

  it("renders an explicit empty value rather than a sender label for an empty To list", () => {
    renderDetail({ ...detail, to: [] });
    expect(screen.getByText("To").nextElementSibling).toHaveTextContent("—");
    expect(screen.getByText("To").nextElementSibling).not.toHaveTextContent("Unknown sender");
  });

  it("keeps only server-authorized actions between the envelope and safe body", () => {
    const replyReady: MessageDetailValue = {
      ...detail,
      reply_to_message_id: detail.message.message_id,
    };
    const { container } = renderDetail(replyReady, { canReply: false });
    const metadata = container.querySelector<HTMLElement>(".message-metadata");
    const actions = container.querySelector<HTMLElement>(".reading-actions");
    const body = container.querySelector<HTMLElement>(".detail-body");

    expect(metadata).not.toBeNull();
    expect(actions).toHaveAttribute("role", "group");
    expect(actions).toHaveAttribute("aria-label", "Message actions");
    expect(body).not.toBeNull();
    expect(metadata!.compareDocumentPosition(actions!) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBeGreaterThan(0);
    expect(actions!.compareDocumentPosition(body!) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBeGreaterThan(0);
    expect(within(actions!).getByRole("button", { name: "Add star" })).toBeVisible();
    const reply = within(actions!).getByRole("button", { name: "Reply" });
    expect(reply).toBeDisabled();
    expect(reply).toHaveAttribute(
      "title",
      "You do not have permission to send from this mailbox",
    );
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("does not render Reply before the server returns a matching reply relation", () => {
    renderDetail();
    expect(screen.queryByRole("button", { name: "Reply" })).not.toBeInTheDocument();
  });

  it("announces a pending read politely and reports failures as alerts", () => {
    const { rerender } = render(
      <MessageDetail
        canReply
        headingRef={createRef<HTMLHeadingElement>()}
        locale="en"
        selected={detail.message}
        t={t}
        onBack={vi.fn()}
        onReply={vi.fn()}
        onRetry={vi.fn()}
        onToggleFlag={vi.fn()}
      />,
    );

    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Loading…");
    expect(status).toHaveAttribute("aria-live", "polite");

    rerender(
      <MessageDetail
        canReply
        headingRef={createRef<HTMLHeadingElement>()}
        loadError="This message is temporarily unavailable"
        locale="en"
        selected={detail.message}
        t={t}
        onBack={vi.fn()}
        onReply={vi.fn()}
        onRetry={vi.fn()}
        onToggleFlag={vi.fn()}
      />,
    );
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("alert"))
      .toHaveTextContent("This message is temporarily unavailable");

    rerender(
      <MessageDetail
        actionError="The star state could not be saved"
        canReply
        detail={detail}
        headingRef={createRef<HTMLHeadingElement>()}
        locale="en"
        selected={detail.message}
        t={t}
        onBack={vi.fn()}
        onReply={vi.fn()}
        onRetry={vi.fn()}
        onToggleFlag={vi.fn()}
      />,
    );
    expect(screen.getByRole("alert"))
      .toHaveTextContent("The star state could not be saved");
  });
});
