// フルロゴのイントロアニメーションを Lottie の JSON として生成する。
// 形と色は assets/logos の SVG から読むので、ロゴを描き直したら、このスクリプトを実行し直せば追従する。
// 実行: node assets/animations/logo-full/build.mjs
//
// 不透明度は一度も動かさない。出現は拡大縮小で、しっぽは線の長さ（トリム）とパスの変形で表す。

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const LOGOS = join(HERE, "../../logos");

const FPS = 60;
// 合成の四辺に足す余白。出現の行き過ぎ、耳の回転、しっぽのうねりで、絵がロゴの外形より外へ出るため。
// プレイヤーは合成の範囲の外を描かないので、余白がないとその部分が欠ける
const MARGIN = 24;
const DURATION = 180; // フレーム数（3 秒）

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

// ---- SVG の読み取り ----

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
  let outlinePath, fillPath, outline, fill;
  const rects = [];
  const circles = [];
  for (const [, tag, attrs] of body.matchAll(/<(path|rect|circle)\b([^>]*)\/>/g)) {
    const attr = (name) => attrs.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];
    const cls = classes[attr("class")];
    if (tag === "path" && cls.stroke) [outlinePath, outline] = [attr("d"), cls.stroke];
    else if (tag === "path") [fillPath, fill] = [attr("d"), cls.fill];
    else if (tag === "rect") rects.push(["x", "y", "width", "height"].map((k) => +attr(k)));
    else circles.push({ c: [+attr("cx"), +attr("cy")], r: +attr("r"), color: cls.fill });
  }
  if (outlinePath !== fillPath) throw new Error(`${file}: 輪郭と塗りのパスが違う`);
  const width = +classes[Object.keys(classes).find((k) => classes[k].stroke)]["stroke-width"].replace("px", "");
  const viewBox = src.match(/viewBox="([^"]*)"/)[1].split(" ").map(Number);
  // 余白の分だけ、すべての座標をずらす
  const m = (p) => [p[0] + MARGIN, p[1] + MARGIN];
  return {
    viewBox: [0, 0, viewBox[2] + 2 * MARGIN, viewBox[3] + 2 * MARGIN],
    contours: parsePath(fillPath).map((segs) => segs.map((seg) => seg.map(m))),
    rects: rects.map(([x, y, w, h]) => [x + MARGIN, y + MARGIN, w, h]),
    circles: circles.map((c) => ({ ...c, c: m(c.c) })),
    outline,
    fill,
    width,
  };
}

// SVG のパスを、輪郭ごとの 3 次ベジェの並び [p0, c1, c2, p1] にする。
// このロゴが使う M, c, s, l, h, v, Z だけを扱う。
function parsePath(d) {
  const tokens = d.match(/[a-zA-Z]|-?(?:\d+\.?\d*|\.\d+)/g);
  const contours = [];
  let i = 0;
  let cmd;
  let cur = [0, 0];
  let start;
  let lastCtrl;
  let segs;
  const num = () => +tokens[i++];
  const add = (a, b) => [a[0] + b[0], a[1] + b[1]];
  while (i < tokens.length) {
    if (/[a-zA-Z]/.test(tokens[i])) cmd = tokens[i++];
    if (cmd === "M") {
      cur = [num(), num()];
      start = cur;
      segs = [];
      contours.push(segs);
      cmd = "L";
      continue;
    }
    if (cmd === "Z" || cmd === "z") {
      if (Math.hypot(cur[0] - start[0], cur[1] - start[1]) > 0.01) segs.push([cur, cur, start, start]);
      cur = start;
      continue;
    }
    let seg;
    if (cmd === "c") {
      const c1 = add(cur, [num(), num()]);
      const c2 = add(cur, [num(), num()]);
      seg = [cur, c1, c2, add(cur, [num(), num()])];
    } else if (cmd === "s") {
      const c1 = lastCtrl ? [2 * cur[0] - lastCtrl[0], 2 * cur[1] - lastCtrl[1]] : cur;
      const c2 = add(cur, [num(), num()]);
      seg = [cur, c1, c2, add(cur, [num(), num()])];
    } else {
      const to =
        cmd === "l" ? add(cur, [num(), num()]) : cmd === "L" ? [num(), num()] : cmd === "h" ? add(cur, [num(), 0]) : add(cur, [0, num()]);
      seg = [cur, cur, to, to];
    }
    lastCtrl = cmd === "c" || cmd === "s" ? seg[2] : undefined;
    cur = seg[3];
    // 長さのない線（"h0" など）は頂点を重ねるだけなので捨てる
    if (Math.hypot(seg[3][0] - seg[0][0], seg[3][1] - seg[0][1]) > 1e-6 || cmd === "c" || cmd === "s") segs.push(seg);
  }
  return contours;
}

// ---- 幾何 ----

const r2 = (v) => Math.round(v * 100) / 100;
const sub = (a, b) => [a[0] - b[0], a[1] - b[1]];

// ベジェの並び（閉じた輪郭）を Lottie の形 { c, v, i, o } にする
function toShape(segs) {
  const v = segs.map((s) => s[0]);
  const o = segs.map((s) => sub(s[1], s[0]));
  const i = segs.map((_, k) => {
    const prev = segs[(k - 1 + segs.length) % segs.length];
    return sub(prev[2], prev[3]);
  });
  const round = (ps) => ps.map((p) => p.map(r2));
  return { c: true, v: round(v), i: round(i), o: round(o) };
}

function bezier([p0, c1, c2, p1], t) {
  const u = 1 - t;
  return [0, 1].map((k) => u * u * u * p0[k] + 3 * u * u * t * c1[k] + 3 * u * t * t * c2[k] + t * t * t * p1[k]);
}

function bbox(contours) {
  const pts = contours.flat().flatMap((seg) => Array.from({ length: 9 }, (_, k) => bezier(seg, k / 8)));
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

// ベジェの並びを、弧長で等間隔に n 点取る
function resample(segs, n) {
  const poly = segs.flatMap((seg, k) => Array.from({ length: k === 0 ? 41 : 40 }, (_, j) => bezier(seg, (k === 0 ? j : j + 1) / 40)));
  const acc = [0];
  for (let k = 1; k < poly.length; k++) acc.push(acc[k - 1] + Math.hypot(...sub(poly[k], poly[k - 1])));
  const total = acc.at(-1);
  return Array.from({ length: n }, (_, j) => {
    const target = (total * j) / (n - 1);
    let k = acc.findIndex((a) => a >= target);
    if (k <= 0) return poly[Math.max(k, 0)];
    const t = (target - acc[k - 1]) / (acc[k] - acc[k - 1]);
    return [0, 1].map((m) => poly[k - 1][m] + t * (poly[k][m] - poly[k - 1][m]));
  });
}

// しっぽは、幅の一定な線を丸い端で太らせた輪郭になっている（下の縁 → 先端の丸み → 上の縁 → 根元の丸み）。
// 下の縁の各点と、上の縁で一番近い点との中点を芯線の点とし、それを少ない頂点のベジェ曲線に当てはめる。
// 頂点は根元と先端の 2 つだけで、1 本の 3 次ベジェにする。幅は、対にした 2 点の距離の平均とする。
function tailSpine(segs) {
  const lower = segs.slice(0, 3);
  const upper = resample(segs.slice(4, 8), 400);
  const lo = resample(lower, 120);
  const nearest = (p) => upper.reduce((best, q) => (Math.hypot(...sub(p, q)) < Math.hypot(...sub(p, best)) ? q : best));
  const pairs = lo.map((p) => [p, nearest(p)]);
  const mids = pairs.map(([p, q]) => [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2]);
  const width = pairs.reduce((s, [p, q]) => s + Math.hypot(...sub(p, q)), 0) / pairs.length;
  return { spine: fitSpine(mids, [0, mids.length - 1]), width };
}

// 点列 pts を、knots の位置で区切った 3 次ベジェの並びに当てはめる（Schneider の方法）。
// 区切りでの接線は前後の点から決め、ハンドルの長さだけを最小二乗で求めるので、つなぎ目は滑らかになる
function fitSpine(pts, knots) {
  const unit = (d) => {
    const l = Math.hypot(...d);
    return [d[0] / l, d[1] / l];
  };
  const tangent = (k) => unit(sub(pts[Math.min(k + 2, pts.length - 1)], pts[Math.max(k - 2, 0)]));
  const handles = knots.map(() => ({ i: [0, 0], o: [0, 0] }));
  for (let s = 0; s < knots.length - 1; s++) {
    const span = pts.slice(knots[s], knots[s + 1] + 1);
    const [p0, p3] = [span[0], span.at(-1)];
    const [t0, t3] = [tangent(knots[s]), tangent(knots[s + 1]).map((v) => -v)];
    const acc = [0];
    for (let k = 1; k < span.length; k++) acc.push(acc[k - 1] + Math.hypot(...sub(span[k], span[k - 1])));
    let [c00, c01, c11, x0, x1] = [0, 0, 0, 0, 0];
    span.forEach((q, k) => {
      const u = acc[k] / acc.at(-1);
      const [b0, b1, b2, b3] = [(1 - u) ** 3, 3 * u * (1 - u) ** 2, 3 * u * u * (1 - u), u ** 3];
      const a1 = [t0[0] * b1, t0[1] * b1];
      const a2 = [t3[0] * b2, t3[1] * b2];
      const r = [0, 1].map((m) => q[m] - (b0 + b1) * p0[m] - (b2 + b3) * p3[m]);
      c00 += a1[0] * a1[0] + a1[1] * a1[1];
      c01 += a1[0] * a2[0] + a1[1] * a2[1];
      c11 += a2[0] * a2[0] + a2[1] * a2[1];
      x0 += a1[0] * r[0] + a1[1] * r[1];
      x1 += a2[0] * r[0] + a2[1] * r[1];
    });
    const det = c00 * c11 - c01 * c01;
    const al = (x0 * c11 - x1 * c01) / det;
    const be = (c00 * x1 - c01 * x0) / det;
    handles[s].o = [t0[0] * al, t0[1] * al];
    handles[s + 1].i = [t3[0] * be, t3[1] * be];
  }
  return { v: knots.map((k) => pts[k]), ...handlesOf(handles) };
}
const handlesOf = (hs) => ({ i: hs.map((h) => h.i), o: hs.map((h) => h.o) });

// CSS の easeInOutCirc
const circInOut = (u) => (u < 0.5 ? (1 - Math.sqrt(1 - (2 * u) ** 2)) / 2 : (Math.sqrt(1 - (2 - 2 * u) ** 2) + 1) / 2);

// from から dir の向きへ進み、輪郭の内側で、輪郭から depth 以上離れた最初の点を返す
function deepPointOnRay(contours, depth, from, dir) {
  const polys = contours.map((segs) => segs.flatMap((seg) => Array.from({ length: 12 }, (_, k) => bezier(seg, k / 12))));
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
          const b = poly[(k + 1) % poly.length];
          const d = sub(b, a);
          const u = Math.max(0, Math.min(1, ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] ** 2 + d[1] ** 2)));
          return Math.hypot(p[0] - a[0] - u * d[0], p[1] - a[1] - u * d[1]);
        }),
      ),
    );
  const l = Math.hypot(...dir);
  for (let d = 0; d < 200; d += 0.5) {
    const p = [from[0] + (dir[0] / l) * d, from[1] + (dir[1] / l) * d];
    if (inside(p) && edge(p) >= depth) return p.map(r2);
  }
  throw new Error("この向きには、しっぽを隠せる深さのある点が w の中にない");
}

// しっぽ全体の回転（度）を 2 フレームごとに求める。先端を下げた向き（SWING.from）から、ばねで完成形の向き（0）へ戻す。
// release まで（しっぽが w から出てくるまで）は from のまま止めておき、そこで放す。
// 止まった状態から放すので、ゆっくり動き出し、勢いのまま完成形を越えて先端が持ち上がり、揺れ返しながら収まる。
// 揺れに強弱を付けるため、ばねの速さを時間とともに落とす。最初の振りは速く鋭く（fast Hz）、そのあとの揺れ返しは
// ゆっくり穏やかになる（slow Hz へ、時定数 relax 秒で近づく）。damping は減衰比、until で残りの揺れをちょうど 0 にして止める
const SWING = { from: 35, release: 62, fast: 1.8, slow: 0.8, relax: 0.35, damping: 0.42, until: 160 };
function tailSwing() {
  const STEPS = 8;
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

// ---- Lottie の部品 ----

const hex = (c) => [1, 3, 5].map((k) => r2(parseInt(c.slice(k, k + 2), 16) / 255)).concat(1);
const still = (k) => ({ a: 0, k });
const EASE = { o: { x: [0.33], y: [0] }, i: { x: [0.67], y: [1] } };
const LINEAR = { o: { x: [0], y: [0] }, i: { x: [1], y: [1] } };

// [フレーム, 値] の並びを、間をなめらかにつなぐキーフレームにする。値は数か配列
function keys(list, ease = EASE) {
  const wrap = (v) => (Array.isArray(v) ? v : [v]);
  return {
    a: 1,
    k: list.map(([t, v], k) => (k === list.length - 1 ? { t, s: wrap(v) } : { t, s: wrap(v), ...ease })),
  };
}

function transform({ anchor = [0, 0], position = anchor, scale = still([100, 100]), rotation = still(0) } = {}) {
  return { a: still(anchor), p: still(position), s: scale, r: rotation, o: still(100) };
}

const group = (name, items) => ({ ty: "gr", nm: name, it: [...items, { ty: "tr", ...transform() }] });
const pathItem = (shape) => ({ ty: "sh", ks: shape.a === 1 ? shape : still(shape) });
const fillItem = (color) => ({ ty: "fl", c: still(hex(color)), o: still(100), r: 1 });
const strokeItem = (color, w) => ({ ty: "st", c: still(hex(color)), o: still(100), w: still(w), lc: 2, lj: 2 });

let nextInd = 1;
function layer(nm, props) {
  return { ddd: 0, ind: nextInd++, ty: 4, nm, sr: 1, ks: transform(), ao: 0, ip: 0, op: DURATION, st: 0, bm: 0, ...props };
}

// ---- 動き ----

// ステッカーが貼られるように、0 から行き過ぎて戻る。横と縦をずらして、弾む感じを出す
function pop(t0, { tilt = 8, big = 1 } = {}) {
  return {
    scale: keys([
      [t0, [0, 0]],
      [t0 + 9, [118 * big, 90]],
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

// 耳は、文字の奥から回りながらせり上がって出てきて、そのあと付け根を中心にぴくっと動く。
// 出始めは付け根より下（文字の裏）に置き、小さな大きさから始めるので、現れる瞬間は文字に隠れて見えない
function ear(t0, dir, twitches) {
  const rot = [
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
    rot.push([t, 0], [t + 4, -16 * dir], [t + 9, 6 * dir], [t + 15, 0]);
    scale.push([t, [100, 100]], [t + 4, [104, 90]], [t + 9, [98, 103]], [t + 15, [100, 100]]);
  }
  // 出番までは大きさ 0 のまま止めておく（h: 1 は次のキーフレームまで値を変えない）
  const hidden = { t: 0, s: [0, 0], h: 1 };
  return { rotation: keys(rot), scale: { a: 1, k: [hidden, ...keys(scale).k] }, offset };
}

const TIMING = {
  n: pop(4),
  y: pop(9, { tilt: -8 }),
  a: pop(14),
  w: pop(19, { tilt: -8 }),
  ni: pop(32, { big: 0.95 }),
  ya: pop(42, { tilt: -10, big: 0.95 }),
  u: pop(52, { big: 0.95 }),
  // 耳は、下にある文字（左の耳は n と y、右の耳は a と w）の出現の拡大が収まってから出す。小さいうちは裏に隠しきれない
  earL: ear(38, 1, [88, 106]),
  earR: ear(46, -1, [120]),
};
// start で w の裏から滑り出し始め、slide フレームかけて元の位置に着く。伸びるのも start からで、grown で伸びきる。
// start は、w の出現の拡大が収まったあと（w が小さいうちは、裏に隠したしっぽがはみ出して見える）。
const TAIL = { start: 46, slide: 30, grown: 92 };

// バブル（ロゴの地の円）は、ずれた位置から回り込むように漂って、元の位置に収まる。
// 文字としっぽの形でまとめて切り抜くので、形の外には出ない
const BUBBLES = { settle: 130, drift: 26, turns: 0.6 };

// すべてが出そろったあと、幅のある明るい帯が斜めに左から右へ一瞬で横切る。バブルと同じく形で切り抜く。
// 動きは circ の in-out（出だしと終わりがゆっくりで、中ほどを一気に抜ける）。
// 帯の色はテーマごとに、塗りより明るい 2 色（太い帯、その後ろを追う細い帯）。
// 時間は、帯がロゴの上を通る部分がリファレンスと同じ約 0.4 秒になる長さ。circ の in-out は中ほどが速いので、全体はその数倍になる
const SWEEP = { from: 64, to: 174, angle: 28, colors: { Light: ["#ffffff", "#ffffff"], Dark: ["#c0c7e2", "#a9b0cf"] } };
// CSS の easeInOutCirc に相当する 3 次ベジェ
const CIRC_IN_OUT = { o: { x: [0.85], y: [0] }, i: { x: [0.15], y: [1] } };

// 部品の回転と拡大の中心。耳は付け根（輪郭の一番下の 2 頂点の中点）、ほかは外接矩形の中心
function pivot(name, contours) {
  if (name.startsWith("ear")) {
    const ends = contours[0].map((s) => s[0]).sort((p, q) => q[1] - p[1]).slice(0, 2);
    return [(ends[0][0] + ends[1][0]) / 2, (ends[0][1] + ends[1][1]) / 2];
  }
  const [x0, y0, x1, y1] = bbox(contours);
  return [(x0 + x1) / 2, (y0 + y1) / 2];
}

// 部品をまとめた 1 つのシェイプレイヤーを作る。部品はそれぞれグループにし、動きはグループのトランスフォームに持たせる。
// 塗りやストロークは、グループの外に 1 つだけ置き、全部品にまとめて掛ける。
// ストロークの幅が部品の拡大の影響を受けず、ロゴ全体の輪郭として一定の幅になる
function partsLayer(svg, names, kind) {
  const groups = names.map((name) => {
    const part = PARTS[name];
    const contours = part.paths.map((k) => svg.contours[k]);
    const anchor = pivot(name, contours).map(r2);
    const { scale, rotation, offset } = TIMING[name];
    const position = offset && keys(offset.map(([t, [dx, dy]]) => [t, [r2(anchor[0] + dx), r2(anchor[1] + dy)]]));
    const shapes = contours.map((c) => pathItem(toShape(c)));
    // 補助の矩形は輪郭の隙間を埋めるためのものなので、ストロークの側にだけ入れる
    const rects =
      kind === "fill"
        ? []
        : (part.rects ?? []).map((k) => {
            const [x, y, w, h] = svg.rects[k];
            return { ty: "rc", d: 1, p: still([r2(x + w / 2), r2(y + h / 2)]), s: still([w, h]), r: still(0) };
          });
    const tr = { ty: "tr", ...transform({ anchor, scale, rotation }), ...(position && { p: position }) };
    return { ty: "gr", nm: name, it: [...shapes, ...rects, tr] };
  });
  // 閉じた形に端の形は効かないが、大きさ 0 の形では丸い端が点として描かれてしまうので、平らな端にする
  const paint = kind === "fill" ? fillItem(svg.fill) : { ...strokeItem(svg.outline, svg.width), lc: 1 };
  return layer(`${names.join(" ")} ${kind === "fill" ? "塗り" : "輪郭"}`, { shapes: [...groups, paint] });
}

// しっぽ: 芯線を太い線で描き、トリムで伸ばす。
// 開いた線なので、他の部品とまとめた塗りには入れられず、塗りも輪郭も線の太さで描く
function tailLayers(svg, theme) {
  const { spine, width } = tailSpine(svg.contours[TAIL_PATH]);
  // 芯線の形は、伸び始めから完成形のまま変えない
  const round = (ps) => ps.map((p) => p.map(r2));
  const spinePath = pathItem({ c: false, v: round(spine.v), i: round(spine.i), o: round(spine.o) });

  // 伸び具合（0〜100）。出始めは、丸い端どうしが重なって 1 つの丸に見えるだけの、ごく短い長さにしておく（w の裏に隠れている）。
  // そこから circ の in-out（easeInOutCirc）で伸びきる
  const DOT = 0.5;
  const growFrom = TAIL.start;
  const growth = (t) => DOT + (100 - DOT) * circInOut(Math.min(Math.max((t - growFrom) / (TAIL.grown - growFrom), 0), 1));
  // 関数の値を 2 フレームごとに置いたキーフレーム
  const animated = (f) => {
    const list = [];
    for (let t = TAIL.start; t <= TAIL.grown; t += 2) list.push([t, r2(f(t))]);
    return keys(list, LINEAR);
  };
  const trim = (start, end) => ({ ty: "tm", s: start, e: end, o: still(0), m: 1 });
  const stroke = (name, color, w, extra = []) => group(name, [spinePath, ...extra, strokeItem(color, w)]);
  // 出番の前は長さ 0 で何も描かない（丸 1 つ分の長さにしておくと、w が出る前から丸が見えてしまう）
  const grownKeys = animated(growth);
  const grown = trim(still(0), { a: 1, k: [{ t: 0, s: [0], h: 1 }, ...grownKeys.k] });

  // 出始めは、しっぽを w の内側へずらしておき、伸びながら元の位置へ滑り出させる。しっぽの塗りと輪郭は w の塗りと輪郭より奥にあるので、
  // w の内側にある間は隠れ、生まれる瞬間は見えない。
  // 伸びる動きと同時に、しっぽ全体を根元を中心に回す（形は変えず、向きだけを変える）
  const root = spine.v[0].map(r2);
  // 隠し場所: 根元の制御点のハンドルの向きを根元から逆に辿り、w の塗りの内側で輪郭から十分に離れた最初の点。丸はこの向きに沿って滑り出す。
  // しっぽの塗りの丸が w の塗りに、輪郭の丸が w の輪郭までに収まれば、どちらも見えない。
  // その向きに半幅の深さの点がないときは、届く一番深い点を使い、出始めのしっぽをその深さに収まる大きさまで縮めておく
  const [hx, hy] = spine.o[0];
  const wContours = PARTS.w.paths.map((k) => svg.contours[k]);
  let depth = width / 2;
  let hidden;
  for (; depth > 1 && !hidden; depth -= 0.25) {
    try {
      hidden = deepPointOnRay(wContours, depth, root, [-hx, -hy]);
    } catch {}
  }
  const shrink = r2((100 * Math.min(depth + 0.25, width / 2)) / (width / 2) - 2);
  const slide = [];
  for (let t = TAIL.start; t <= TAIL.start + TAIL.slide; t += 2) {
    const left = 1 - circInOut((t - TAIL.start) / TAIL.slide);
    slide.push([t, [r2(root[0] + (hidden[0] - root[0]) * left), r2(root[1] + (hidden[1] - root[1]) * left)]]);
  }
  const appear = {
    ...transform({
      anchor: root,
      rotation: keys(tailSwing(), LINEAR),
    }),
    p: keys(slide, LINEAR),
    s: keys(
      slide.map(([t]) => [t, Array(2).fill(r2(shrink + (100 - shrink) * circInOut((t - TAIL.start) / TAIL.slide)))]),
      LINEAR,
    ),
  };
  const fill = () => layer("しっぽ 塗り", { ks: appear, shapes: [stroke("しっぽ", svg.fill, width, [grown])] });
  return {
    // バブルと帯は、しっぽの塗りを型にして切り抜く。型はしっぽと同じ線なので、伸びや回転に合わせて切り抜く形も変わる
    front: [
      { ...fill(), nm: "しっぽ バブルの型", td: 1 },
      layer("しっぽ バブル", { tt: 1, shapes: [...sweepGroups(theme), ...bubbleGroups(svg.circles)] }),
      fill(),
    ],
    outline: layer("しっぽ 輪郭", { ks: appear, shapes: [stroke("しっぽ", svg.outline, width + svg.width, [grown])] }),
  };
}

// 文字の段と、その奥の耳としっぽの段に分けて重ねる。段ごとに、塗り、輪郭、バブルを持つ。
// 段を分けるのは、耳としっぽの輪郭と塗りを文字の輪郭より奥に置き、文字の下から出てくるようにするため
const TEXT = Object.keys(PARTS).filter((n) => !n.startsWith("ear"));
const EARS = ["earL", "earR"];

function build(theme) {
  const svg = parseSvg(`Logo_Full_${theme}.svg`);
  const [, , vw, vh] = svg.viewBox;
  const [w, h] = [Math.ceil(vw), Math.ceil(vh)];
  const assets = [
    { id: "front", nm: "文字の塗り", layers: [partsLayer(svg, TEXT, "fill")] },
    { id: "back", nm: "耳の塗り", layers: [partsLayer(svg, EARS, "fill")] },
  ];
  const tail = tailLayers(svg, theme);
  const precomp = (nm, refId, props = {}) => ({ ...layer(nm, props), ty: 0, refId, w, h });
  const reveal = revealMasks(svg);
  // 塗りの合成を型（アルファマット）にして、直後のバブルと帯をその段の形で切り抜く
  const tier = (id, name, outlines) => [
    precomp(`${name} バブルの型`, id, { td: 1 }),
    layer(`${name} バブル`, { tt: 1, shapes: [...sweepGroups(theme), ...bubbleGroups(svg.circles)] }),
    precomp(`${name} 塗り`, id),
    ...outlines,
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
      // しっぽは耳と同じ奥の段に置く。塗りは耳の塗りと同じ高さ、輪郭は耳の輪郭と同じ高さ
      // 耳としっぽの段は、見えてよい範囲だけを残すマスクで切り抜く（バブルは型の側で切り抜かれる）
      ...tier("back", "耳としっぽ", [...tail.front, partsLayer(svg, EARS, "outline"), tail.outline]).map((l) =>
        l.tt ? l : { ...l, hasMask: true, masksProperties: reveal },
      ),
    ],
  };
}

// 耳としっぽを、文字の裏に隠れている間に確実に見えなくするマスク。2 つの範囲を足し合わせる。
// - 耳の範囲: 文字（n、y、a、w）の上端から REVEAL.below だけ下がった線より上。出始めの耳はこの線より下にあるので、
//   文字がまだ小さいときや文字の隙間からも見えない。線より下に残る付け根は、完成時には文字に隠れる範囲
// - しっぽの範囲: w の右端から REVEAL.inset だけ左の線より右。w の裏にある間は見えず、この線を越えたところから現れる
// マスクは層の座標で固定され、部品の動きとは一緒に動かない
const REVEAL = { below: 10, inset: 2 };
function revealMasks(svg) {
  const top = Math.min(...["n", "y", "a", "w"].map((n) => bbox(PARTS[n].paths.map((k) => svg.contours[k]))[1]));
  const right = bbox(PARTS.w.paths.map((k) => svg.contours[k]))[2];
  const FAR = 10000;
  const rect = (x0, y0, x1, y1) => ({
    c: true,
    v: [
      [x0, y0],
      [x1, y0],
      [x1, y1],
      [x0, y1],
    ].map((p) => p.map(r2)),
    i: Array(4).fill([0, 0]),
    o: Array(4).fill([0, 0]),
  });
  const mask = (nm, shape) => ({ inv: false, mode: "a", pt: still(shape), o: still(100), x: still(0), nm });
  return [
    mask("耳の範囲", rect(-FAR, -FAR, FAR, top + REVEAL.below)),
    mask("しっぽの範囲", rect(right - REVEAL.inset, -FAR, FAR, FAR)),
  ];
}

// 斜めの帯。バブルと同じ層に入れ、同じ型で切り抜く
function sweepGroups(theme) {
  const [wide, thin] = SWEEP.colors[theme];
  const band = (name, color, width, offset) => ({
    ty: "gr",
    nm: name,
    it: [
      { ty: "rc", d: 1, p: still([offset, 0]), s: still([width, 400]), r: still(0) },
      fillItem(color),
      {
        ty: "tr",
        ...transform({ rotation: still(SWEEP.angle) }),
        // 帯がロゴの左の外から右の外まで抜けきる範囲。始めと終わりで、帯（細い帯を含む）がどの部品にも掛からない
        p: keys(
          [
            [SWEEP.from, [-84 + MARGIN, 75 + MARGIN]],
            [SWEEP.to, [506 + MARGIN, 75 + MARGIN]],
          ],
          CIRC_IN_OUT,
        ),
      },
    ],
  });
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
        const u = Math.min(t / BUBBLES.settle, 1);
        const left = (1 - u) ** 2; // 残りのずれ。終わりに向けて緩やかに 0 になる
        const ang = k * 2.4 + 2 * Math.PI * BUBBLES.turns * u * (k % 2 ? 1 : -1);
        p.push([t, [r2(c[0] + BUBBLES.drift * left * Math.cos(ang)), r2(c[1] + BUBBLES.drift * left * Math.sin(ang))]]);
        size.push([t, Array(2).fill(r2(2 * r * (1 - 0.3 * left)))]);
      }
      return group(`バブル ${k}`, [{ ty: "el", d: 1, p: keys(p, LINEAR), s: keys(size, LINEAR) }, fillItem(color)]);
    })
    .reverse();
}

for (const theme of ["Light", "Dark"]) {
  const out = join(HERE, `logo-full-${theme.toLowerCase()}.json`);
  writeFileSync(out, JSON.stringify(build(theme)));
  console.log(`wrote ${out}`);
}
