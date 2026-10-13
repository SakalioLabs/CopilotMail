import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  BrandMark,
  EmptyApprovalsArt,
  EmptyAuditArt,
  EmptyComplianceArt,
  EmptyDomainsArt,
  EmptyDraftsArt,
  EmptyInboxArt,
  EmptyRolesArt,
  EmptySearchArt,
  EmptySentArt,
  Field,
  ICON_NAMES,
  Icon,
  Input,
  Select,
  SelectMessageArt,
  SelectRecordArt,
  Textarea,
  type IconName,
} from "./index";

describe("shared form primitives", () => {
  it("links a stable label and merged descriptions to its single control", () => {
    const view = render(
      <>
        <span id="existing-description">Existing constraint</span>
        <Field label="Policy key" hint="Server supplied key">
          <Input
            aria-describedby="existing-description"
            className="policy-input"
          />
        </Field>
      </>,
    );

    const input = screen.getByRole("textbox", { name: "Policy key" });
    const hint = screen.getByText("Server supplied key");
    const generatedId = input.id;
    expect(generatedId).not.toBe("");
    expect(screen.getByText("Policy key")).toHaveAttribute("for", generatedId);
    expect(input).toHaveAttribute(
      "aria-describedby",
      `existing-description ${hint.id}`,
    );
    expect(input).toHaveAccessibleDescription(
      "Existing constraint Server supplied key",
    );
    expect(input).toHaveClass("control", "policy-input");

    view.rerender(
      <>
        <span id="existing-description">Existing constraint</span>
        <Field label="Policy key" hint="Updated hint">
          <Input
            aria-describedby="existing-description"
            className="policy-input"
          />
        </Field>
      </>,
    );
    expect(screen.getByRole("textbox", { name: "Policy key" })).toHaveAttribute(
      "id",
      generatedId,
    );
  });

  it("preserves explicit ids and merges custom classes for every control", () => {
    render(
      <>
        <Field label="Outcome">
          <Select className="outcome-select" id="outcome">
            <option>Accepted</option>
          </Select>
        </Field>
        <Field label="Reason">
          <Textarea className="reason-textarea" id="reason" />
        </Field>
      </>,
    );

    expect(screen.getByRole("combobox", { name: "Outcome" }))
      .toHaveClass("control", "outcome-select");
    expect(screen.getByRole("textbox", { name: "Reason" }))
      .toHaveClass("control", "reason-textarea");
    expect(screen.getByText("Outcome")).toHaveAttribute("for", "outcome");
    expect(screen.getByText("Reason")).toHaveAttribute("for", "reason");
  });
});

describe("shared governance navigation icons", () => {
  it("renders every registered glyph as a non-empty decorative SVG on the common grid", () => {
    for (const name of ICON_NAMES) {
      const view = render(<Icon name={name} aria-hidden size={20} />);
      const svg = view.container.querySelector("svg");
      expect(svg).toHaveAttribute("viewBox", "0 0 24 24");
      expect(svg).toHaveAttribute("data-icon", name);
      expect(svg).toHaveAttribute("aria-hidden", "true");
      expect(svg?.childElementCount).toBeGreaterThan(0);
      view.unmount();
    }
  });

  const names = [
    "admin-overview",
    "admin-cases",
    "admin-holds",
    "admin-approvals",
    "admin-roles",
    "admin-agent-inspector",
    "admin-audit",
    "admin-retention",
    "admin-mail-domains",
  ] as const satisfies readonly IconName[];

  it("owns every Admin navigation glyph on the shared 24px grid", () => {
    for (const name of names) {
      expect(ICON_NAMES).toContain(name);
      const view = render(<Icon aria-hidden name={name} />);
      const icon = view.container.querySelector("svg");
      expect(icon).toHaveAttribute("data-icon", name);
      expect(icon).toHaveAttribute("viewBox", "0 0 24 24");
      expect(icon).toHaveAttribute("stroke-width", "1.75");
      expect(icon?.childElementCount).toBeGreaterThan(0);
      view.unmount();
    }
  });
});

describe("shared mail navigation icons", () => {
  const names = ["inbox", "sent", "draft"] as const satisfies readonly IconName[];

  it("owns every Mail navigation glyph on the shared 24px grid", () => {
    for (const name of names) {
      expect(ICON_NAMES).toContain(name);
      const view = render(<Icon aria-hidden name={name} />);
      const icon = view.container.querySelector("svg");
      expect(icon).toHaveAttribute("data-icon", name);
      expect(icon).toHaveAttribute("viewBox", "0 0 24 24");
      expect(icon).toHaveAttribute("stroke-width", "1.75");
      expect(icon?.childElementCount).toBeGreaterThan(0);
      view.unmount();
    }
  });
});

describe("shared empty-state illustrations", () => {
  const scenes = [
    EmptyInboxArt,
    EmptySearchArt,
    EmptyDraftsArt,
    EmptySentArt,
    SelectMessageArt,
    SelectRecordArt,
    EmptyApprovalsArt,
    EmptyAuditArt,
    EmptyComplianceArt,
    EmptyDomainsArt,
    EmptyRolesArt,
  ];

  it("renders every scene as a non-empty decorative SVG on the shared grid", () => {
    for (const Scene of scenes) {
      const view = render(<Scene />);
      const svg = view.container.querySelector("svg");
      expect(svg).toHaveAttribute("viewBox", "0 0 200 150");
      expect(svg).toHaveAttribute("aria-hidden", "true");
      expect(svg).toHaveAttribute("focusable", "false");
      expect(svg?.childElementCount).toBeGreaterThan(0);
      view.unmount();
    }
  });

  it("keeps the character logomark decorative beside the visible wordmark", () => {
    const view = render(<BrandMark />);
    const svg = view.container.querySelector("svg");
    expect(svg).toHaveAttribute("viewBox", "0 0 24 24");
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("focusable", "false");
    expect(svg?.childElementCount).toBeGreaterThan(0);
  });
});
