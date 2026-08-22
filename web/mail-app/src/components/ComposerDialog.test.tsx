// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import { ComposerDialog, type ComposerDialogProps } from "./ComposerDialog";

const t = createTranslator("en");

function props(
  overrides: Partial<ComposerDialogProps> = {},
): ComposerDialogProps {
  return {
    t,
    replyMode: false,
    fromAddress: "reader@example.com",
    to: "alice@example.com",
    cc: "",
    bcc: "",
    subject: "",
    textBody: "",
    draftStatus: "Draft saved",
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
  it("renders the server-selected From identity as read-only", () => {
    render(<ComposerDialog {...props({ fromAddress: "shared@example.com" })} />);

    expect(screen.getByLabelText("From")).toHaveValue("shared@example.com");
    expect(screen.getByLabelText("From")).toHaveAttribute("readonly");
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
