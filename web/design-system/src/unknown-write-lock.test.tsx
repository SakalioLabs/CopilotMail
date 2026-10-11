import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { UnknownWriteLock } from "./unknown-write-lock";

describe("UnknownWriteLock", () => {
  it("communicates an unresolved result and exposes only its supplied reconciliation action", () => {
    render(
      <UnknownWriteLock
        description="The result is not confirmed. Read the exact resource before any new change."
        evidence={<code>request-safe-123</code>}
        reconciliationAction={<button type="button">Refresh confirmed state</button>}
        title="Write outcome needs reconciliation"
      />,
    );

    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("data-write-state", "unknown");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveTextContent("Write outcome needs reconciliation");
    expect(status).toHaveTextContent("request-safe-123");
    expect(screen.getByRole("button", { name: "Refresh confirmed state" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
  });

  it("can opt out of announcements without inventing a live role", () => {
    render(
      <UnknownWriteLock
        description="Use a read-only reconciliation request."
        live="off"
        reconciliationAction={<a href="#confirmed">Read confirmed state</a>}
        title="Outcome unknown"
      />,
    );

    const lock = screen.getByText("Outcome unknown").closest("section");
    expect(lock).not.toHaveAttribute("role");
    expect(lock).not.toHaveAttribute("aria-live");
    expect(screen.getByRole("link", { name: "Read confirmed state" })).toHaveAttribute(
      "href",
      "#confirmed",
    );
  });

  it("does not invent a reconciliation action when no exact read target exists", () => {
    const { container } = render(
      <UnknownWriteLock
        description="No exact resource can be read safely from this unresolved request."
        title="Outcome unknown"
      />,
    );

    const lock = container.querySelector<HTMLElement>(".unknown-write-lock");
    expect(lock).not.toBeNull();
    if (!lock) throw new Error("Unknown write lock was not rendered");
    expect(lock).toHaveTextContent("Outcome unknown");
    expect(lock.querySelector(".unknown-write-lock__action")).toBeNull();
    expect(within(lock).queryByRole("button")).not.toBeInTheDocument();
    expect(within(lock).queryByRole("link")).not.toBeInTheDocument();
  });
});
