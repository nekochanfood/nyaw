// Testing の Web 版。GPUI の Testing と同じく、基本的なコンポーネントの操作と VR（カーネル）の操作を 1 画面で試す。
// コンポーネントは Base UI、見た目は Tailwind。ポインタは普通の DOM イベントで受ける
// （VR ではレーザーを UI 層のブリッジが DOM イベントにし、novr では OS のマウスがそのまま届く）。
// 要素の id は E2E が位置を引くのに使う（GPUI の debug_selector に当たる）。
import { Button } from "@base-ui/react/button";
import { Checkbox } from "@base-ui/react/checkbox";
import { Input } from "@base-ui/react/input";
import { Menu } from "@base-ui/react/menu";
import { Slider } from "@base-ui/react/slider";
import { Switch } from "@base-ui/react/switch";
import { Tabs } from "@base-ui/react/tabs";
import { Toggle } from "@base-ui/react/toggle";
import {
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";
import { type Command, nyaw, useStatus } from "../nyaw";
import { useProbe } from "../probe";
import { SwipeScrollArea } from "../SwipeScrollArea";

/** E2E が数える DOM イベント。 */
const TYPES = [
  "pointerover",
  "pointerenter",
  "pointerdown",
  "pointermove",
  "pointerup",
  "pointercancel",
  "pointerout",
  "pointerleave",
  "gotpointercapture",
  "lostpointercapture",
  "mousedown",
  "mouseup",
  "click",
  "wheel",
  "keydown",
] as const;

type Last = {
  type: string;
  target: string;
  x: number;
  y: number;
  pointerType: string | null;
  buttons: number;
  trusted: boolean;
} | null;

const btn =
  "rounded px-3 py-1.5 text-sm text-ink bg-btn hover:bg-btn-hover active:bg-btn-press data-[on]:bg-accent outline-none";

const cmd = (c: Command) => () => void nyaw().command(c);
/** 有効な状態（GPUI の BUTTON_ACTIVE_BG に当たる）。 */
const on = (b: boolean) => (b ? { "data-on": "" } : {});

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-wrap content-start items-center gap-2">
      <h2 className="w-full text-xs text-ink/60">{title}</h2>
      {children}
    </section>
  );
}

export function App() {
  const status = useStatus();
  const [clicks, setClicks] = useState(0);
  const [hovered, setHovered] = useState(false);
  const [held, setHeld] = useState(false);
  const [toggled, setToggled] = useState(false);
  const [switched, setSwitched] = useState(false);
  const [checked, setChecked] = useState(false);
  const [slider, setSlider] = useState(20);
  const [menuOpen, setMenuOpen] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [tab, setTab] = useState<string>("tab-a");
  const [dropped, setDropped] = useState(false);
  const [text, setText] = useState("");
  const [resizes, setResizes] = useState(0);

  // イベントの記録は描画と関係ないので ref に持つ（毎回描き直さない）。
  const counts = useRef(Object.fromEntries(TYPES.map((t) => [t, 0])) as Record<string, number>);
  const last = useRef<{ any: Last; pointer: Last }>({ any: null, pointer: null });
  const drag = useRef({ id: -1, x: 0, y: 0, moves: 0, captured: false });
  const scrolled = useRef({ wheel: 0, top: 0, left: 0, both: [0, 0] });

  useEffect(() => {
    const record = (e: Event) => {
      counts.current[e.type]++;
      if (e.type === "pointermove") return;
      const m = e as MouseEvent & Partial<PointerEvent>;
      const target = e.target as Element;
      const entry: Last = {
        type: e.type,
        target: target.id || target.tagName?.toLowerCase() || "",
        x: m.clientX ?? 0,
        y: m.clientY ?? 0,
        pointerType: m.pointerType ?? null,
        buttons: m.buttons ?? 0,
        trusted: e.isTrusted,
      };
      last.current.any = entry;
      if (e.type.startsWith("pointer")) last.current.pointer = entry;
    };
    for (const t of TYPES) document.addEventListener(t, record, { capture: true });
    const resize = () => setResizes((n) => n + 1);
    addEventListener("resize", resize);
    return () => {
      for (const t of TYPES) document.removeEventListener(t, record, { capture: true });
      removeEventListener("resize", resize);
    };
  }, []);

  useProbe(() => ({
    clicks,
    hovered,
    held,
    toggled,
    switched,
    checked,
    slider,
    menuOpen,
    picked,
    tab,
    dropped,
    text,
    resizes,
    counts: counts.current,
    last: last.current.any,
    lastPointer: last.current.pointer,
    dragMoves: drag.current.moves,
    captured: drag.current.captured,
    wheel: scrolled.current.wheel,
    scrollTop: scrolled.current.top,
    scrollLeft: scrolled.current.left,
    scroll2d: scrolled.current.both,
    status,
  }));

  // ---- ドラッグ: pointerdown でキャプチャし、パネルの外へ出ても追い続ける ----
  const zone = useRef<HTMLDivElement>(null);
  const [offset, setOffset] = useState<[number, number] | null>(null);
  const inside = (x: number, y: number) => {
    const r = zone.current?.getBoundingClientRect();
    return !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  };
  const chipDown = (e: ReactPointerEvent) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { ...drag.current, id: e.pointerId, x: e.clientX, y: e.clientY };
    e.preventDefault();
  };
  const chipMove = (e: ReactPointerEvent) => {
    if (e.pointerId !== drag.current.id) return;
    drag.current.moves++;
    setOffset([e.clientX - drag.current.x, e.clientY - drag.current.y]);
  };
  const chipUp = (e: ReactPointerEvent) => {
    if (e.pointerId !== drag.current.id) return;
    if (inside(e.clientX, e.clientY)) setDropped(true);
    drag.current.id = -1;
    setOffset(null);
  };
  const chipCancel = () => { drag.current.id = -1; setOffset(null); };

  const s = status;
  return (
    <main className="grid h-full grid-cols-2 gap-3 overflow-hidden bg-panel p-3 text-ink">
      <div className="flex min-w-0 flex-col gap-3">
        <Section title="VR（カーネル）">
          <Button
            id="attach-hmd"
            className={btn}
            {...on(s?.attached === "hmd")}
            onClick={cmd({ attach: "hmd" })}
          >
            Attach HMD
          </Button>
          <Button
            id="attach-left"
            className={btn}
            {...on(s?.attached === "left")}
            onClick={cmd({ attach: "left" })}
          >
            Attach Left
          </Button>
          <Button
            id="attach-right"
            className={btn}
            {...on(s?.attached === "right")}
            onClick={cmd({ attach: "right" })}
          >
            Attach Right
          </Button>
          <Button id="detach" className={btn} onClick={cmd("detach")}>
            Detach
          </Button>
          <Button
            id="lock"
            className={btn}
            {...on(!!s?.locked)}
            onClick={cmd({ lock: !s?.locked })}
          >
            Lock
          </Button>
          <Button id="spawn-panel" className={btn} onClick={() => void nyaw().spawn("testing")}>
            Spawn Panel
          </Button>
          <Button
            id="open-transparent"
            className={btn}
            onClick={() => void nyaw().spawn("testing-transparent")}
          >
            Transparent
          </Button>
          <Button id="close" className={btn} onClick={cmd("close")}>
            Close
          </Button>
          <span id="menu-held" className={btn} {...on(!!s?.menu)}>
            Menu
          </span>
          <span id="grabbed" className={btn} {...on(!!s?.grabbed)}>
            Grabbed
          </span>
        </Section>
        <Section title="リサイズ">
          <Button
            id="resize-small"
            className={btn}
            onClick={cmd({ resize: { width: 640, height: 480 } })}
          >
            640×480
          </Button>
          <Button
            id="resize-large"
            className={btn}
            onClick={cmd({ resize: { width: 1000, height: 700 } })}
          >
            1000×700
          </Button>
          <span id="size" className="text-sm">
            {s
              ? `${s.runtime} / ${s.resolution.join("×")}px ${s.size.map((m) => m.toFixed(2)).join("×")}m`
              : "-"}
          </span>
        </Section>
        <Section title="入力欄（キーボードは novr のみ）">
          <Input
            id="text-field"
            placeholder="Type here"
            value={text}
            onValueChange={setText}
            className="w-full rounded border border-btn-hover bg-black/30 px-2 py-1.5 text-sm text-ink outline-none focus:outline-2 focus:outline-accent"
          />
        </Section>
        <Section title="タブ（Base UI Tabs）">
          <Tabs.Root value={tab} onValueChange={(v) => setTab(String(v))} className="w-full">
            <Tabs.List className="flex gap-1">
              {["tab-a", "tab-b"].map((t) => (
                <Tabs.Tab key={t} id={t} value={t} className={`${btn} data-[active]:bg-accent`}>
                  {t}
                </Tabs.Tab>
              ))}
            </Tabs.List>
            <Tabs.Panel value="tab-a" className="p-2 text-sm">
              A の中身
            </Tabs.Panel>
            <Tabs.Panel value="tab-b" className="p-2 text-sm">
              B の中身
            </Tabs.Panel>
          </Tabs.Root>
        </Section>
      </div>

      <div className="flex min-w-0 flex-col gap-3">
        <Section title="ボタン">
          <Button
            id="click-me"
            className={`${btn} ${clicks ? "bg-ok hover:bg-ok" : ""}`}
            onClick={() => setClicks((n) => n + 1)}
            onPointerEnter={() => setHovered(true)}
            onPointerLeave={() => setHovered(false)}
          >
            {clicks ? `Clicked ${clicks}` : "Click Me"}
          </Button>
          <Button
            id="hold-me"
            className={btn}
            onPointerDown={() => setHeld(true)}
            onPointerUp={() => setHeld(false)}
            onPointerLeave={() => setHeld(false)}
          >
            {held ? "Holding" : "Hold Me"}
          </Button>
          <Toggle
            id="toggle"
            pressed={toggled}
            onPressedChange={setToggled}
            className={`${btn} data-[pressed]:bg-accent`}
          >
            Toggle
          </Toggle>
        </Section>
        <Section title="Switch / Checkbox / Slider">
          <span id="switch" className="inline-flex">
            <Switch.Root
              checked={switched}
              onCheckedChange={setSwitched}
              className="relative h-6 w-11 rounded-full bg-btn data-[checked]:bg-accent"
            >
              <Switch.Thumb className="block h-5 w-5 translate-x-0.5 rounded-full bg-ink transition-transform data-[checked]:translate-x-[22px]" />
            </Switch.Root>
          </span>
          <span id="checkbox" className="inline-flex">
            <Checkbox.Root
              checked={checked}
              onCheckedChange={setChecked}
              className="flex h-6 w-6 items-center justify-center rounded bg-btn data-[checked]:bg-accent"
            >
              <Checkbox.Indicator className="text-sm">✓</Checkbox.Indicator>
            </Checkbox.Root>
          </span>
          <Slider.Root
            id="slider"
            value={slider}
            onValueChange={(v) => setSlider(Array.isArray(v) ? v[0] : v)}
            className="w-48"
          >
            <Slider.Control className="flex h-6 items-center">
              <Slider.Track className="h-1.5 w-full rounded bg-btn">
                <Slider.Indicator className="rounded bg-accent" />
                <Slider.Thumb className="h-4 w-4 rounded-full bg-ink">
                  {/* Base UI は Thumb の id を中の input に付けるので、見える要素に id を付ける */}
                  <span id="slider-thumb" className="block h-full w-full rounded-full" />
                </Slider.Thumb>
              </Slider.Track>
            </Slider.Control>
          </Slider.Root>
          <span className="text-sm">{slider}</span>
        </Section>
        <Section title="ポップアップ（Base UI Menu）">
          <Menu.Root open={menuOpen} onOpenChange={setMenuOpen}>
            <Menu.Trigger id="open-menu" className={btn}>
              {picked ?? "Open Menu"}
            </Menu.Trigger>
            <Menu.Portal>
              <Menu.Positioner sideOffset={4}>
                <Menu.Popup className="rounded bg-btn p-1 text-sm text-ink shadow-lg">
                  {["menu-a", "menu-b"].map((m) => (
                    <Menu.Item
                      key={m}
                      id={m}
                      onClick={() => setPicked(m)}
                      className="rounded px-3 py-1.5 outline-none data-[highlighted]:bg-accent"
                    >
                      {m}
                    </Menu.Item>
                  ))}
                </Menu.Popup>
              </Menu.Positioner>
            </Menu.Portal>
          </Menu.Root>
        </Section>
        <Section title="ドラッグ（setPointerCapture）">
          <div
            id="drag-chip"
            className="h-8 w-16 touch-none rounded-full bg-accent"
            style={offset ? { transform: `translate(${offset[0]}px, ${offset[1]}px)` } : undefined}
            onPointerDown={chipDown}
            onPointerMove={chipMove}
            onPointerUp={chipUp}
            onPointerCancel={chipCancel}
            onGotPointerCapture={() => (drag.current.captured = true)}
            onLostPointerCapture={() => { drag.current.captured = false; chipCancel(); }}
          />
          <div
            id="drop-zone"
            ref={zone}
            className={`grid h-14 w-36 place-items-center rounded-md border-2 border-dashed border-btn-hover text-sm ${dropped ? "bg-ok" : ""}`}
          >
            Drop
          </div>
        </Section>
        <Section title="一次元スクロール（スワイプ／スティック）">
            <SwipeScrollArea axis="y" role="region" aria-label="一次元スクロール"
              id="scroll-list"
              onWheel={(e) => (scrolled.current.wheel += e.deltaY)}
              onScroll={(e) => { scrolled.current.top = e.currentTarget.scrollTop; scrolled.current.left = e.currentTarget.scrollLeft; }}
            >
              {Array.from({ length: 30 }, (_, i) => (
                <div key={i} className="border-b border-white/10 px-2 py-1 text-sm">
                  Row {i}
                </div>
              ))}
            </SwipeScrollArea>
        </Section>
        <Section title="二次元スクロール（スワイプ／スティック）">
          <SwipeScrollArea id="scroll-grid" axis="both" role="region" aria-label="二次元スクロール"
            onScroll={(e) => { scrolled.current.both = [e.currentTarget.scrollLeft, e.currentTarget.scrollTop]; }}>
            <div className="grid w-[960px] grid-cols-12 auto-rows-[60px]">
              {Array.from({ length: 120 }, (_, i) => <div key={i} className="border border-white/10 p-2 text-sm">{i % 12}, {Math.floor(i / 12)}</div>)}
            </div>
          </SwipeScrollArea>
        </Section>
      </div>
    </main>
  );
}
