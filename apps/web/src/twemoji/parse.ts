import { parse } from "@twemoji/parser";

/** 画像を取得する jdecked/twemoji の版。パーサーの既定は @latest なので固定する */
const ASSETS = "https://cdn.jsdelivr.net/gh/jdecked/twemoji@17.0.3/assets/svg";

export type Segment = { type: "text"; text: string } | { type: "emoji"; text: string; url: string };

/** 文字列を、地の文と絵文字（Twemoji の画像の URL を持つ）に、出てくる順で分ける */
export function parseTwemoji(text: string): Segment[] {
  const segments: Segment[] = [];
  let end = 0;
  const pushText = (to: number) => {
    if (to > end) segments.push({ type: "text", text: text.slice(end, to) });
  };
  for (const entity of parse(text, { buildUrl: (code) => `${ASSETS}/${code}.svg` })) {
    const [from, to] = entity.indices as [number, number];
    pushText(from);
    segments.push({ type: "emoji", text: entity.text, url: entity.url });
    end = to;
  }
  pushText(text.length);
  return segments;
}
