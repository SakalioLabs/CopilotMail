// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AdminErrorNotice,
  AdminUiError,
  adminErrorPresentation,
} from "./AdminErrorNotice";
import { ApiError } from "./api";
import { translator } from "./i18n";

const en = translator("en");

afterEach(() => cleanup());

describe("AdminErrorNotice disclosure boundary", () => {
  it("renders only a bounded public code and request ID for API failures", () => {
    const retry = vi.fn();
    const error = new ApiError(503, {
      code: "audit_unavailable",
      message: "postgres://secret@internal-db:5432 shard east failed",
      request_id: "request-safe-503",
      trace_id: "trace-secret-503",
      retryable: true,
      details: { debug: "private policy reason" },
    });

    render(<AdminErrorNotice error={error} retry={retry} t={en} />);

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("audit_unavailable");
    expect(alert).toHaveTextContent("request-safe-503");
    expect(alert).not.toHaveTextContent("internal-db");
    expect(alert).not.toHaveTextContent("trace-secret-503");
    expect(alert).not.toHaveTextContent("private policy reason");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("does not offer a retry when the API explicitly marks it non-retryable", () => {
    render(
      <AdminErrorNotice
        error={new ApiError(403, {
          code: "forbidden",
          message: "sensitive authorization explanation",
          request_id: "request-safe-403",
          trace_id: "trace-secret-403",
          retryable: false,
        })}
        retry={vi.fn()}
        t={en}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("forbidden");
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("rejects malformed server identifiers instead of reflecting them", () => {
    const presentation = adminErrorPresentation(new ApiError(500, {
      code: "<internal secret>",
      message: "unused",
      request_id: "request id with spaces and secret",
      trace_id: "unused",
      retryable: true,
    }));

    expect(presentation).toEqual({
      code: "unexpected_error",
      requestId: undefined,
      retryable: true,
    });
  });

  it("allows only explicitly public local validation codes", () => {
    expect(adminErrorPresentation(
      new AdminUiError("retention_rule_invalid_json"),
    )).toEqual({
      code: "retention_rule_invalid_json",
      retryable: false,
    });
    expect(adminErrorPresentation(new Error("C:\\private\\stack"))).toEqual({
      code: "unexpected_error",
      retryable: true,
    });
  });
});
