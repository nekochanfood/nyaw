import { describe, expect, it } from "vitest";

import { negotiateLocale, switchLocale } from "@/i18n/config";
import { messages, translate } from "@/i18n/index";

const entries = (locale: "en" | "ja"): [string, string][] =>
  Object.entries(messages[locale]).flatMap(([ns, keys]) =>
    Object.entries(keys as Record<string, string>).map(
      ([key, text]) => [`${ns}.${key}`, text] as [string, string],
    ),
  );
const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe("翻訳リソース", () => {
  it("英語と日本語でキーと変数が一致する", () => {
    const ja = new Map(entries("ja"));
    for (const [key, text] of entries("en")) {
      expect(ja.has(key), key).toBe(true);
      expect(placeholders(ja.get(key) ?? ""), key).toEqual(placeholders(text));
    }
    expect(ja.size).toBe(entries("en").length);
  });

  it("英語はイギリス英語の綴りで書く", () => {
    const american =
      /\b(colors?|favorites?|behaviors?|centers?|minimiz\w*|localiz\w*|analyz\w*|catalog|license|canceled|toward|gray)\b/i;
    for (const [key, text] of entries("en")) expect(text, key).not.toMatch(american);
  });

  it("日本語の和欧文の間には半角スペースを入れ、数字の前後には入れない", () => {
    const kana = String.raw`\p{sc=Hiragana}\p{sc=Katakana}\p{sc=Han}`;
    const touching = new RegExp(`[${kana}][A-Za-z]|[A-Za-z][${kana}]`, "u");
    // 数を入れる変数も数字とみなす。「Apache-2.0」のように英字に続く数字は英字の一部として扱う
    const number = String.raw`((?<![A-Za-z\d.\-])\d+|\{(?:n|count|size)\})`;
    const spacedNumber = new RegExp(`[${kana}] ${number}|${number} [${kana}]`, "u");
    for (const [key, text] of entries("ja")) {
      expect(text, key).not.toMatch(touching);
      expect(text, key).not.toMatch(spacedNumber);
    }
  });
});

describe("translate", () => {
  it("言語ごとの訳を引く", () => {
    expect(translate("en", "nav", "download")).toBe("Download");
    expect(translate("ja", "nav", "download")).toBe("ダウンロード");
  });
});

describe("negotiateLocale", () => {
  it("cookie を Accept-Language より優先する", () => {
    expect(negotiateLocale("a=1; locale=ja", "en-GB,en;q=0.9")).toBe("ja");
  });

  it("対応していない cookie は無視して Accept-Language を見る", () => {
    expect(negotiateLocale("locale=fr", "fr-FR,ja;q=0.8")).toBe("ja");
  });

  it("どちらもなければ英語", () => {
    expect(negotiateLocale(null, null)).toBe("en");
    expect(negotiateLocale("", "de")).toBe("en");
  });
});

describe("switchLocale", () => {
  it("先頭の言語だけを替える", () => {
    expect(switchLocale("/ja/download", "en")).toBe("/en/download");
    expect(switchLocale("/en", "ja")).toBe("/ja");
    expect(switchLocale("/ja/", "en")).toBe("/en");
  });

  it("言語のないパスには前に付ける", () => {
    expect(switchLocale("/", "ja")).toBe("/ja");
    expect(switchLocale("/download", "en")).toBe("/en/download");
  });
});
