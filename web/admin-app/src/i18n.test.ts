import { describe, expect, it } from "vitest";
import { resolveInitialLocale } from "./i18n";

describe("resolveInitialLocale", () => {
  it("uses either explicitly supported stored locale", () => {
    expect(resolveInitialLocale("zh-CN", "en-US")).toBe("zh-CN");
    expect(resolveInitialLocale("en", "zh-CN")).toBe("en");
  });

  it("ignores undeclared storage values and derives a supported browser locale", () => {
    expect(resolveInitialLocale("fr", "zh-Hans-CN")).toBe("zh-CN");
    expect(resolveInitialLocale("", "en-GB")).toBe("en");
    expect(resolveInitialLocale(null, "de-DE")).toBe("en");
  });
});
