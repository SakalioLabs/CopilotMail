// @vitest-environment jsdom

import { Dialog } from "@copilotmail/design-system";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ADMIN_LIST_DETAIL_MEDIA_QUERY,
  AdminListDetail,
} from "./AdminListDetail";

interface MediaController {
  setMatches: (matches: boolean) => void;
}

function installMatchMedia(initialMatches: boolean): MediaController {
  let matches = initialMatches;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mediaQuery = {
    get matches() {
      return matches;
    },
    media: ADMIN_LIST_DETAIL_MEDIA_QUERY,
    onchange: null,
    addEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => {
      listeners.add(listener);
    },
    removeEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => {
      listeners.delete(listener);
    },
    addListener: (listener: (event: MediaQueryListEvent) => void) => listeners.add(listener),
    removeListener: (listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener),
    dispatchEvent: () => true,
  } as MediaQueryList;

  vi.stubGlobal("matchMedia", vi.fn((query: string) => {
    expect(query).toBe(ADMIN_LIST_DETAIL_MEDIA_QUERY);
    return mediaQuery;
  }));

  return {
    setMatches(nextMatches) {
      matches = nextMatches;
      const event = { matches, media: ADMIN_LIST_DETAIL_MEDIA_QUERY } as MediaQueryListEvent;
      for (const listener of listeners) listener(event);
    },
  };
}

function ListDetailHarness({
  initialSelection = null,
  locked = false,
  removeRowOnBack = false,
  withDialog = false,
}: {
  initialSelection?: string | null;
  locked?: boolean;
  removeRowOnBack?: boolean;
  withDialog?: boolean;
}) {
  const [selection, setSelection] = useState<string | null>(initialSelection);
  const [rowPresent, setRowPresent] = useState(true);
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <AdminListDetail
      backLabel="Back to cases"
      detailBody={(
        <>
          <p>Confirmed case facts</p>
          {withDialog ? (
            <button type="button" onClick={() => setDialogOpen(true)}>
              Open confirmation
            </button>
          ) : null}
          {dialogOpen ? (
            <Dialog
              closeLabel="Close confirmation"
              title="Confirm action"
              onClose={() => setDialogOpen(false)}
            >
              <button type="button">Confirm</button>
            </Dialog>
          ) : null}
        </>
      )}
      detailEmpty={<p>Select a case</p>}
      detailHeader={<p>Case details</p>}
      detailTitle="Case one"
      detailTrailing={<span>Open</span>}
      listLabel="Cases"
      navigationLocked={locked}
      renderList={(registerTrigger) => rowPresent ? (
        <button
          ref={registerTrigger("case-1")}
          type="button"
          onClick={() => setSelection("case-1")}
        >
          Open case one
        </button>
      ) : <p>No cases</p>}
      selectionKey={selection}
      onBack={() => {
        if (removeRowOnBack) setRowPresent(false);
        setSelection(null);
      }}
    />
  );
}

afterEach(() => {
  cleanup();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("AdminListDetail", () => {
  it("keeps the desktop two-panel view without stealing focus or handling Escape", () => {
    installMatchMedia(false);
    render(<ListDetailHarness />);

    const root = document.querySelector<HTMLElement>(".admin-list-detail");
    const list = document.querySelector<HTMLElement>(".admin-list-detail__list");
    const detail = document.querySelector<HTMLElement>(".admin-list-detail__detail");
    const row = screen.getByRole("button", { name: "Open case one" });
    expect(root).toHaveAttribute("data-detail-open", "false");
    expect(list).not.toHaveAttribute("hidden");
    expect(detail).not.toHaveAttribute("hidden");
    expect(screen.getByText("Select a case")).toBeVisible();

    row.focus();
    fireEvent.click(row);

    expect(root).toHaveAttribute("data-detail-open", "true");
    expect(screen.getByRole("heading", { name: "Case one" })).not.toHaveFocus();
    expect(row).toHaveFocus();
    expect(screen.queryByRole("button", { name: "Back to cases" })).not.toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.getByText("Confirmed case facts")).toBeVisible();
  });

  it("shows one panel at a time on narrow screens and restores the selected trigger", () => {
    installMatchMedia(true);
    render(<ListDetailHarness />);

    const root = document.querySelector<HTMLElement>(".admin-list-detail");
    const list = document.querySelector<HTMLElement>(".admin-list-detail__list");
    const detail = document.querySelector<HTMLElement>(".admin-list-detail__detail");
    const row = screen.getByRole("button", { name: "Open case one" });
    expect(list).not.toHaveAttribute("hidden");
    expect(detail).toHaveAttribute("hidden");

    row.focus();
    fireEvent.click(row);

    const title = screen.getByRole("heading", { name: "Case one" });
    expect(root).toHaveAttribute("data-detail-open", "true");
    expect(list).toHaveAttribute("hidden");
    expect(detail).not.toHaveAttribute("hidden");
    expect(title).toHaveAttribute("tabindex", "-1");
    expect(title).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Back to cases" }));

    expect(root).toHaveAttribute("data-detail-open", "false");
    expect(list).not.toHaveAttribute("hidden");
    expect(detail).toHaveAttribute("hidden");
    expect(row).toHaveFocus();
  });

  it("treats Escape as an explicit narrow Back and falls back to the list panel", () => {
    installMatchMedia(true);
    const { rerender } = render(<ListDetailHarness />);

    const row = screen.getByRole("button", { name: "Open case one" });
    row.focus();
    fireEvent.click(row);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(row).toHaveFocus();

    rerender(<ListDetailHarness removeRowOnBack />);
    const remainingRow = screen.getByRole("button", { name: "Open case one" });
    fireEvent.click(remainingRow);
    fireEvent.click(screen.getByRole("button", { name: "Back to cases" }));

    expect(screen.queryByRole("button", { name: "Open case one" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Cases" })).toHaveFocus();
  });

  it("locks every narrow return path while a detail operation is acting", () => {
    installMatchMedia(true);
    render(<ListDetailHarness initialSelection="case-1" locked />);

    const root = document.querySelector<HTMLElement>(".admin-list-detail");
    const back = screen.getByRole("button", { name: "Back to cases" });
    expect(root).toHaveAttribute("aria-busy", "true");
    expect(root).toHaveAttribute("data-navigation-locked", "true");
    expect(back).toBeDisabled();

    fireEvent.click(back);
    fireEvent.keyDown(document, { key: "Escape" });

    expect(screen.getByText("Confirmed case facts")).toBeVisible();
    expect(document.querySelector(".admin-list-detail__list")).toHaveAttribute("hidden");
  });

  it("lets a portalled modal own Escape before returning from the detail", () => {
    installMatchMedia(true);
    render(<ListDetailHarness withDialog />);

    const row = screen.getByRole("button", { name: "Open case one" });
    fireEvent.click(row);
    fireEvent.click(screen.getByRole("button", { name: "Open confirmation" }));
    const dialog = screen.getByRole("dialog", { name: "Confirm action" });

    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText("Confirmed case facts")).toBeVisible();

    fireEvent.keyDown(document.activeElement ?? document, { key: "Escape" });
    expect(row).toHaveFocus();
  });

  it("moves focus only when crossing into the narrow selected layout", () => {
    const media = installMatchMedia(false);
    render(<ListDetailHarness initialSelection="case-1" />);

    const outside = document.createElement("button");
    document.body.append(outside);
    outside.focus();
    expect(screen.getByRole("heading", { name: "Case one" })).not.toHaveFocus();

    act(() => media.setMatches(true));
    expect(screen.getByRole("heading", { name: "Case one" })).toHaveFocus();

    act(() => media.setMatches(false));
    outside.focus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(outside).toHaveFocus();
    expect(screen.getByText("Confirmed case facts")).toBeVisible();
  });

  it("keeps CSS and matchMedia on the same explicit breakpoint", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");
    expect(ADMIN_LIST_DETAIL_MEDIA_QUERY).toBe("(max-width: 1080px)");
    expect(styles).toContain("@media (max-width: 1080px)");
    expect(styles).toContain(".admin-list-detail[data-detail-open=\"false\"] > .admin-list-detail__detail");
    expect(styles).toContain(".admin-list-detail[data-detail-open=\"true\"] > .admin-list-detail__list");
  });
});
