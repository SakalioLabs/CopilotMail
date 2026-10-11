// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AgentMailSubmissionEvidenceView from "./AgentMailSubmissionEvidenceView";
import { ApiError, type ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { AgentMailSubmissionEvidence } from "./types";

afterEach(() => cleanup());

const AGENT_ID = "019b0000-0000-7000-8000-000000000101";
const SUBMISSION_ID = "019b0000-0000-7000-8000-000000000109";
const SECOND_SUBMISSION_ID = "019b0000-0000-7000-8000-00000000010a";

function evidence(submissionId = SUBMISSION_ID): AgentMailSubmissionEvidence {
  return {
    agent_id: AGENT_ID,
    submission: {
      submission_id: submissionId,
      status: "accepted",
      created_at: "2026-08-24T09:00:00Z",
      updated_at: "2026-08-24T09:01:00Z",
    },
    admission: {
      admission_id: "019b0000-0000-7000-8000-000000000110",
      admitted_at: "2026-08-24T09:00:05Z",
      policy: {
        activation_id: "019b0000-0000-7000-8000-000000000111",
        activated_at: "2026-08-24T08:59:00Z",
        version_id: "019b0000-0000-7000-8000-000000000112",
        version: 4,
        approval_request_id: "019b0000-0000-7000-8000-000000000113",
      },
      recipient_count: 2,
      first_contact_count: 0,
      first_contact_approval_request_id: null,
    },
    delivery: null,
    audit_references: [
      {
        event_id: "019b0000-0000-7000-8000-000000000114",
        action: "mail.submission.requested",
        occurred_at: "2026-08-24T09:00:00Z",
      },
      {
        event_id: "019b0000-0000-7000-8000-000000000115",
        action: "agent.mail.admitted",
        occurred_at: "2026-08-24T09:00:05Z",
      },
      {
        event_id: "019b0000-0000-7000-8000-000000000116",
        action: "mail.submission.accepted",
        occurred_at: "2026-08-24T09:01:00Z",
      },
    ],
    observed_at: "2026-08-24T09:02:00Z",
  };
}

function view(getAgentMailSubmissionEvidence: ReturnType<typeof vi.fn>) {
  return render(
    <AgentMailSubmissionEvidenceView
      agentId={AGENT_ID}
      client={{ getAgentMailSubmissionEvidence } as unknown as ControlPlaneClient}
      locale="en"
      t={translator("en")}
    />,
  );
}

function submit(submissionId: string) {
  fireEvent.change(screen.getByRole("textbox", { name: /^Submission ID/ }), {
    target: { value: submissionId },
  });
  fireEvent.click(screen.getByRole("button", { name: "Read submission evidence" }));
}

describe("AgentMailSubmissionEvidenceView", () => {
  it("does not prefetch, poll, or invent an Agent mail-submission state", () => {
    const getAgentMailSubmissionEvidence = vi.fn();
    view(getAgentMailSubmissionEvidence);

    expect(screen.getByRole("textbox", { name: /^Submission ID/ })).toHaveValue("");
    expect(screen.getByText(/does not prefetch, poll, or retain this submission ID/)).toBeInTheDocument();
    expect(getAgentMailSubmissionEvidence).not.toHaveBeenCalled();
    expect(screen.queryByText("Latest delivery observation")).not.toBeInTheDocument();
  });

  it("rejects malformed, nil, and upper-case submission IDs locally", () => {
    const getAgentMailSubmissionEvidence = vi.fn();
    view(getAgentMailSubmissionEvidence);

    submit("not-a-uuid");
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a canonical mail-submission UUID.");

    submit("00000000-0000-0000-0000-000000000000");
    submit(SUBMISSION_ID.toUpperCase());
    expect(getAgentMailSubmissionEvidence).not.toHaveBeenCalled();
  });

  it("renders only returned evidence facts and keeps audit references collapsed", async () => {
    const getAgentMailSubmissionEvidence = vi.fn().mockResolvedValue(evidence());
    view(getAgentMailSubmissionEvidence);

    submit(SUBMISSION_ID);

    expect(await screen.findByRole("heading", { name: "Mail submission evidence" })).toBeInTheDocument();
    expect(getAgentMailSubmissionEvidence).toHaveBeenCalledWith(AGENT_ID, SUBMISSION_ID);
    expect(screen.getByText("This response did not return a delivery observation. It does not mean the submission was not delivered or failed.")).toBeInTheDocument();
    expect(screen.queryByText("Not delivered")).not.toBeInTheDocument();
    expect(screen.queryByText("recipient@example.com")).not.toBeInTheDocument();
    expect(screen.queryByText("smtp")).not.toBeInTheDocument();

    const disclosure = screen.getByText("Immutable audit references", { exact: true })
      .closest("details");
    expect(disclosure).not.toBeNull();
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByText("mail.submission.accepted")).toBeInTheDocument();
  });

  it("does not let a late pair response overwrite the latest explicit pair", async () => {
    let resolveFirst: (value: AgentMailSubmissionEvidence) => void = () => {};
    let resolveSecond: (value: AgentMailSubmissionEvidence) => void = () => {};
    const first = new Promise<AgentMailSubmissionEvidence>((resolve) => { resolveFirst = resolve; });
    const second = new Promise<AgentMailSubmissionEvidence>((resolve) => { resolveSecond = resolve; });
    const getAgentMailSubmissionEvidence = vi.fn()
      .mockReturnValueOnce(first)
      .mockReturnValueOnce(second);
    view(getAgentMailSubmissionEvidence);

    submit(SUBMISSION_ID);
    submit(SECOND_SUBMISSION_ID);
    await act(async () => { resolveSecond(evidence(SECOND_SUBMISSION_ID)); });

    expect(await screen.findByText(SECOND_SUBMISSION_ID)).toBeInTheDocument();
    await act(async () => { resolveFirst(evidence(SUBMISSION_ID)); });

    expect(screen.getByText(SECOND_SUBMISSION_ID)).toBeInTheDocument();
    expect(screen.queryByText(SUBMISSION_ID)).not.toBeInTheDocument();
  });

  it("keeps target-unavailable errors uniform and never reflects the server message", async () => {
    const getAgentMailSubmissionEvidence = vi.fn().mockRejectedValue(new ApiError(404, {
      code: "resource_not_found",
      message: "tenant-private submission details",
      request_id: "request-404",
      trace_id: "trace-404",
      retryable: false,
    }));
    view(getAgentMailSubmissionEvidence);

    submit(SUBMISSION_ID);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("This submission cannot be shown");
    expect(alert).toHaveTextContent("request-404");
    expect(alert).not.toHaveTextContent("tenant-private submission details");
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });
});
