import { Button } from "@copilotmail/design-system";
import { ApiError } from "./api";
import type { translator } from "./i18n";

type Translate = ReturnType<typeof translator>;

export interface AdminErrorPresentation {
  code: string;
  requestId?: string;
  retryable: boolean;
}

const PUBLIC_ERROR_CODE = /^[a-z][a-z0-9_.-]{0,99}$/;
const PUBLIC_REQUEST_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/;

/** An explicitly public, local UI validation code. */
export class AdminUiError extends Error {
  constructor(readonly code: string) {
    super(code);
    this.name = "AdminUiError";
  }
}

/**
 * Converts an untrusted transport error into a bounded support reference.
 * Server messages, trace IDs, details, URLs, and stack text never enter the DOM.
 */
export function adminErrorPresentation(error: unknown): AdminErrorPresentation {
  if (error instanceof AdminUiError) {
    return {
      code: PUBLIC_ERROR_CODE.test(error.code) ? error.code : "unexpected_error",
      retryable: false,
    };
  }
  if (!(error instanceof ApiError)) {
    return { code: "unexpected_error", retryable: true };
  }

  const code = typeof error.body.code === "string"
    && PUBLIC_ERROR_CODE.test(error.body.code)
    ? error.body.code
    : "unexpected_error";
  const requestId = typeof error.body.request_id === "string"
    && PUBLIC_REQUEST_ID.test(error.body.request_id)
    ? error.body.request_id
    : undefined;

  return {
    code,
    requestId,
    retryable: error.body.retryable === true,
  };
}

export function AdminErrorNotice({
  error,
  retry,
  t,
}: {
  error: unknown;
  retry?: () => void;
  t: Translate;
}) {
  const presentation = adminErrorPresentation(error);
  return (
    <div className="notice notice--error" role="alert">
      <div>
        <strong>{t("loadFailed")}</strong>
        <p><code>{presentation.code}</code></p>
        {presentation.requestId ? (
          <small>{t("requestId")}: {presentation.requestId}</small>
        ) : null}
      </div>
      {retry && presentation.retryable ? (
        <Button type="button" variant="secondary" onClick={retry}>
          {t("retry")}
        </Button>
      ) : null}
    </div>
  );
}
