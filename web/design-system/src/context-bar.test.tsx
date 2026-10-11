import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ContextBar, EvidenceInspector } from "./context-bar";
import { StatusIndicator } from "./index";

describe("ContextBar", () => {
  it("renders exact supplied facts as a named semantic region", () => {
    const { container } = render(
      <ContextBar
        actions={<button type="button">Open approval</button>}
        facts={[
          { id: "organization", label: "Organization", value: "Acme Mail" },
          { id: "resource", label: "Resource", value: "archive_4a9c" },
        ]}
        headingLevel={3}
        status={<StatusIndicator label="Approval required" tone="warning" />}
        title="Export context"
      />,
    );

    const region = screen.getByRole("region", { name: "Export context" });
    expect(screen.getByRole("heading", { level: 3, name: "Export context" })).toBeInTheDocument();
    expect(region).toContainElement(screen.getByText("Organization"));
    expect(region).toContainElement(screen.getByText("Acme Mail"));
    expect(region).toContainElement(screen.getByText("Resource"));
    expect(region).toContainElement(screen.getByText("archive_4a9c"));
    expect(screen.getByText("Approval required")).toHaveClass("status-indicator__label");
    expect(screen.getByText("Approval required").closest(".status-indicator"))
      .toHaveAttribute("data-status-tone", "warning");
    expect(screen.getByRole("button", { name: "Open approval" })).toBeInTheDocument();

    const definitionList = container.querySelector("dl.context-bar__facts");
    expect(definitionList?.querySelectorAll("dt")).toHaveLength(2);
    expect(definitionList?.querySelectorAll("dd")).toHaveLength(2);
  });

  it("does not invent status or actions when callers omit them", () => {
    const { container } = render(
      <ContextBar
        facts={[{ id: "actor", label: "Actor", value: "Teni" }]}
        title="Current scope"
      />,
    );

    expect(screen.getByRole("region", { name: "Current scope" })).toBeInTheDocument();
    expect(container.querySelector(".context-bar__status")).toBeNull();
    expect(container.querySelector(".context-bar__actions")).toBeNull();
    expect(container.querySelectorAll(".context-bar__fact")).toHaveLength(1);
  });
});

describe("EvidenceInspector", () => {
  it("starts closed by default and reports an uncontrolled native disclosure", async () => {
    const onOpenChange = vi.fn();
    const { container } = render(
      <EvidenceInspector count={3} summary="Approval evidence" onOpenChange={onOpenChange}>
        <p>Request ID: req_4a9c</p>
      </EvidenceInspector>,
    );

    const details = container.querySelector("details");
    const summary = screen.getByText("Approval evidence").closest("summary");
    expect(details).not.toHaveAttribute("open");
    expect(screen.getByText("Request ID: req_4a9c")).not.toBeVisible();
    expect(screen.getByText("3")).toBeInTheDocument();

    if (!summary) throw new Error("expected native evidence summary");
    fireEvent.click(summary);

    expect(details).toHaveAttribute("open");
    expect(screen.getByText("Request ID: req_4a9c")).toBeVisible();
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(true));
  });

  it("honors a controlled caller even when it declines an expansion request", async () => {
    const onOpenChange = vi.fn();
    const { container } = render(
      <EvidenceInspector open={false} summary="Immutable events" onOpenChange={onOpenChange}>
        <p>event_1</p>
      </EvidenceInspector>,
    );

    const details = container.querySelector("details");
    const summary = screen.getByText("Immutable events").closest("summary");
    if (!summary) throw new Error("expected native evidence summary");

    fireEvent.click(summary);

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(true));
    await waitFor(() => expect(details).not.toHaveAttribute("open"));
    expect(screen.getByText("event_1")).not.toBeVisible();
  });

  it("respects a caller-controlled open prop without creating a duplicate control", () => {
    const onOpenChange = vi.fn();
    const { container, rerender } = render(
      <EvidenceInspector open summary="Request lineage" onOpenChange={onOpenChange}>
        <p>approval_7d2f</p>
      </EvidenceInspector>,
    );

    expect(container.querySelector("details")).toHaveAttribute("open");
    expect(container.querySelector("summary")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("approval_7d2f")).toBeVisible();
    expect(screen.getByRole("group", { name: "Request lineage" })).toBeInTheDocument();
    expect(container.querySelector("summary")).toHaveTextContent("Request lineage");
    expect(container.querySelectorAll("button")).toHaveLength(0);

    const summary = screen.getByText("Request lineage").closest("summary");
    if (!summary) throw new Error("expected native evidence summary");
    fireEvent.click(summary);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(container.querySelector("details")).toHaveAttribute("open");

    rerender(
      <EvidenceInspector open={false} summary="Request lineage" onOpenChange={onOpenChange}>
        <p>approval_7d2f</p>
      </EvidenceInspector>,
    );
    expect(container.querySelector("details")).not.toHaveAttribute("open");
    expect(container.querySelector("summary")).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText("approval_7d2f")).not.toBeVisible();
  });
});
