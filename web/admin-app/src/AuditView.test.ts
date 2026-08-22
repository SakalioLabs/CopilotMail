import { describe, expect, it } from "vitest";
import { auditFiltersFromDraft, type AuditDraftFilters } from "./AuditView";

function draft(from: string, to: string): AuditDraftFilters {
  return {
    from,
    to,
    action: " compliance.case.queried ",
    actor: "",
    resourceType: "compliance.case_query",
    resourceId: " case-1 ",
    outcome: "success",
    requestId: "",
  };
}

describe("audit filter boundary", () => {
  it("converts local input to exact ISO instants and trims optional values", () => {
    expect(auditFiltersFromDraft(draft(
      "2026-07-25T08:00",
      "2026-07-26T08:00",
    ))).toMatchObject({
      occurred_from: new Date("2026-07-25T08:00").toISOString(),
      occurred_to: new Date("2026-07-26T08:00").toISOString(),
      action: "compliance.case.queried",
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success",
    });
  });

  it("rejects reversed and over-31-day windows before making a request", () => {
    expect(auditFiltersFromDraft(draft(
      "2026-07-26T08:00",
      "2026-07-25T08:00",
    ))).toBeNull();
    expect(auditFiltersFromDraft(draft(
      "2026-06-01T08:00",
      "2026-07-26T08:00",
    ))).toBeNull();
  });
});
