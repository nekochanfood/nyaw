// フルロゴのイントロアニメーションを Lottie の JSON として生成する。
// 形と色は assets/logos の SVG から読むので、ロゴを描き直したら、このスクリプトを実行し直せば追従する。
// 実行: node assets/animations/logo-full/build.mjs
//
// 不透明度は一度も動かさない。見え隠れはすべて、拡大縮小、トリム、回転、マスク、マットで表す。

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const LOGOS = join(HERE, "../../logos");
const THEMES = ["Light", "Dark"];

// ---- 設定 ----

const FPS = 60;
const DURATION = 180; // フレーム数（3 秒）
// 合成の四辺に足す余白。出現の行き過ぎ、耳の回転、しっぽの揺れで、絵がロゴの外形より外へ出るため。
// プレイヤーは合成の範囲の外を描かないので、余白がないとその部分が欠ける
const MARGIN = 24;

// SVG のパスは、輪郭（サブパス）の並びで次の部品になっている。添字はその並びの位置。
// 補助の矩形（rects）は、輪郭線の隙間を埋めるために SVG にある線だけの四角で、近い部品に付ける。
const PARTS = {
  n: { paths: [0] },
  y: { paths: [1] },
  a: { paths: [2, 3] },
  w: { paths: [4] },
  ni: { paths: [6, 7, 8], rects: [1] },
  ya: { paths: [9] },
  u: { paths: [10, 11], rects: [0] },
  earL: { paths: [12] },
  earR: { paths: [13] },
};
const TAIL_PATH = 5;
const LETTERS = ["n", "y", "a", "w"];
const TEXT = Object.keys(PARTS).filter((n) => !n.startsWith("ear"));
const EARS = ["earL", "earR"];

// しっぽ: start で w の裏から滑り出し始め、slide フレームかけて元の位置に着く。伸びるのも start からで、grown で伸びきる。
// start は、w の出現の拡大が収まったあと（w が小さいうちは、裏に隠したしっぽがはみ出して見える）
// minScale は出始めの太さ（%）。そこから thicken フレームかけて、イーズインで元の太さに戻す。
// w と「う」の間を抜けて見え始めるとき（61 フレームごろ）はまだ細めで、輪郭に切り抜かれた丸が目立たず、
// 出てきてすぐには十分な太さになる。輪郭の幅は太さによらず一定にする
const TAIL = { start: 46, slide: 30, grown: 92, minScale: 20, thicken: 20 };

// しっぽ全体の回転（度）。先端を下げた向き（from）から、ばねで完成形の向き（0）へ戻す。
// release まで（しっぽが w から出てくるまで）は from のまま止めておき、そこで放す。
// 揺れに強弱を付けるため、ばねの速さを時間とともに落とす。最初の振りは速く鋭く（fast Hz）、そのあとの揺れ返しは
// ゆっくり穏やかになる（slow Hz へ、時定数 relax 秒で近づく）。damping は減衰比、until で残りの揺れをちょうど 0 にして止める
const SWING = { from: 35, release: 62, fast: 1.8, slow: 0.8, relax: 0.35, damping: 0.42, until: 160 };

// バブル（ロゴの地の円）は、ずれた位置から回り込むように漂って、settle で元の位置に収まる
const BUBBLES = { settle: 130, drift: 26, turns: 0.6 };

// 幅のある明るい帯が、斜めに左から右へ横切る。動きは circ の in-out（出だしと終わりがゆっくりで、中ほどを一気に抜ける）。
// 時間は、帯がロゴの上を通る部分がリファレンスと同じ約 0.4 秒になる長さ。circ の in-out は中ほどが速いので、全体はその数倍になる。
// 帯の中心は、x が fromX から toX まで動く（始めと終わりで、細い帯を含めてどの部品にも掛からない位置）。
// 色はテーマごとに、塗りより明るい 2 色（太い帯、その後ろを追う細い帯）
const SWEEP = {
  from: 64,
  to: 174,
  fromX: -84,
  toX: 506,
  y: 75,
  angle: 28,
  colors: { Light: ["#ffffff", "#ffffff"], Dark: ["#c0c7e2", "#a9b0cf"] },
};

// 耳としっぽを、文字の裏に隠れている間に確実に見えなくするマスクの位置（revealMasks を参照）
const REVEAL = { below: 10, inset: 2 };

// ---- SVG の読み取り ----

// ロゴの SVG から、輪郭（塗りと同じ形）、補助の矩形、バブルの円、色、輪郭線の幅を読む。座標は余白の分だけずらす
function parseSvg(file) {
  const src = readFileSync(join(LOGOS, file), "utf8");
  const classes = {};
  const style = src.match(/<style>(.*?)<\/style>/s)[1];
  for (const [, sel, body] of style.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const props = Object.fromEntries(
      body
        .split(";")
        .filter(Boolean)
        .map((kv) => kv.split(":").map((s) => s.trim())),
    );
    for (const name of sel.split(",")) {
      const key = name.trim().replace(/^\./, "");
      classes[key] = { ...classes[key], ...props };
    }
  }
  // clipPath の中身は、塗りのパスと同じ形なので読み飛ばす
  const body = src.replace(/<clipPath.*?<\/clipPath>/s, "");
  let outlinePath, fillPath, outline, fill, width;
  const rects = [];
  const circles = [];
  for (const [, tag, attrs] of body.matchAll(/<(path|rect|circle)\b([^>]*)\/>/g)) {
    const attr = (name) => attrs.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];
    const cls = classes[attr("class")];
    if (tag === "path" && cls.stroke) {
      [outlinePath, outline, width] = [attr("d"), cls.stroke, parseFloat(cls["stroke-width"])];
    } else if (tag === "path") [fillPath, fill] = [attr("d"), cls.fill];
    else if (tag === "rect") rects.push(["x", "y", "width", "height"].map((k) => +attr(k)));
    else circles.push({ c: [+attr("cx"), +attr("cy")], r: +attr("r"), color: cls.fill });
  }
  if (outlinePath !== fillPath) throw new Error(`${file}: 輪郭と塗りのパスが違う`);
  const viewBox = src.match(/viewBox="([^"]*)"/)[1].split(" ").map(Number);
  const shift = (p) => add(p, [MARGIN, MARGIN]);
  return {
    size: [Math.ceil(viewBox[2] + 2 * MARGIN), Math.ceil(viewBox[3] + 2 * MARGIN)],
    contours: parsePath(fillPath).map((segs) => segs.map((seg) => seg.map(shift))),
    rects: rects.map(([x, y, w, h]) => [x + MARGIN, y + MARGIN, w, h]),
    circles: circles.map((c) => ({ ...c, c: shift(c.c) })),
    outline,
    fill,
    width,
  };
}

// SVG のパスを、輪郭ごとの 3 次ベジェの並び [p0, c1, c2, p1] にする。
// このロゴが使う M, c, s, l, h, v, Z だけを扱う
function parsePath(d) {
  const tokens = d.match(/[a-zA-Z]|-?(?:\d+\.?\d*|\.\d+)/g);
  const contours = [];
  let i = 0;
  let cmd, cur, start, lastCtrl, segs;
  const num = () => +tokens[i++];
  const rel = () => add(cur, [num(), num()]);
  while (i < tokens.length) {
    if (/[a-zA-Z]/.test(tokens[i])) cmd = tokens[i++];
    if (cmd === "M") {
      cur = start = [num(), num()];
      contours.push((segs = []));
      cmd = "L";
      continue;
    }
    if (cmd === "Z" || cmd === "z") {
      if (dist(cur, start) > 0.01) segs.push([cur, cur, start, start]);
      cur = start;
      continue;
    }
    let seg;
    if (cmd === "c") seg = [cur, rel(), rel(), rel()];
    else if (cmd === "s") seg = [cur, lastCtrl ? sub(scale(cur, 2), lastCtrl) : cur, rel(), rel()];
    else {
      const to = cmd === "l" ? rel() : cmd === "L" ? [num(), num()] : cmd === "h" ? add(cur, [num(), 0]) : add(cur, [0, num()]);
      seg = [cur, cur, to, to];
    }
    const curved = cmd === "c" || cmd === "s";
    lastCtrl = curved ? seg[2] : undefined;
    cur = seg[3];
    // 長さのない線（"h0" など）は頂点を重ねるだけなので捨てる
    if (curved || dist(seg[3], seg[0]) > 1e-6) segs.push(seg);
  }
  return contours;
}

// ---- 幾何 ----

const add = (a, b) => [a[0] + b[0], a[1] + b[1]];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1]];
const scale = (a, k) => [a[0] * k, a[1] * k];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1];
const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
const unit = (a) => scale(a, 1 / Math.hypot(...a));
const lerp = (a, b, t) => add(a, scale(sub(b, a), t));
const r2 = (v) => Math.round(v * 100) / 100;
const roundPts = (ps) => ps.map((p) => p.map(r2));
const clamp01 = (v) => Math.min(Math.max(v, 0), 1);

function bezier([p0, c1, c2, p1], t) {
  const u = 1 - t;
  return [0, 1].map((k) => u * u * u * p0[k] + 3 * u * u * t * c1[k] + 3 * u * t * t * c2[k] + t * t * t * p1[k]);
}

// ベジェの並びを、各セグメント n 点の折れ線にする（終点は次のセグメントの始点と重なるので入れない）
const polyline = (segs, n) => segs.flatMap((seg) => Array.from({ length: n }, (_, k) => bezier(seg, k / n)));

// 折れ線の各点までの累積の長さ
function arcLengths(pts) {
  const acc = [0];
  for (let k = 1; k < pts.length; k++) acc.push(acc[k - 1] + dist(pts[k], pts[k - 1]));
  return acc;
}

function bbox(contours) {
  const pts = contours.flat().flatMap((seg) => Array.from({ length: 9 }, (_, k) => bezier(seg, k / 8)));
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

// ベジェの並び（開いた線）を、弧長で等間隔に n 点取る
function resample(segs, n) {
  const poly = [...polyline(segs, 40), segs.at(-1)[3]];
  const acc = arcLengths(poly);
  return Array.from({ length: n }, (_, j) => {
    const target = (acc.at(-1) * j) / (n - 1);
    const k = acc.findIndex((a) => a >= target);
    if (k <= 0) return poly[Math.max(k, 0)];
    return lerp(poly[k - 1], poly[k], (target - acc[k - 1]) / (acc[k] - acc[k - 1]));
  });
}

// ベジェの並び（閉じた輪郭）を Lottie の形 { c, v, i, o } にする
function toShape(segs) {
  return {
    c: true,
    v: roundPts(segs.map((s) => s[0])),
    i: roundPts(segs.map((_, k) => sub(segs.at(k - 1)[2], segs.at(k - 1)[3]))),
    o: roundPts(segs.map((s) => sub(s[1], s[0]))),
  };
}

// しっぽは、幅の一定な線を丸い端で太らせた輪郭になっている（下の縁 → 先端の丸み → 上の縁 → 根元の丸み）。
// 下の縁の各点と、上の縁で一番近い点との中点を芯線の点とし、根元と先端を頂点とする 1 本の 3 次ベジェに当てはめる。
// 幅は、対にした 2 点の距離の平均とする
function tailSpine(segs) {
  const upper = resample(segs.slice(4, 8), 400);
  const pairs = resample(segs.slice(0, 3), 120).map((p) => [p, upper.reduce((best, q) => (dist(p, q) < dist(p, best) ? q : best))]);
  const mids = pairs.map(([p, q]) => lerp(p, q, 0.5));
  const width = pairs.reduce((s, [p, q]) => s + dist(p, q), 0) / pairs.length;
  return { spine: fitCubic(mids), width };
}

// 点列を、両端を頂点とする 1 本の 3 次ベジェに当てはめ、Lottie の開いた形にする（Schneider の方法）。
// 両端の接線は端の近くの点から決め、ハンドルの長さだけを最小二乗で求める
function fitCubic(pts) {
  const [p0, p3] = [pts[0], pts.at(-1)];
  const t0 = unit(sub(pts[2], p0));
  const t3 = unit(sub(pts.at(-3), p3));
  const acc = arcLengths(pts);
  let [c00, c01, c11, x0, x1] = [0, 0, 0, 0, 0];
  pts.forEach((q, k) => {
    const u = acc[k] / acc.at(-1);
    const [b0, b1, b2, b3] = [(1 - u) ** 3, 3 * u * (1 - u) ** 2, 3 * u * u * (1 - u), u ** 3];
    const a1 = scale(t0, b1);
    const a2 = scale(t3, b2);
    const r = sub(q, add(scale(p0, b0 + b1), scale(p3, b2 + b3)));
    c00 += dot(a1, a1);
    c01 += dot(a1, a2);
    c11 += dot(a2, a2);
    x0 += dot(a1, r);
    x1 += dot(a2, r);
  });
  const det = c00 * c11 - c01 * c01;
  const out = scale(t0, (x0 * c11 - x1 * c01) / det);
  const into = scale(t3, (c00 * x1 - c01 * x0) / det);
  return { c: false, v: roundPts([p0, p3]), i: roundPts([[0, 0], into]), o: roundPts([out, [0, 0]]) };
}

// from から dir の向きへ進み、輪郭の内側で、輪郭から depth 以上離れた最初の点を探す。
// そこまで深い点がなければ、届く一番深さのある点を探す。見つけた点と、その点の深さを返す
function pointInside(contours, from, dir, depth) {
  const polys = contours.map((segs) => polyline(segs, 12));
  const inside = ([x, y]) =>
    polys.reduce((acc, poly) => {
      let hit = false;
      for (let k = 0, j = poly.length - 1; k < poly.length; j = k++) {
        const [a, b] = [poly[k], poly[j]];
        if (a[1] > y !== b[1] > y && x < ((b[0] - a[0]) * (y - a[1])) / (b[1] - a[1]) + a[0]) hit = !hit;
      }
      return acc !== hit;
    }, false);
  const edge = (p) =>
    Math.min(
      ...polys.flatMap((poly) =>
        poly.map((a, k) => {
          const d = sub(poly[(k + 1) % poly.length], a);
          return dist(p, add(a, scale(d, clamp01(dot(sub(p, a), d) / dot(d, d)))));
        }),
      ),
    );
  const ray = Array.from({ length: 400 }, (_, k) => add(from, scale(unit(dir), k / 2)))
    .filter(inside)
    .map((p) => ({ p, depth: edge(p) }));
  if (ray.length === 0) throw new Error("この向きには、輪郭の内側の点がない");
  const reach = Math.min(depth, Math.max(...ray.map((q) => q.depth)));
  const found = ray.find((q) => q.depth >= reach);
  return { point: roundPts([found.p])[0], depth: reach };
}

// CSS の easeInOutCirc
const circInOut = (u) => (u < 0.5 ? (1 - Math.sqrt(1 - (2 * u) ** 2)) / 2 : (Math.sqrt(1 - (2 - 2 * u) ** 2) + 1) / 2);

// ---- Lottie の部品 ----

const hex = (c) => [1, 3, 5].map((k) => r2(parseInt(c.slice(k, k + 2), 16) / 255)).concat(1);
const still = (k) => ({ a: 0, k });
const EASE = { o: { x: [0.33], y: [0] }, i: { x: [0.67], y: [1] } };
const LINEAR = { o: { x: [0], y: [0] }, i: { x: [1], y: [1] } };
// CSS の easeInOutCirc に相当する 3 次ベジェ
const CIRC_IN_OUT = { o: { x: [0.85], y: [0] }, i: { x: [0.15], y: [1] } };

// [フレーム, 値] の並びをキーフレームにする。値は数か配列
function keys(list, ease = EASE) {
  const wrap = (v) => (Array.isArray(v) ? v : [v]);
  return {
    a: 1,
    k: list.map(([t, v], k) => (k === list.length - 1 ? { t, s: wrap(v) } : { t, s: wrap(v), ...ease })),
  };
}

// 関数 f(t) の値を、from から to まで 2 フレームごとに置いた、線形につなぐキーフレーム
function sampled(from, to, f) {
  const list = [];
  for (let t = from; t <= to; t += 2) list.push([t, f(t)]);
  return keys(list, LINEAR);
}

// keyframes の前に、frame 0 から最初のキーフレームまで value のまま止めておくキーフレームを足す（h: 1 は次まで値を変えない）
const holdFirst = (value, keyframes) => ({ a: 1, k: [{ t: 0, s: value, h: 1 }, ...keyframes.k] });

function transform({ anchor = [0, 0], position = still(anchor), scale = still([100, 100]), rotation = still(0) } = {}) {
  return { a: still(anchor), p: position, s: scale, r: rotation, o: still(100) };
}

const group = (name, items, tr = transform()) => ({ ty: "gr", nm: name, it: [...items, { ty: "tr", ...tr }] });
const pathItem = (shape) => ({ ty: "sh", ks: shape.a === 1 ? shape : still(shape) });
const fillItem = (color) => ({ ty: "fl", c: still(hex(color)), o: still(100), r: 1 });
// lc と lj の 2 は丸い端と丸い角
// w は数か、キーフレーム
const strokeItem = (color, w, lc = 2) => ({ ty: "st", c: still(hex(color)), o: still(100), w: typeof w === "number" ? still(w) : w, lc, lj: 2 });
const rectItem = (center, size) => ({ ty: "rc", d: 1, p: still(roundPts([center])[0]), s: still(size), r: still(0) });

let nextInd = 1;
function layer(nm, props) {
  return { ddd: 0, ind: nextInd++, ty: 4, nm, sr: 1, ks: transform(), ao: 0, ip: 0, op: DURATION, st: 0, bm: 0, ...props };
}

// ---- 部品の動き ----

// ステッカーが貼られるように、0 から行き過ぎて戻る。横と縦をずらして、弾む感じを出す。
// tilt は出だしの傾き（度）、stretch は行き過ぎたときの横の伸び（1 で 118%）
function pop(t0, { tilt = 8, stretch = 1 } = {}) {
  return {
    scale: keys([
      [t0, [0, 0]],
      [t0 + 9, [118 * stretch, 90]],
      [t0 + 15, [92, 108]],
      [t0 + 21, [103, 97]],
      [t0 + 27, [100, 100]],
    ]),
    rotation: keys([
      [t0, -tilt * 2],
      [t0 + 10, tilt * 0.6],
      [t0 + 18, -tilt * 0.25],
      [t0 + 27, 0],
    ]),
  };
}

// 耳は、文字の奥から回りながらせり上がって出てきて、そのあと付け根を中心にぴくっと動く（twitches はその開始フレーム）。
// 出始めは付け根より下（文字の裏）に置き、小さな大きさから始める。dir は左の耳が 1、右の耳が -1
function ear(t0, dir, twitches) {
  const rotation = [
    [t0, -50 * dir],
    [t0 + 18, 12 * dir],
    [t0 + 27, -5 * dir],
    [t0 + 36, 0],
  ];
  const scale = [
    [t0, [45, 45]],
    [t0 + 16, [106, 106]],
    [t0 + 26, [97, 103]],
    [t0 + 36, [100, 100]],
  ];
  const offset = [
    [t0, [0, 52]],
    [t0 + 16, [0, -5]],
    [t0 + 26, [0, 2]],
    [t0 + 34, [0, 0]],
  ];
  for (const t of twitches) {
    rotation.push([t, 0], [t + 4, -16 * dir], [t + 9, 6 * dir], [t + 15, 0]);
    scale.push([t, [100, 100]], [t + 4, [104, 90]], [t + 9, [98, 103]], [t + 15, [100, 100]]);
  }
  // 出番までは大きさ 0 のまま止めておく
  return { rotation: keys(rotation), scale: holdFirst([0, 0], keys(scale)), offset };
}

const MOTION = {
  n: pop(4),
  y: pop(9, { tilt: -8 }),
  a: pop(14),
  w: pop(19, { tilt: -8 }),
  ni: pop(32, { stretch: 0.95 }),
  ya: pop(42, { tilt: -10, stretch: 0.95 }),
  u: pop(52, { stretch: 0.95 }),
  // 耳は、下にある文字（左の耳は n と y、右の耳は a と w）の出現の拡大が収まってから出す
  earL: ear(38, 1, [88, 106]),
  earR: ear(46, -1, [120]),
};

// しっぽ全体の回転（度）を 2 フレームごとに求める（SWING を参照）。ばねの式を数値積分する
function tailSwing() {
  const STEPS = 8; // 1 フレームを分ける数（積分を安定させる）
  const dt = 1 / FPS / STEPS;
  let [angle, vel] = [SWING.from, 0];
  const out = [];
  for (let t = TAIL.start; t <= SWING.until; t++) {
    if ((t - TAIL.start) % 2 === 0 || t === SWING.until) {
      // 止まるフレームでちょうど 0 になるよう、終わり際だけ緩やかに絞る
      const fade = 1 - ((t - TAIL.start) / (SWING.until - TAIL.start)) ** 4;
      out.push([t, r2(angle * fade)]);
    }
    if (t < SWING.release) continue;
    const tau = (t - SWING.release) / FPS;
    const w = 2 * Math.PI * (SWING.slow + (SWING.fast - SWING.slow) * Math.exp(-tau / SWING.relax));
    for (let k = 0; k < STEPS; k++) {
      vel += (-w * w * angle - 2 * SWING.damping * w * vel) * dt;
      angle += vel * dt;
    }
  }
  return out;
}

// ---- 層 ----

const contoursOf = (svg, name) => PARTS[name].paths.map((k) => svg.contours[k]);

// 部品の回転と拡大の中心。耳は付け根（輪郭の一番下の 2 頂点の中点）、ほかは外接矩形の中心
function pivot(name, contours) {
  if (name.startsWith("ear")) {
    const [p, q] = contours[0].map((s) => s[0]).sort((p, q) => q[1] - p[1]);
    return lerp(p, q, 0.5);
  }
  const [x0, y0, x1, y1] = bbox(contours);
  return [(x0 + x1) / 2, (y0 + y1) / 2];
}

// 部品をまとめた 1 つのシェイプレイヤーを作る。kind は "fill"（塗り）か "outline"（輪郭）。
// 部品はそれぞれグループにし、動きはグループのトランスフォームに持たせる。塗りやストロークは、グループの外に 1 つだけ置き、
// 全部品にまとめて掛ける。ストロークの幅が部品の拡大の影響を受けず、ロゴ全体の輪郭として一定の幅になる
function partsLayer(svg, names, kind) {
  const groups = names.map((name) => {
    const contours = contoursOf(svg, name);
    const anchor = pivot(name, contours).map(r2);
    const { scale, rotation, offset } = MOTION[name];
    const position = offset ? keys(offset.map(([t, d]) => [t, roundPts([add(anchor, d)])[0]])) : still(anchor);
    // 補助の矩形は輪郭の隙間を埋めるためのものなので、輪郭の側にだけ入れる
    const rects = kind === "fill" ? [] : (PARTS[name].rects ?? []).map((k) => svg.rects[k]).map(([x, y, w, h]) => rectItem([x + w / 2, y + h / 2], [w, h]));
    return group(name, [...contours.map((c) => pathItem(toShape(c))), ...rects], transform({ anchor, position, scale, rotation }));
  });
  // 閉じた形に端の形は効かないが、大きさ 0 の形では丸い端が点として描かれてしまうので、平らな端（1）にする
  const paint = kind === "fill" ? fillItem(svg.fill) : strokeItem(svg.outline, svg.width, 1);
  return layer(`${names.join(" ")} ${kind === "fill" ? "塗り" : "輪郭"}`, { shapes: [...groups, paint] });
}

// しっぽ: 芯線を太い線で描き、トリムで伸ばす。開いた線なので、他の部品とまとめた塗りには入れられず、塗りも輪郭も線の太さで描く。
// 芯線の形は完成形のまま変えず、層の位置、大きさ、回転で動かす
function tailLayers(svg, theme) {
  const { spine, width } = tailSpine(svg.contours[TAIL_PATH]);
  const root = spine.v[0];

  // 伸び具合（0〜100）。出始めは、丸い端どうしが重なって 1 つの丸に見えるだけの、ごく短い長さにしておく（w の裏に隠れている）。
  // そこから circ の in-out で伸びきる。出番の前は長さ 0 で何も描かない（丸 1 つ分の長さにしておくと、w が出る前から丸が見える）
  const DOT = 0.5;
  const growth = (t) => DOT + (100 - DOT) * circInOut(clamp01((t - TAIL.start) / (TAIL.grown - TAIL.start)));
  const trim = { ty: "tm", s: still(0), e: holdFirst([0], sampled(TAIL.start, TAIL.grown, (t) => r2(growth(t)))), o: still(0), m: 1 };
  const line = (name, color, w) => group(name, [pathItem(spine), trim, strokeItem(color, w)]);

  // 出始めは w の裏に隠し、伸びながら元の位置へ滑り出させる。しっぽは w より奥の段にあり、w の右端より左はマスクで隠れるので、
  // w の裏にある間は見えない。隠し場所は、根元の制御点のハンドルの向きを根元から逆に辿り、w の塗りの内側で輪郭から
  // 半幅以上離れた最初の点（その向きに届かなければ、一番深い点）。丸はこの向きに沿って滑り出す
  const { point: hidden } = pointInside(contoursOf(svg, "w"), root, scale(spine.o[0], -1), width / 2);
  const progress = (t) => (t - TAIL.start) / TAIL.slide;
  const motion = transform({
    anchor: root,
    position: sampled(TAIL.start, TAIL.start + TAIL.slide, (t) => roundPts([lerp(hidden, root, circInOut(progress(t)))])[0]),
    rotation: keys(tailSwing(), LINEAR),
  });
  // 塗りの線の太さ。イーズイン（3 次）で、前半は細いまま、後半で一気に元の太さになる。
  // 層ごと拡大縮小すると輪郭の幅まで変わるので、線の太さだけを動かす。輪郭はこれに、ロゴ共通の輪郭の幅を足した太さにする
  const thickness = (extra) =>
    sampled(TAIL.start, TAIL.start + TAIL.thicken, (t) => {
      const u = (t - TAIL.start) / TAIL.thicken;
      return r2((width * (TAIL.minScale + (100 - TAIL.minScale) * u ** 3)) / 100 + extra);
    });
  const fill = (nm, props = {}) => layer(nm, { ks: motion, shapes: [line("しっぽ", svg.fill, thickness(0))], ...props });
  return {
    // バブルと帯は、しっぽの塗りを型にして切り抜く。型はしっぽと同じ線なので、動きに合わせて切り抜く形も変わる
    fills: [fill("しっぽ バブルの型", { td: 1 }), layer("しっぽ バブル", { tt: 1, shapes: decorations(svg, theme) }), fill("しっぽ 塗り")],
    outline: layer("しっぽ 輪郭", { ks: motion, shapes: [line("しっぽ", svg.outline, thickness(svg.width))] }),
  };
}

// 耳としっぽを、文字の裏に隠れている間に確実に見えなくするマスク。2 つの範囲を足し合わせる。
// - 耳の範囲: 文字（n、y、a、w）の上端から REVEAL.below だけ下がった線より上。出始めの耳はこの線より下にあるので、
//   文字がまだ小さいときや文字の隙間からも見えない。線より下に残る付け根は、完成時には文字に隠れる範囲
// - しっぽの範囲: w の右端から REVEAL.inset だけ左の線より右。w の裏にある間は見えず、この線を越えたところから現れる
// マスクは層の座標で固定され、部品の動きとは一緒に動かない
function revealMasks(svg) {
  const top = Math.min(...LETTERS.map((n) => bbox(contoursOf(svg, n))[1]));
  const right = bbox(contoursOf(svg, "w"))[2];
  const FAR = 10000;
  const rect = (x0, y0, x1, y1) => ({
    c: true,
    v: roundPts([
      [x0, y0],
      [x1, y0],
      [x1, y1],
      [x0, y1],
    ]),
    i: Array(4).fill([0, 0]),
    o: Array(4).fill([0, 0]),
  });
  const mask = (nm, shape) => ({ inv: false, mode: "a", pt: still(shape), o: still(100), x: still(0), nm });
  return [mask("耳の範囲", rect(-FAR, -FAR, FAR, top + REVEAL.below)), mask("しっぽの範囲", rect(right - REVEAL.inset, -FAR, FAR, FAR))];
}

// 塗りの上に重ねる飾り（斜めの帯と、バブル）。型で切り抜いて使う
const decorations = (svg, theme) => [...sweepGroups(theme), ...bubbleGroups(svg.circles)];

// 斜めの帯（SWEEP を参照）
function sweepGroups(theme) {
  const [wide, thin] = SWEEP.colors[theme];
  const position = keys(
    [
      [SWEEP.from, [SWEEP.fromX + MARGIN, SWEEP.y + MARGIN]],
      [SWEEP.to, [SWEEP.toX + MARGIN, SWEEP.y + MARGIN]],
    ],
    CIRC_IN_OUT,
  );
  const band = (name, color, width, offset) =>
    group(name, [rectItem([offset, 0], [width, 400]), fillItem(color)], transform({ position, rotation: still(SWEEP.angle) }));
  return [band("帯 細", thin, 14, -52), band("帯 太", wide, 56, 0)];
}

// バブルを SVG と同じ重なり順で描く。SVG は後の要素が手前、Lottie は先の要素が手前なので、順を逆にする。
// 各バブルは元の位置のまわりを、ずれを縮めながら回り込み、小さめの半径から元の半径に戻る
function bubbleGroups(circles) {
  return circles
    .map(({ c, r, color }, k) => {
      const p = [];
      const size = [];
      for (let t = 0; t <= BUBBLES.settle; t += 4) {
        const u = t / BUBBLES.settle;
        const left = (1 - u) ** 2; // 残りのずれ。終わりに向けて緩やかに 0 になる
        const ang = k * 2.4 + 2 * Math.PI * BUBBLES.turns * u * (k % 2 ? 1 : -1);
        p.push([t, roundPts([add(c, scale([Math.cos(ang), Math.sin(ang)], BUBBLES.drift * left))])[0]]);
        size.push([t, Array(2).fill(r2(2 * r * (1 - 0.3 * left)))]);
      }
      return group(`バブル ${k}`, [{ ty: "el", d: 1, p: keys(p, LINEAR), s: keys(size, LINEAR) }, fillItem(color)]);
    })
    .reverse();
}

// ---- 合成 ----

// 文字の段と、その奥の耳としっぽの段に分けて重ねる。段ごとに、塗り、輪郭、バブルを持つ。
// 段を分けるのは、耳としっぽの輪郭と塗りを文字の輪郭より奥に置き、文字の下から出てくるようにするため
function build(theme) {
  nextInd = 1;
  const svg = parseSvg(`Logo_Full_${theme}.svg`);
  const [w, h] = svg.size;
  const assets = [
    { id: "front", nm: "文字の塗り", layers: [partsLayer(svg, TEXT, "fill")] },
    { id: "back", nm: "耳の塗り", layers: [partsLayer(svg, EARS, "fill")] },
  ];
  const tail = tailLayers(svg, theme);
  const reveal = revealMasks(svg);
  const precomp = (nm, refId, props = {}) => ({ ...layer(nm, props), ty: 0, refId, w, h });
  // 塗りの合成を型（アルファマット）にして、直後のバブルと帯をその段の形で切り抜く
  const tier = (id, name, rest) => [
    precomp(`${name} バブルの型`, id, { td: 1 }),
    layer(`${name} バブル`, { tt: 1, shapes: decorations(svg, theme) }),
    precomp(`${name} 塗り`, id),
    ...rest,
  ];
  return {
    v: "5.12.2",
    fr: FPS,
    ip: 0,
    op: DURATION,
    w,
    h,
    nm: "nyaw フルロゴ イントロ",
    ddd: 0,
    assets,
    // Lottie では配列の先頭が手前に描かれる。段の中では、輪郭を塗りの奥に置く
    layers: [
      ...tier("front", "文字", [partsLayer(svg, TEXT, "outline")]),
      // しっぽの塗りは耳の塗りと、しっぽの輪郭は耳の輪郭と同じ高さに置く。
      // この段は、見えてよい範囲だけを残すマスクで切り抜く（バブルは型の側で切り抜かれる）
      ...tier("back", "耳としっぽ", [...tail.fills, partsLayer(svg, EARS, "outline"), tail.outline]).map((l) =>
        l.tt ? l : { ...l, hasMask: true, masksProperties: reveal },
      ),
    ],
  };
}

for (const theme of THEMES) {
  const out = join(HERE, `logo-full-${theme.toLowerCase()}.json`);
  writeFileSync(out, JSON.stringify(build(theme)));
  console.log(`wrote ${out}`);
}
