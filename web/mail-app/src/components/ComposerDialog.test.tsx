// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import { ComposerDialog, type ComposerDialogProps } from "./ComposerDialog";

const t = createTranslator("en");

function props(
  overrides: Partial<ComposerDialogProps> = {},
): ComposerDialogProps {
  return {
    t,
    locale: "en",
    replyMode: false,
    mailbox: {
      mailbox_id: "019b0000-0000-7000-8000-000000000020",
      kind: "human",
      primary_address: "reader@example.com",
      can_send: true,
    },
    to: "alice@example.com",
    cc: "",
    bcc: "",
    subject: "",
    textBody: "",
    draftStatus: "Draft saved",
    draftStatusTone: "positive",
    submissionLocked: false,
    sendBusy: false,
    terminalSubmission: false,
    onToChange: vi.fn(),
    onCcChange: vi.fn(),
    onBccChange: vi.fn(),
    onSubjectChange: vi.fn(),
    onTextBodyChange: vi.fn(),
    onClose: vi.fn(),
    onSend: vi.fn(),
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  document.body.replaceChildren();
});

describe("ComposerDialog", () => {
  it("places the supplied asynchronous status beside the only send action, outside Compose context", () => {
    render(<ComposerDialog {...props({
      draftStatus: "The send outcome requires review",
      draftStatusTone: "warning",
    })} />);

    const status = screen.getByRole("status");
    const footer = status.closest("footer");
    const context = screen.getByRole("region", { name: "Compose context" });

    expect(status).toHaveClass("status-indicator--warning");
    expect(status).toHaveTextContent("The send outcome requires review");
    expect(status.querySelector(".status-indicator__mark")).toBeInTheDocument();
    expect(footer).not.toBeNull();
    expect(footer!).toContainElement(status);
    expect(footer!).toContainElement(screen.getByRole("button", { name: "Send" }));
    expect(context).not.toContainElement(status);
    expect(context.querySelector(".composer-status")).toBeNull();
  });

  it("keeps real manual-review evidence collapsed until the user explicitly opens it", () => {
    const { rerender } = render(<ComposerDialog {...props()} />);
    expect(screen.queryByText("Submission and delivery evidence"))
      .not.toBeInTheDocument();

    rerender(<ComposerDialog {...props({
      draftStatus: "The send status needs manual review; do not send it again",
      draftStatusTone: "warning",
      submissionEvidence: {
        submissionId: "019b0000-0000-7000-8000-000000000040",
        status: "manual_review",
        stateVersion: 2,
        updatedAt: "2026-07-27T12:00:02.000Z",
        delivery: null,
      },
      terminalSubmission: true,
    })} />);

    const evidence = screen.getByText("Submission and delivery evidence").closest("details");
    expect(evidence).not.toBeNull();
    expect(evidence).not.toHaveAttribute("open");
    expect(within(evidence!).getByText("Submission ID")).not.toBeVisible();

    fireEvent.click(within(evidence!).getByText("Submission and delivery evidence"));

    expect(evidence).toHaveAttribute("open");
    expect(within(evidence!).getByRole("img", {
      name: "A mail-processing route paused at a review gate",
    })).toHaveAttribute("src", expect.stringContaining("agent-human-review-v1.webp"));
    expect(within(evidence!).getByText(
      "Copy the submission ID and contact an administrator; do not send this message again. Reopen this evidence with Compose after closing it.",
    )).toBeVisible();
    expect(within(evidence!).getByText("Submission ID")).toBeVisible();
    expect(within(evidence!).getByText("019b0000-0000-7000-8000-000000000040"))
      .toBeVisible();
    expect(screen.getByRole("button", { name: "Sending locked" })).toBeDisabled();
  });

  it("renders an Agent identity from the server mailbox directory without speculative metadata", () => {
    const { container } = render(<ComposerDialog {...props({
      mailbox: {
        mailbox_id: "019b0000-0000-7000-8000-000000000021",
        kind: "agent",
        primary_address: "triage-agent@example.com",
        can_send: true,
      },
    })} />);

    const identity = screen.getByRole("region", { name: "Compose context" });
    expect(within(identity).getByText("Agent")).toBeInTheDocument();
    expect(within(identity).getByText("triage-agent@example.com")).toBeInTheDocument();
    expect(within(identity).getByText(
      "Server status: this mailbox can send email.",
    )).toHaveClass("status-indicator__label");
    expect(identity.querySelectorAll(".context-bar__fact")).toHaveLength(2);
    expect(identity.querySelector(".context-bar__facts"))
      .not.toHaveTextContent("Server status:");
    expect(identity.querySelector(".context-bar__status [data-sendable='true']")).toHaveClass(
      "status-indicator--positive",
    );
    expect(identity).not.toHaveTextContent(/owner|delegat|on behalf|actor/i);
    expect(container.querySelector("[data-owner], [data-scope], [data-delegation]")).toBeNull();
    expect(container.querySelector(".composer-evidence")).toBeNull();
  });

  it("renders only aggregate server delivery evidence, never recipient or SMTP details", () => {
    render(<ComposerDialog {...props({
      submissionEvidence: {
        submissionId: "019b0000-0000-7000-8000-000000000040",
        status: "accepted",
        stateVersion: 3,
        updatedAt: "2026-07-27T12:00:03.000Z",
        delivery: {
          observed_at: "2026-07-27T12:00:04.000Z",
          terminal: false,
          recipient_count: 2,
          delivered_count: 1,
          failed_count: 0,
          queued_count: 1,
          unknown_count: 0,
        },
      },
    })} />);

    const evidence = screen.getByText("Submission and delivery evidence").closest("details");
    expect(evidence).not.toBeNull();
    expect(evidence).not.toHaveAttribute("open");
    fireEvent.click(within(evidence!).getByText("Submission and delivery evidence"));

    expect(within(evidence!).getByText("Accepted by the mail server")).toBeVisible();
    expect(within(evidence!).getByText("Recipient count")).toBeVisible();
    expect(within(evidence!).getByText("2")).toBeVisible();
    expect(evidence).not.toHaveTextContent(/recipient_ref|smtp_reply|250 2\.0\.0/i);
    expect(evidence).not.toHaveTextContent(/owner|delegat|on behalf|actor/i);
  });

  it("shows the server-returned read-only boundary and prevents sending", () => {
    render(<ComposerDialog {...props({
      mailbox: {
        mailbox_id: "019b0000-0000-7000-8000-000000000022",
        kind: "shared",
        primary_address: "read-only@example.com",
        can_send: false,
      },
    })} />);

    expect(screen.getByText(
      "Server status: this mailbox is read-only and cannot send email.",
    )).toHaveClass("status-indicator__label");
    expect(screen.getByRole("region", { name: "Compose context" })
      .querySelector("[data-sendable='false']")).toHaveClass("status-indicator--warning");
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("captures focus and initially focuses the recipient", () => {
    const trigger = document.createElement("button");
    trigger.textContent = "Compose";
    document.body.append(trigger);
    trigger.focus();

    render(<ComposerDialog {...props()} />);

    expect(document.activeElement).toBe(screen.getByLabelText("To"));
  });

  it("labels a restored draft distinctly and initially focuses its body", () => {
    render(<ComposerDialog {...props({
      restoredMode: true,
      initialFocus: "body",
      textBody: "Recovered only in memory",
    })} />);

    expect(screen.getByRole("dialog", { name: "Edit draft" })).toBeInTheDocument();
    expect(screen.getByLabelText("Message body")).toHaveValue("Recovered only in memory");
    expect(screen.getByLabelText("Message body")).toHaveFocus();
  });

  it("focuses the first available control when submission locking disables recipients", () => {
    render(<ComposerDialog {...props({ submissionLocked: true })} />);

    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close composer" }));
  });

  it("labels an unresolved frozen submission as confirming the same request, not a new send", () => {
    const onSend = vi.fn();
    const { rerender } = render(<ComposerDialog {...props({
      submissionLocked: true,
      onSend,
    })} />);

    const englishAction = screen.getByRole("button", {
      name: "Confirm same submission (no duplicate email)",
    });
    expect(englishAction).toBeEnabled();
    expect(screen.getByRole("textbox", { name: "To" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Retry send" })).toBeNull();
    fireEvent.click(englishAction);
    expect(onSend).toHaveBeenCalledTimes(1);

    rerender(<ComposerDialog {...props({
      t: createTranslator("zh-CN"),
      locale: "zh-CN",
      submissionLocked: true,
    })} />);
    expect(screen.getByRole("button", {
      name: "再次确认同一提交（不会创建第二封邮件）",
    })).toBeEnabled();
  });

  it("isolates every marked background and restores its exact attributes", () => {
    const plainBackground = document.createElement("main");
    plainBackground.dataset.dialogBackground = "";
    const attributedBackground = document.createElement("aside");
    attributedBackground.dataset.dialogBackground = "";
    attributedBackground.setAttribute("inert", "preserved");
    attributedBackground.setAttribute("aria-hidden", "false");
    document.body.append(plainBackground, attributedBackground);

    const { unmount } = render(<ComposerDialog {...props()} />);

    for (const background of [plainBackground, attributedBackground]) {
      expect(background.hasAttribute("inert")).toBe(true);
      expect(background.getAttribute("aria-hidden")).toBe("true");
    }

    unmount();
    expect(plainBackground.hasAttribute("inert")).toBe(false);
    expect(plainBackground.hasAttribute("aria-hidden")).toBe(false);
    expect(attributedBackground.getAttribute("inert")).toBe("preserved");
    expect(attributedBackground.getAttribute("aria-hidden")).toBe("false");
  });

  it("wraps Tab and Shift+Tab at the dialog boundaries", () => {
    render(<ComposerDialog {...props()} />);
    const close = screen.getByRole("button", { name: "Close composer" });
    const send = screen.getByRole("button", { name: "Send" });

    send.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(close);

    close.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(send);
  });

  it("closes on Escape when sending is idle", () => {
    const onClose = vi.fn();
    render(<ComposerDialog {...props({ onClose })} />);

    fireEvent.keyDown(document, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("refuses to close on Escape while sending", () => {
    const onClose = vi.fn();
    render(<ComposerDialog {...props({ onClose, sendBusy: true })} />);

    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Close composer" }));

    expect(onClose).not.toHaveBeenCalled();
  });

  it("restores focus to the captured trigger after unmount", () => {
    const trigger = document.createElement("button");
    trigger.textContent = "Compose";
    document.body.append(trigger);
    trigger.focus();
    const { unmount } = render(<ComposerDialog {...props()} />);

    unmount();

    expect(document.activeElement).toBe(trigger);
  });
});
