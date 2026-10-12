import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_LOCALE,
  LOCALE_STORAGE_KEY,
  MESSAGES,
  TRANSLATION_FALLBACK,
  createTranslator,
  detectLocale,
  formatCount,
  formatDate,
  formatKilobytes,
  formatMessageCount,
  formatNumber,
  persistLocale,
  readLocale,
  translate,
  type LocaleStorage,
  type Translator,
} from "./i18n";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("mail app translations", () => {
  it("keeps both locale dictionaries and their interpolation parameters identical", () => {
    const chineseKeys = Object.keys(MESSAGES["zh-CN"]).sort();
    const englishKeys = Object.keys(MESSAGES.en).sort();
    expect(englishKeys).toEqual(chineseKeys);

    const placeholders = (value: string) =>
      Array.from(value.matchAll(/\{([A-Za-z][A-Za-z0-9_]*)\}/g), (match) => match[1]).sort();
    for (const key of chineseKeys) {
      expect(placeholders(MESSAGES.en[key as keyof typeof MESSAGES.en]))
        .toEqual(placeholders(MESSAGES["zh-CN"][key as keyof typeof MESSAGES["zh-CN"]]));
    }

    for (const [locale, catalog] of Object.entries(MESSAGES)) {
      for (const [key, value] of Object.entries(catalog)) {
        expect(value.trim(), `${locale}.${key}`).not.toBe("");
      }
    }
  });

  it("translates static and parameterized messages in both languages", () => {
    const chinese = createTranslator("zh-CN");
    const english = createTranslator("en");

    expect(chinese("nav.inbox")).toBe("收件箱");
    expect(english("nav.inbox")).toBe("Inbox");
    expect(chinese("delivery.completed", { delivered: 3, failed: 1 }))
      .toBe("投递完成：成功 3，失败 1");
    expect(english("delivery.acceptedProgress", { delivered: 2, total: 4 }))
      .toBe("Accepted by the mail server; delivered 2/4");
    expect(chinese("drafts.confirmTitle")).toBe("替换当前邮件？");
    expect(english("drafts.recipientCount.other", { count: 2 })).toBe("2 recipients");
    expect(english("error.draftRestoreConflict"))
      .toBe("This draft changed elsewhere; the list was refreshed");
  });

  it("fails closed for unknown keys and missing or invalid parameters", () => {
    const unsafe = createTranslator("en") as unknown as (
      key: string,
      params?: Record<string, unknown>,
    ) => string;

    expect(unsafe("not.a.real.key")).toBe(TRANSLATION_FALLBACK);
    expect(unsafe("delivery.completed", { delivered: 2 })).toBe(TRANSLATION_FALLBACK);
    expect(unsafe("delivery.completed", { delivered: Number.NaN, failed: 1 }))
      .toBe(TRANSLATION_FALLBACK);
    expect(unsafe("__proto__")).toBe(TRANSLATION_FALLBACK);
  });
});

describe("locale selection", () => {
  it("honours only the persisted selection and defaults to zh-CN", () => {
    const storage: LocaleStorage = {
      getItem: () => "en-US",
      setItem: vi.fn(),
    };
    expect(detectLocale({ storage })).toBe("en");
    expect(DEFAULT_LOCALE).toBe("zh-CN");
    expect(detectLocale({ storage: null })).toBe("zh-CN");
    expect(detectLocale({
      storage: { getItem: () => "fr", setItem: vi.fn() },
    })).toBe("zh-CN");
  });

  it("uses a deterministic fallback when no supported locale is stored", () => {
    expect(detectLocale({ storage: null })).toBe(DEFAULT_LOCALE);
    expect(detectLocale({ storage: null, fallback: "en" })).toBe("en");
  });

  it("reads and persists locale without allowing storage failures to escape", () => {
    const values = new Map<string, string>();
    const storage: LocaleStorage = {
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => { values.set(key, value); },
    };
    expect(persistLocale("en", storage)).toBe(true);
    expect(values.get(LOCALE_STORAGE_KEY)).toBe("en");
    expect(readLocale(storage)).toBe("en");

    const unavailable: LocaleStorage = {
      getItem: () => { throw new Error("storage unavailable"); },
      setItem: () => { throw new Error("storage unavailable"); },
    };
    expect(readLocale(unavailable)).toBeUndefined();
    expect(persistLocale("zh-CN", unavailable)).toBe(false);
  });
});

describe("locale-aware display formatting", () => {
  it("formats dates with locale-specific ordering", () => {
    const value = new Date("2026-07-27T12:00:00.000Z");
    const options = { timeZone: "UTC" } as const;

    expect(formatDate("zh-CN", value, options))
      .toBe(new Intl.DateTimeFormat("zh-CN", {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        timeZone: "UTC",
      }).format(value));
    expect(formatDate("en", value, options))
      .toBe(new Intl.DateTimeFormat("en", {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        timeZone: "UTC",
      }).format(value));
    expect(formatDate("en", "not-a-date")).toBe(TRANSLATION_FALLBACK);
  });

  it("formats numbers, message counts, and rounded-up KB values", () => {
    expect(formatNumber("en", 12345.5)).toBe("12,345.5");
    expect(formatCount("zh-CN", 12345.9)).toBe("12,345");
    expect(formatMessageCount("en", 1)).toBe("1 message");
    expect(formatMessageCount("en", 2)).toBe("2 messages");
    expect(formatMessageCount("zh-CN", 2)).toBe("2 封邮件");
    expect(formatKilobytes("en", 1537)).toBe("2 KB");
    expect(formatKilobytes("zh-CN", 0)).toBe("0 KB");
    expect(formatCount("en", -1)).toBe(TRANSLATION_FALLBACK);
    expect(formatKilobytes("en", Number.POSITIVE_INFINITY)).toBe(TRANSLATION_FALLBACK);
  });

  it("exposes the typed direct translator API", () => {
    const translator: Translator = createTranslator("en");
    expect(translate("en", "compose.replySubject", { subject: "Quarterly review" }))
      .toBe("Re: Quarterly review");
    expect(translator("detail.attachmentMeta", { mediaType: "text/plain", size: "2 KB" }))
      .toBe("text/plain · 2 KB");
  });
});
