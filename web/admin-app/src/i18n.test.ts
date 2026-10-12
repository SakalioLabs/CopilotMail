import { describe, expect, it } from "vitest";
import { CATALOGS, DEFAULT_LOCALE, resolveInitialLocale } from "./i18n";

describe("resolveInitialLocale", () => {
  it("uses either explicitly supported stored locale", () => {
    expect(resolveInitialLocale("zh-CN")).toBe("zh-CN");
    expect(resolveInitialLocale("en")).toBe("en");
  });

  it("defaults to zh-CN for absent or undeclared storage values", () => {
    expect(DEFAULT_LOCALE).toBe("zh-CN");
    expect(resolveInitialLocale("fr")).toBe("zh-CN");
    expect(resolveInitialLocale("")).toBe("zh-CN");
    expect(resolveInitialLocale(null)).toBe("zh-CN");
  });
});

describe("admin locale catalogs", () => {
  it("declares identical zh-CN and en key sets", () => {
    expect(Object.keys(CATALOGS["zh-CN"]).sort())
      .toEqual(Object.keys(CATALOGS.en).sort());
  });

  it("never ships an empty translation value", () => {
    for (const [locale, catalog] of Object.entries(CATALOGS)) {
      for (const [key, value] of Object.entries(catalog)) {
        expect(value.trim(), `${locale}.${key}`).not.toBe("");
      }
    }
  });
});
