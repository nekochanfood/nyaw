import { parseTwemoji } from "@/twemoji/parse";

/**
 * 文字列の中の絵文字を Twemoji の画像にして表示する。OS のフォントに国旗がない環境（Windows）でも同じ見た目になる。
 * 画像を読み込めないときは、alt の絵文字が出る
 */
export function Twemoji({ text }: { text: string }) {
  return parseTwemoji(text).map((segment, i) =>
    segment.type === "emoji" ? (
      <img
        // oxlint-disable-next-line react/no-array-index-key -- 同じ文字列からは同じ並びになる
        key={i}
        src={segment.url}
        alt={segment.text}
        draggable={false}
        className="inline-block size-[1.25em] align-[-0.25em]"
      />
    ) : (
      segment.text
    ),
  );
}
