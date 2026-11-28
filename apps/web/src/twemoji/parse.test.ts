import { describe, expect, it } from "vitest";

import { parseTwemoji } from "@/twemoji/parse";

const emoji = (text: string, code: string) => ({
  type: "emoji",
  text,
  url: `https://cdn.jsdelivr.net/gh/jdecked/twemoji@17.0.3/assets/svg/${code}.svg`,
});
const plain = (text: string) => ({ type: "text", text });

describe("parseTwemoji", () => {
  it("国旗は、2 つの地域指示記号をまとめて 1 つの絵文字にする", () => {
    expect(parseTwemoji("🇬🇧")).toEqual([emoji("🇬🇧", "1f1ec-1f1e7")]);
    expect(parseTwemoji("🇯🇵")).toEqual([emoji("🇯🇵", "1f1ef-1f1f5")]);
  });

  it("続けて書いた国旗は分ける", () => {
    expect(parseTwemoji("🇬🇧🇯🇵")).toEqual([emoji("🇬🇧", "1f1ec-1f1e7"), emoji("🇯🇵", "1f1ef-1f1f5")]);
  });

  it("地の文と絵文字を、出てくる順に分ける", () => {
    expect(parseTwemoji("a 🇯🇵 日本語 🐱!")).toEqual([
      plain("a "),
      emoji("🇯🇵", "1f1ef-1f1f5"),
      plain(" 日本語 "),
      emoji("🐱", "1f431"),
      plain("!"),
    ]);
  });

  it("絵文字がなければ地の文のまま返し、空の文字列は何も返さない", () => {
    expect(parseTwemoji("English")).toEqual([plain("English")]);
    expect(parseTwemoji("")).toEqual([]);
  });

  it("異体字セレクタは、ZWJ のない絵文字の画像名からだけ除く", () => {
    expect(parseTwemoji("❤️")).toEqual([emoji("❤️", "2764")]);
    expect(parseTwemoji("🏳️‍🌈")).toEqual([emoji("🏳️‍🌈", "1f3f3-fe0f-200d-1f308")]);
  });

  it("続けて呼んでも同じ結果になる", () => {
    expect(parseTwemoji("🐱")).toEqual(parseTwemoji("🐱"));
  });
});
