// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { readRuntimeConfig } from "./config";

afterEach(() => {
  delete window.__COPILOTMAIL_MAIL_CONFIG__;
});

describe("mail runtime configuration", () => {
  it("accepts only a canonical lowercase mailbox preference", () => {
    window.__COPILOTMAIL_MAIL_CONFIG__ = {
      apiBaseUrl: "/api/",
      mailboxId: " 018f0000-0000-7000-8000-000000000001 ",
    };
    expect(readRuntimeConfig()).toEqual({
      apiBaseUrl: "/api",
      mailboxId: "018f0000-0000-7000-8000-000000000001",
    });

    for (const mailboxId of [
      "018F0000-0000-7000-8000-000000000001",
      "not-a-mailbox-id",
      "",
    ]) {
      window.__COPILOTMAIL_MAIL_CONFIG__ = { apiBaseUrl: "", mailboxId };
      expect(readRuntimeConfig().mailboxId).toBeUndefined();
    }
  });
});
