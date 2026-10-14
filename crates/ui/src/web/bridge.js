// Web UI 層がすべてのパネルの WebView に注入するブリッジ（ページより先に読み込まれる）。
//
// 1. カーネルとの対話: `window.nyaw` を公開する。操作は Tauri のコマンド、状態の変化は Tauri のイベントで受ける。
//    - nyaw.command(cmd)  … "detach" / "close" / { attach: "hmd" | "left" | "right" | { tracker: id } } /
//                            { lock: bool } / { resize: { width, height } } / { pointer_mode: "mouse" | "touch" }
//    - nyaw.status()      … 現在の状態（Promise）。nyaw.onStatus(cb) で変化を受ける（戻り値で購読をやめる）。
//    - nyaw.spawn(page)   … 隣にページのパネルを開く。
// 2. レーザーのポインタ: Tauri のイベント `nyaw:pointer`（イベントの束）を DOM の PointerEvent / MouseEvent / click / wheel として発火する。
//    ページは普通の Web ページと同じく addEventListener で書ける。setPointerCapture も使える。
//    novr（デスクトップ）では OS のマウスがそのまま届くので、このイベントは来ない。
//    方式（イベントの mode）が "touch" のときはタッチとして流す（下の touch）。押したまま動かすとスワイプになる。
//
// ponytail: 合成イベントでは CSS の :hover / :active が効かない。代わりにレーザーが乗っている要素（と祖先）に
// data-nyaw-hover、押している間は data-nyaw-active を付ける。`:is(:hover, [data-nyaw-hover])` のように書く。
// ネイティブ入力の注入（GdkEvent / WebView2 の SendMouseInput）に置き換えれば要らなくなる。
// ponytail: HTML5 のドラッグ＆ドロップ（draggable / dragstart）とテキスト選択は合成イベントでは起きない。
// ポインタイベントで書いたドラッグは動く。
(() => {
  if (window.nyaw) return;

  const internals = () => window.__TAURI_INTERNALS__;
  const invoke = (cmd, args) => internals().invoke(cmd, args);
  // このパネル宛て（emit_to）だけを受ける。event.listen は既定で全宛先を受けるので、ほかのパネルのレーザーまで届いてしまう。
  const listen = (event, cb) =>
    window.__TAURI__.webviewWindow.getCurrentWebviewWindow().listen(event, (e) => cb(e.payload));

  /** wheel の 1 行あたりの px（スティックの倒し量は行単位で来る）。 */
  const LINE = 40;
  /** 手ごとの pointerId。ネイティブのマウス（1）と重ならないようにする。 */
  const POINTER_ID = { right: 1001, left: 1002 };

  /** 手ごとの状態。 */
  const hands = new Map();
  /** pointerId → キャプチャしている要素。 */
  const captures = new Map();

  const hand = (name) => {
    let s = hands.get(name);
    if (!s) {
      s = {
        name,
        id: POINTER_ID[name] ?? 1003,
        x: 0,
        y: 0,
        over: null,
        down: null,
        pressed: false,
        mouseSuppressed: false,
        touch: null,
        fling: null,
      };
      hands.set(name, s);
    }
    return s;
  };
  const synthetic = (id) => [...hands.values()].some((s) => s.id === id);

  // setPointerCapture は合成の pointerId を知らないので例外になる。合成の分だけ自前で持つ。
  const native = {
    set: Element.prototype.setPointerCapture,
    release: Element.prototype.releasePointerCapture,
    has: Element.prototype.hasPointerCapture,
  };
  Element.prototype.setPointerCapture = function (id) {
    if (!synthetic(id)) return native.set.call(this, id);
    const prev = captures.get(id);
    if (prev === this) return;
    const s = [...hands.values()].find((h) => h.id === id);
    if (prev) fire(prev, "lostpointercapture", s, { bubbles: true, cancelable: false });
    captures.set(id, this);
    fire(this, "gotpointercapture", s, { bubbles: true, cancelable: false });
  };
  Element.prototype.releasePointerCapture = function (id) {
    if (!synthetic(id)) return native.release.call(this, id);
    if (captures.get(id) === this) release([...hands.values()].find((h) => h.id === id));
  };
  Element.prototype.hasPointerCapture = function (id) {
    if (!synthetic(id)) return native.has.call(this, id);
    return captures.get(id) === this;
  };

  const release = (s) => {
    const el = captures.get(s.id);
    if (!el) return;
    captures.delete(s.id);
    fire(el, "lostpointercapture", s, { bubbles: true, cancelable: false });
  };

  /** 型ごとのコンストラクタ。PointerEvent が無ければ MouseEvent で代用する。 */
  const ctor = (type) => {
    if (type === "wheel") return WheelEvent;
    if (type.startsWith("pointer") || type.endsWith("pointercapture"))
      return window.PointerEvent ?? MouseEvent;
    return MouseEvent;
  };

  /** 要素へイベントを発火する。既定動作を止められなければ true。 */
  const fire = (el, type, s, extra = {}) => {
    if (!el) return true;
    const init = {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      clientX: s.x,
      clientY: s.y,
      screenX: s.x + window.screenX,
      screenY: s.y + window.screenY,
      button: 0,
      buttons: s.pressed ? 1 : 0,
      pointerId: s.id,
      pointerType: s.touch ? "touch" : "mouse",
      isPrimary: true,
      width: 1,
      height: 1,
      pressure: s.pressed ? 0.5 : 0,
      ...extra,
    };
    const Ctor = ctor(type);
    return el.dispatchEvent(new Ctor(type, init));
  };

  /** 要素と、その祖先（html まで）。 */
  const path = (el) => {
    const out = [];
    for (let e = el; e; e = e.parentElement) out.push(e);
    return out;
  };

  /** 要素ごとに、乗っている／押している手の集合。属性はどれかの手があれば付ける。 */
  const marks = { "data-nyaw-hover": new WeakMap(), "data-nyaw-active": new WeakMap() };
  const mark = (attr, els, s, on) => {
    for (const el of els) {
      const set = marks[attr].get(el) ?? new Set();
      if (on) set.add(s.name);
      else set.delete(s.name);
      marks[attr].set(el, set);
      if (set.size) el.setAttribute(attr, "");
      else el.removeAttribute(attr);
    }
  };

  const at = (s) => document.elementFromPoint(s.x, s.y) ?? document.documentElement;
  /** ポインタイベントの宛先。キャプチャ中はその要素。 */
  const target = (s) => captures.get(s.id) ?? at(s);

  /** 乗っている要素を `el` に変え、境界イベント（over / out / enter / leave）を出す。`quiet` なら印だけ付け替える。 */
  const hover = (s, el, quiet = false) => {
    const old = s.over;
    if (old === el) return;
    s.over = el;
    const oldPath = old && old.isConnected ? path(old) : [];
    const newPath = el ? path(el) : [];
    const left = oldPath.filter((e) => !newPath.includes(e));
    const entered = newPath.filter((e) => !oldPath.includes(e)).toReversed();
    if (quiet) {
      mark("data-nyaw-hover", left, s, false);
      mark("data-nyaw-hover", entered, s, true);
      return;
    }
    const boundary = { cancelable: false };
    const leaf = { bubbles: false, cancelable: false };
    if (oldPath.length) {
      fire(old, "pointerout", s, { ...boundary, relatedTarget: el });
      for (const e of left) fire(e, "pointerleave", s, { ...leaf, relatedTarget: el });
      if (!s.mouseSuppressed) {
        fire(old, "mouseout", s, { relatedTarget: el });
        for (const e of left) fire(e, "mouseleave", s, { ...leaf, relatedTarget: el });
      }
    }
    mark("data-nyaw-hover", left, s, false);
    mark("data-nyaw-hover", entered, s, true);
    if (el) {
      fire(el, "pointerover", s, { ...boundary, relatedTarget: old });
      for (const e of entered) fire(e, "pointerenter", s, { ...leaf, relatedTarget: old });
      if (!s.mouseSuppressed) {
        fire(el, "mouseover", s, { relatedTarget: old });
        for (const e of entered) fire(e, "mouseenter", s, { ...leaf, relatedTarget: old });
      }
    }
  };

  /** 押した要素に近い、フォーカスできる要素。 */
  const FOCUSABLE =
    'a[href], button, input, select, textarea, summary, iframe, [tabindex], [contenteditable=""], [contenteditable="true"]';
  const focusFrom = (el) => {
    const f = el.closest?.(FOCUSABLE);
    if (f && !f.disabled) f.focus({ preventScroll: true });
    else if (document.activeElement && document.activeElement !== document.body)
      document.activeElement.blur();
  };

  /** 縦横にスクロールできる一番近い祖先（無ければ文書）。 */
  const scroller = (el, dx, dy) => {
    for (
      let e = el;
      e && e !== document.documentElement && e !== document.body;
      e = e.parentElement
    ) {
      const st = getComputedStyle(e);
      const canY = dy && /(auto|scroll)/.test(st.overflowY) && e.scrollHeight > e.clientHeight;
      const canX = dx && /(auto|scroll)/.test(st.overflowX) && e.scrollWidth > e.clientWidth;
      if (canY || canX) return e;
    }
    return document.scrollingElement ?? document.documentElement;
  };

  const common = (a, b) => {
    const pa = path(a);
    return path(b).find((e) => pa.includes(e)) ?? null;
  };

  /** これより動いたらタップではなくスワイプ（CSS px。UI 層の SWIPE_SLOP と同じ）。 */
  const SLOP = 12;
  /** 要素とその祖先のどれも touch-action: none でなければ、指で中身を動かせる。 */
  const pannable = (el) => path(el).every((e) => getComputedStyle(e).touchAction !== "none");

  /**
   * タッチとして流す（ブラウザのタッチと同じ順序）。押すと pointerdown（pointerType "touch"）、
   * しきい値を超えて動かすと pointercancel してスクロール（スワイプ。touch-action: none の上では pointermove のまま）、
   * 動かさずに離すと pointerup と互換のマウスイベントと click（タップ）。指が触れていない間はホバーのイベントを出さず、印だけ付ける。
   */
  // ponytail: TouchEvent（touchstart 等）は出さない。要るページが出てきたら足す。
  /** 慣性の 1 フレーム（約 90Hz）あたりの減衰と、止める速さ（CSS px / フレーム）。UI 層の FRICTION / MOMENTUM_STOP と同じ。 */
  const FRICTION = 0.95;
  const MOMENTUM_STOP = 0.5;
  const FRAME_MS = 1000 / 90;
  /** スワイプを離した後、`el` を速度 (vx, vy)（中身が指に付いてくる向き、px / フレーム）で滑らせる。触れたら止める。 */
  // 非表示の WebView では requestAnimationFrame が止まることがあるので、タイマーで経過時間から減衰させる。
  const fling = (s, el, vx, vy) => {
    let last = performance.now();
    const step = () => {
      const now = performance.now();
      const k = (now - last) / FRAME_MS;
      last = now;
      const f = Math.pow(FRICTION, k);
      [vx, vy] = [vx * f, vy * f];
      if (Math.hypot(vx, vy) < MOMENTUM_STOP) return (s.fling = null);
      el.scrollBy({ left: -vx * k, top: -vy * k, behavior: "instant" });
      s.fling = setTimeout(step, FRAME_MS);
    };
    s.fling = setTimeout(step, FRAME_MS);
  };
  const touch = (p, s) => {
    const t = s.touch;
    switch (p.type) {
      case "move": {
        if (!t) return hover(s, at(s), true);
        const [dx, dy] = t.pan ? [s.x - t.lx, s.y - t.ly] : [s.x - t.x0, s.y - t.y0];
        [t.lx, t.ly] = [s.x, s.y];
        if (!t.pan && s.down && Math.hypot(dx, dy) > SLOP && pannable(s.down)) {
          // パンが始まったらポインタは取り消す。
          fire(captures.get(s.id) ?? s.down, "pointercancel", s, { cancelable: false });
          mark("data-nyaw-active", path(s.down), s, false);
          release(s);
          hover(s, null);
          t.pan = scroller(s.down, dx, dy);
        }
        // 中身が指に付いてくる向き（下へ動かすと上へスクロール）。
        if (t.pan) {
          t.pan.scrollBy({ left: -dx, top: -dy, behavior: "instant" });
          // 慣性の初速（直近を重く見て均す）。
          [t.vx, t.vy] = [t.vx * 0.5 + dx * 0.5, t.vy * 0.5 + dy * 0.5];
        } else fire(captures.get(s.id) ?? s.down, "pointermove", s);
        break;
      }
      case "down": {
        s.pressed = true;
        s.mouseSuppressed = true; // 互換のマウスイベントはタップと決まってから出す
        // 触れたら慣性は止まる。
        clearTimeout(s.fling);
        s.fling = null;
        s.touch = { x0: s.x, y0: s.y, lx: s.x, ly: s.y, pan: null, vx: 0, vy: 0, compat: true };
        const el = at(s);
        hover(s, el);
        s.down = el;
        mark("data-nyaw-active", path(el), s, true);
        s.touch.compat = fire(el, "pointerdown", s);
        break;
      }
      case "up": {
        if (!t) return;
        s.pressed = false;
        if (!t.pan) {
          fire(captures.get(s.id) ?? s.down, "pointerup", s);
          if (s.down) mark("data-nyaw-active", path(s.down), s, false);
          const el = at(s);
          // pointerdown を止めたら互換のマウスイベントは出さない。click は出す（Pointer Events の仕様）。
          if (t.compat) {
            if (fire(el, "mousedown", s, { buttons: 1 })) focusFrom(el);
            fire(el, "mouseup", s);
          }
          const c = s.down && s.down.isConnected ? common(s.down, el) : null;
          if (c) fire(c, "click", s, { detail: 1 });
          release(s);
          hover(s, null); // 指が離れると乗っていない
        } else {
          fling(s, t.pan, t.vx, t.vy);
        }
        s.down = null;
        s.touch = null;
        s.mouseSuppressed = false;
        hover(s, at(s), true);
        break;
      }
      case "leave": {
        if (t && !t.pan) {
          fire(captures.get(s.id) ?? s.down ?? at(s), "pointercancel", s, { cancelable: false });
          if (s.down) mark("data-nyaw-active", path(s.down), s, false);
          release(s);
        }
        hover(s, null, !t || !!t.pan);
        s.pressed = false;
        s.down = null;
        s.touch = null;
        s.mouseSuppressed = false;
        break;
      }
    }
  };

  /** カーネル（オーバーレイ層）からのポインタイベント 1 件を DOM に流す。 */
  const pointer = (p) => {
    const s = hand(p.hand);
    if (p.type !== "leave") [s.x, s.y] = [p.x, p.y];
    // 方式は押した瞬間に決め、離すまで変えない。スティックのスクロールはどちらでも同じ。
    if (p.type !== "scroll" && (s.touch || (p.mode === "touch" && !s.pressed))) return touch(p, s);
    switch (p.type) {
      case "move": {
        const t = target(s);
        hover(s, t);
        fire(t, "pointermove", s);
        if (!s.mouseSuppressed) fire(t, "mousemove", s);
        break;
      }
      case "down": {
        s.pressed = true;
        const t = target(s);
        hover(s, t);
        s.down = t;
        mark("data-nyaw-active", path(t), s, true);
        // pointerdown を止めたら、上げるまで互換のマウスイベントを出さない（Pointer Events の仕様）。
        s.mouseSuppressed = !fire(t, "pointerdown", s);
        if (!s.mouseSuppressed && fire(t, "mousedown", s)) focusFrom(t);
        break;
      }
      case "up": {
        s.pressed = false;
        const t = target(s);
        fire(t, "pointerup", s);
        if (!s.mouseSuppressed) fire(t, "mouseup", s);
        const c = s.down && s.down.isConnected ? common(s.down, t) : null;
        if (s.down) mark("data-nyaw-active", path(s.down), s, false);
        s.down = null;
        s.mouseSuppressed = false;
        if (c) fire(c, "click", s, { detail: 1 });
        release(s);
        hover(s, at(s));
        break;
      }
      case "scroll": {
        const t = target(s);
        hover(s, t);
        // スティックを右／上に倒すと右／上へ進む。wheel の deltaY は下が正。
        const dx = p.dx * LINE;
        const dy = -p.dy * LINE;
        if (fire(t, "wheel", s, { deltaX: dx, deltaY: dy, deltaMode: 0 })) {
          scroller(t, dx, dy).scrollBy({ left: dx, top: dy, behavior: "instant" });
        }
        break;
      }
      case "leave": {
        if (s.pressed) {
          s.pressed = false;
          fire(target(s), "pointercancel", s, { cancelable: false });
          if (s.down) mark("data-nyaw-active", path(s.down), s, false);
          s.down = null;
          s.mouseSuppressed = false;
          release(s);
        }
        hover(s, null);
        break;
      }
    }
  };

  const statusListeners = [];
  window.nyaw = {
    command: (command) => invoke("panel_command", { command }),
    status: () => invoke("panel_status"),
    spawn: (page) => invoke("spawn_panel", { page }),
    onStatus: (cb) => {
      statusListeners.push(cb);
      return () => {
        const i = statusListeners.indexOf(cb);
        if (i >= 0) statusListeners.splice(i, 1);
      };
    },
  };

  // Tauri の API（withGlobalTauri）はこのスクリプトの後に用意されることがあるので、読み込み後に購読する。
  const subscribe = () => {
    // 束を流し終えたら返事をする。返事が届くまで次の束は来ない（その間 Move は最新だけ・Scroll は合算される）。
    listen("nyaw:pointer", (batch) => {
      try {
        for (const p of batch) pointer(p);
      } finally {
        invoke("pointer_ack");
      }
    });
    listen("nyaw:status", (st) => statusListeners.forEach((cb) => cb(st)));
  };
  if (document.readyState === "loading")
    document.addEventListener("DOMContentLoaded", subscribe, { once: true });
  else subscribe();
})();
