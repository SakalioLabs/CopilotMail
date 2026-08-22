import { beforeEach, describe, expect, it } from "vitest";
import {
  configurationProblems,
  readRuntimeConfig,
  sameOriginApiBaseUrl,
} from "./config";

const organizationId = "018f13b9-8bf2-7bc8-965d-48a28866f073";

describe("runtime configuration", () => {
  beforeEach(() => {
    window.__COPILOTMAIL_CONFIG__ = undefined;
  });

  it("contains only same-origin BFF routing and tenant context", () => {
    window.__COPILOTMAIL_CONFIG__ = {
      apiBaseUrl: "/control/",
      organizationId,
    };

    expect(readRuntimeConfig()).toEqual({
      apiBaseUrl: "/control",
      organizationId,
    });
  });

  it("rejects a cross-origin API base because BFF cookies are host-bound", () => {
    expect(configurationProblems({
      apiBaseUrl: "https://control.example.test",
      organizationId,
    })).toEqual(["apiBaseUrl.sameOrigin"]);
    expect(() => sameOriginApiBaseUrl("https://control.example.test"))
      .toThrow("api_base_url_must_be_same_origin");
  });

  it("accepts the empty same-origin proxy and a same-origin path prefix", () => {
    expect(configurationProblems({ apiBaseUrl: "", organizationId })).toEqual([]);
    expect(sameOriginApiBaseUrl("/control/")).toBe("/control");
  });
});
