//! フレームループ（[`run`] / [`pump`]）: カーネルを進め、パネルの窓を開け閉めし、入力を配って描画を読み戻す。

use super::*;

/// ホバーだけの入力で描き直す間隔の下限（約 40Hz）。
const HOVER_REDRAW: Duration = Duration::from_millis(25);
/// 描き直しても絵が変わらないパネルの、描き直す間隔の上限（時計の分の切り替わりはこの遅れで見える）。
const IDLE_MAX: Duration = Duration::from_secs(1);

/// ウィンドウごとのツールチップの要求（[`design::components::Tooltips`]）を、そのパネルを指している手に振り分け、
/// 手ごとの向き（[`Kernel::tooltip_anchor`]）とともに [`PanelControl`] へ書く。変わったら `true`（ツールチップを描き直す）。
/// 全体の設定でツールチップを切っていれば振り分けない。入力を配送した後に毎フレーム呼ぶ。本番と testkit 共通。
pub fn sync_tooltips<B: VrBackend>(
    kernel: &Kernel<B>,
    bridges: &HashMap<(PanelId, Hand), PointerBridge>,
    windows: impl IntoIterator<Item = (PanelId, gpui::WindowId)>,
    cx: &mut App,
) -> bool {
    let windows: Vec<_> = windows.into_iter().collect();
    let shown = settings(cx).tooltips;
    cx.default_global::<design::components::TooltipSource>()
        .1
        .retain(|id| windows.iter().any(|(_, window)| window == id));
    let requests = cx.default_global::<design::components::Tooltips>();
    requests.drop_stale();
    let mut tooltip = HashMap::new();
    for (panel, window) in windows {
        for hand in [Hand::Left, Hand::Right] {
            let index = match hand {
                Hand::Left => 0,
                Hand::Right => 1,
            };
            if shown
                && let Some((_, text, _)) = requests.0.get(&(window, index))
                && bridges.get(&(panel, hand)).is_some_and(|b| b.on_panel())
            {
                tooltip.insert(hand, text.clone());
            }
        }
    }
    let anchor: HashMap<Hand, Anchor> = [Hand::Left, Hand::Right]
        .into_iter()
        .map(|h| (h, kernel.tooltip_anchor(h)))
        .collect();
    let ctl = cx.default_global::<PanelControl>();
    let changed = ctl.tooltip != tooltip || ctl.anchor != anchor;
    (ctl.tooltip, ctl.anchor) = (tooltip, anchor);
    changed
}

/// 1フレーム分: 下位パイプラインを回し、パネル宛てのイベントを GPUI 入力に変換して返す。
/// 本番の [`run`] とテスト用の testkit が共通で通る経路。
pub fn pump<B: VrBackend>(
    kernel: &mut Kernel<B>,
    bridges: &mut HashMap<(PanelId, Hand), PointerBridge>,
) -> Vec<(PanelId, Hand, PlatformInput)> {
    let mut gpui = Vec::new();
    for e in kernel.tick() {
        let mode = kernel.panel_pointer_mode(e.panel);
        let bridge = bridges.entry((e.panel, e.hand)).or_default();
        let p = kernel.panel(e.panel);
        let distance = p.pose.position.distance(kernel.hmd().position);
        bridge.slop =
            SWIPE_SLOP_DEG.to_radians().tan() * distance * p.resolution.0 as f32 / p.size.x;
        let input = bridge.translate(e.event, mode);
        if bridge.held() {
            // 長押しでドラッグになった。ほかのパネルへ運べるよう、オーバーレイのキャプチャを外す。
            kernel.release_capture(e.hand);
        }
        // GPUI のマウスはウィンドウに 1 つ。この手が動かしたので、同じパネルのもう一方の手は「マウスはもう自分の所にある」と
        // 思わず、次は必ず位置から送り直す（送らないと、押下がもう一方の手の位置に届いて押せないことがあった）。
        if !input.is_empty() {
            for (&(panel, hand), b) in bridges.iter_mut() {
                if panel == e.panel && hand != e.hand {
                    b.rehover = true;
                }
            }
        }
        gpui.extend(input.into_iter().map(|input| (e.panel, e.hand, input)));
    }
    for (&(panel, hand), b) in bridges.iter_mut() {
        if kernel.desktop_streaming(panel) {
            if b.on_panel() || b.pressed || b.touch.is_some() {
                gpui.extend(
                    b.translate(PointerEvent::Leave, kernel.panel_pointer_mode(panel))
                        .into_iter()
                        .map(|input| (panel, hand, input)),
                );
            }
            *b = PointerBridge::default();
            continue;
        }
        if let Some(input) = b.momentum() {
            gpui.push((panel, hand, input));
        }
    }
    gpui
}

/// テストなどがフレームループに差し込む口。
#[derive(Default)]
pub struct Hooks {
    /// 毎フレーム、メインスレッドで `App` を触る処理を受け取る（別スレッドのテストが GPUI を操作する）。
    pub remote: Option<std::sync::mpsc::Receiver<Remote>>,
    /// 計測はするがプロファイラパネルを出さない（ベンチマークが自分の描画で計測を汚さないように）。
    pub hide_profiler: bool,
}

/// フレームループ（メインスレッド）で実行してもらう処理。
pub type Remote = Box<dyn FnOnce(&mut App) + Send>;

/// パネルを描く GPUI のアプリ。プラットフォームの選択は GPUI に任せる。
pub fn application() -> gpui::Application {
    gpui_platform::application()
}

/// メインスレッドを占有する。GPUI のイベントループがランタイムのフレームループも回す。
/// どのランタイム（OpenVR / OpenXR / novr）でも同じ経路を通る（周期は `poll` が vsync まで待って作る）。
/// パネルに載せる画面（上の層）は `panels` で受け取る。UI 層はどの画面かを知らない。
/// `profiler` があれば起動時から計測し（`PanelControl::profiling`）、段階ごとの所要時間をプロファイラパネルにグラフで出す。
pub fn run(kernel: Kernel, panels: Vec<PanelSpec>, profiler: Option<Profiler>) {
    run_with(kernel, panels, profiler, Hooks::default())
}

/// [`run`] にテスト用の差し込み口（[`Hooks`]）を足したもの。
pub fn run_with(
    mut kernel: Kernel,
    panels: Vec<PanelSpec>,
    profiler: Option<Profiler>,
    hooks: Hooks,
) {
    let remote = hooks.remote;
    let hide_profiler = hooks.hide_profiler;
    // novr はフレームの周期が決まっているので、ドロップをその長さで数える（間隔からの推定は揺れに弱い）。
    let budget = kernel.frame_interval();
    let with_budget = move |p: Profiler| match budget {
        Some(b) => p.with_budget(b),
        None => p,
    };
    application().run(move |cx: &mut App| {
        // 画面が作られた時から本当の設定（テーマ・言語・利き手）を見られるよう、パネルを開く前に一度反映する。
        // トランジションを有効にする前なので、起動時のテーマは移り変わらずに決まる。
        sync_settings(&mut kernel, cx);
        // 部品のトランジションを有効にする（テストのキットは置かない）。
        cx.set_global(design::Motion::default());
        design::font::install(cx);
        design::corner::install();
        // 初めてキーボードを出すときにディスクから配列を読み始めると、それまでキーの無いカードが出る。先に読んでおく。
        keyboard::preload();
        if let Some(p) = profiler {
            cx.set_global(with_budget(p));
            cx.default_global::<PanelControl>().profiling = true;
        }
        let mut profiler_panel: Option<PanelId> = None;
        let mut slots = Slots::default();
        for (i, spec) in panels.into_iter().enumerate() {
            if let Some(opened) = open_panel(&mut kernel, spec, i, cx) {
                slots.push(opened);
            }
        }

        cx.spawn(async move |cx| {
            let mut bridges = HashMap::new();
            let mut warned = false;
            // ホバーだけの Move を受けて、まだ描いていないパネル。
            let mut hover_pending = std::collections::HashSet::new();
            // 描いて読み戻しを積んだまま、まだ受け取っていないパネル。
            let mut collecting = std::collections::HashSet::new();
            let mut render_sizes = HashMap::new();
            let limit = kernel
                .max_overlays()
                .map_or(MAX_PANELS, |m| m.min(MAX_PANELS));
            // ドラッグ中の手と、その見た目を描くパネル。
            let mut drag: Option<(Hand, PanelId)> = None;
            // VR では poll がブロックして周期を作る。待ちの前に一度 GPUI へ制御を返す。
            // 0 だとこのループがすぐ再スケジュールされ、後から積まれた GPUI のタスクが走らない。
            // 途中で開いたパネルのリサイズ（SetWindowPos はタスクで行われる）が届かず、1x1 のまま表示されなかった。
            // poll は次の vsync までの残りを眠るので、ここで 1ms 譲っても周期は変わらない。
            let wait = Duration::from_millis(1);
            loop {
                let begin = Instant::now();
                // GPUI のタイマーは Windows では粗く（1ms を頼んでも 6〜12ms 掛かり、フレームが 60fps 台に頭打ちになる）、
                // バックグラウンドのスレッドで眠ってから戻る（std の sleep は細かい）。
                cx.background_executor()
                    .spawn(async move { std::thread::sleep(wait) })
                    .await;
                if let Some(remote) = &remote {
                    while let Ok(f) = remote.try_recv() {
                        cx.update(f);
                    }
                }
                let mut inputs: HashMap<PanelId, Vec<(Hand, PlatformInput)>> = HashMap::new();
                let gpui_inputs = pump(&mut kernel, &mut bridges);
                for (panel, hand, input) in gpui_inputs {
                    inputs.entry(panel).or_default().push((hand, input));
                }
                // タッチの長押しの進み具合を手ごとに置き、変わればカーソルを描き直す（リングが満ちていく）。
                let mut hold: HashMap<Hand, f32> = HashMap::new();
                for (&(_, hand), b) in &bridges {
                    if let Some(p) = b.hold_progress() {
                        let e = hold.entry(hand).or_default();
                        *e = e.max(p);
                    }
                }
                let hold_changed = cx.update(|cx| {
                    let ctl = cx.default_global::<PanelControl>();
                    let changed = ctl.hold != hold;
                    ctl.hold = hold;
                    changed
                });
                // 前のフレームの配送でホバーした要素のツールチップを手に振り分ける。変わればツールチップのパネルを描き直す。
                let tooltip_changed = cx.update(|cx| {
                    let ids = slots
                        .open
                        .iter()
                        .map(|&p| (p, slots.windows[p].1.window_id()));
                    sync_tooltips(&kernel, &bridges, ids, cx)
                });
                for &p in &slots.open {
                    let w = slots.windows[p].1;
                    let part = kernel.panel(p).pointer.map(|(_, part)| part);
                    if hold_changed && part == Some(Pointer::Cursor) {
                        slots.drawn[p] = None;
                    }
                    if tooltip_changed && part == Some(Pointer::Tooltip) {
                        let _ = cx.update_window(w, |_, w, _| w.refresh());
                        slots.drawn[p] = None;
                    }
                }
                // ドラッグの見た目は中心がカーソルに来るよう置かれるので、そのウィンドウのマウスを中心に置いて毎フレーム描く。
                if let Some((_, dp)) = drag {
                    let (w, h) = kernel.panel(dp).resolution;
                    let pos = Vec2::new(w as f32, h as f32) / 2.0;
                    inputs.entry(dp).or_default().extend(
                        PointerBridge::default()
                            .translate(PointerEvent::Move { pos }, PointerMode::Mouse)
                            .into_iter()
                            .map(|input| (Hand::Right, input)),
                    );
                }
                // wait = ランタイムの待ち（VR は vsync、novr はエミュレーターの周期）。オーバーレイ層と合成は別に測る。
                let timing = kernel.timing();
                let mut stages = Stages {
                    wait: begin
                        .elapsed()
                        .saturating_sub(timing.overlay + timing.compositor),
                    overlay: timing.overlay,
                    compositor: timing.compositor,
                    ..Default::default()
                };
                if kernel.quit_requested() {
                    cx.update(quit);
                    return;
                }
                // WindowHandle::update はルートビューを借用したままになり、draw やクリック処理で
                // ビューを更新すると二重借用で落ちる。AnyWindowHandle 経由で借用しない。
                // 何もしていないパネルの描き直しは 1 フレームに 1 枚まで（同じフレームに重なると読み戻しの待ちが積み上がる）。
                let mut idle_slot = true;
                for panel in slots.open.iter().copied() {
                    let window = slots.windows[panel].1;
                    let inputs = inputs.remove(&panel).unwrap_or_default();
                    if kernel.panel(panel).closed {
                        cx.update(|cx| {
                            sync_panel(&mut kernel, panel, cx);
                            let _ = cx.update_window(window, |_, window, _| window.remove_window());
                        });
                        slots.last[panel] = None;
                        continue;
                    }
                    // 描画と読み戻し（GPU の完了待ちを含む）は重いので、入力・状態の変化があったとき、
                    // 前回描いてから idle[panel] 経ったとき（1 フレームに 1 枚）だけ行う。
                    // 見えていないパネル（ほかのパネルに隠れた・視野の外）は、何もしていないときに描き直さない（カリング）。
                    let visible = kernel.panel_seen(panel);
                    if visible && !slots.seen[panel] {
                        slots.drawn[panel] = None;
                    }
                    slots.seen[panel] = visible;
                    // ボタンを押していない Move だけなら、変わりうるのはホバーの見た目だけ（カーソルは別のパネルで毎フレーム動く）。
                    // 描き直しを HOVER_REDRAW ごとに間引き、間引いた分は後で描く（読み戻しはゲーム中 1 回数 ms 掛かる）。
                    let hover_only = !inputs.is_empty()
                        && inputs.iter().all(|(_, i)| match i {
                            PlatformInput::MouseMove(m) => m.pressed_button.is_none(),
                            _ => false,
                        });
                    if hover_only {
                        hover_pending.insert(panel);
                    }
                    let hover_due = hover_pending.contains(&panel)
                        && slots.drawn[panel].is_none_or(|t| t.elapsed() >= HOVER_REDRAW);
                    let forced = (!inputs.is_empty() && !hover_only)
                        || hover_due
                        || slots.drawn[panel].is_none();
                    if forced {
                        hover_pending.remove(&panel);
                    }
                    // 遠い（LOD1 以上の）パネルはフレームを間引いて描く。
                    let due = kernel.panel_frame_due(panel);
                    // 配信中のパネルは GPUI で描かず描いた時刻が進まないので、1 枚の枠を毎フレーム取らせない。
                    let idle_due = !forced
                        && visible
                        && !kernel.desktop_streaming(panel)
                        && due
                        && idle_slot
                        && slots.drawn[panel].is_some_and(|t| t.elapsed() >= slots.idle[panel]);
                    idle_slot &= !idle_due;
                    let mut redraw = forced || idle_due;
                    let mut changed = false;
                    let mut asked = false;
                    let mut others = false;
                    let mut collected = false;
                    let Ok(frame) = cx.update_window(window, |_, window, cx| {
                        let t = Instant::now();
                        for (hand, input) in inputs {
                            dispatch_input(panel, hand, input, window, cx);
                        }
                        let before = cx
                            .try_global::<PanelControl>()
                            .and_then(|c| c.panels.get(&panel))
                            .map(|s| s.status.clone());
                        changed = sync_panel(&mut kernel, panel, cx);
                        redraw |= changed;
                        // LOD と指されているかはそのパネルだけの状態。変わるたびにほかのパネルまで描き直させない。
                        let status = kernel.status(panel);
                        others = changed
                            && before.is_none_or(|b| {
                                PanelStatus {
                                    lod: status.lod,
                                    pointed: status.pointed,
                                    ..b
                                } != status
                            });
                        // 部品やテーマの切り替えのトランジションが動いている間は、毎フレーム描き直す。
                        let transitioning = cx.has_global::<design::Motion>()
                            && cx
                                .global_mut::<design::Motion>()
                                .animating
                                .remove(&window.window_handle().window_id())
                            || design::Theme::fading(cx);
                        // 頼まれた描き直しは、見えていなければ捨てる（見えた瞬間に描き直す）。
                        asked = cx.has_global::<PanelControl>()
                            && cx
                                .global_mut::<PanelControl>()
                                .panels
                                .get_mut(&panel)
                                .is_some_and(|s| std::mem::take(&mut s.redraw));
                        redraw |= asked && visible;
                        if visible
                            && (transitioning
                                || cx
                                    .try_global::<PanelControl>()
                                    .and_then(|c| c.panels.get(&panel))
                                    .is_some_and(|s| s.live))
                        {
                            // ビューが描くたびに変わる。GPUI が描き直すよう汚す。`refresh` だと使い回すビュー（Markdown など）まで描き直すので、根だけ汚す。
                            match window.root::<PanelRoot>().flatten() {
                                Some(root) => root.update(cx, |_, cx| cx.notify()),
                                None => window.refresh(),
                            }
                            redraw = true;
                        }
                        let resolution = kernel.panel(panel).resolution;
                        let lod = kernel.panel_lod(panel);
                        if !kernel.desktop_streaming(panel)
                            && render_sizes.insert(panel, (resolution, lod))
                                != Some((resolution, lod))
                        {
                            set_render_lod(window, cx, resolution, lod);
                            slots.drawn[panel] = None;
                            redraw = false;
                        }
                        if !cx.has_active_drag() {
                            if let Some((hand, _)) = drag.take() {
                                kernel.set_drag(hand, None);
                            }
                        } else if drag.is_none() {
                            drag = start_drag(&mut kernel, &bridges, &slots.windows, panel);
                        }
                        stages.input += t.elapsed();
                        if kernel.panel(panel).closed {
                            window.remove_window();
                            return None;
                        }
                        // 配信画像はカーネルが直接送る。GPUI の描画・読み戻しで毎フレーム GPU を待たない。
                        if kernel.desktop_streaming(panel) {
                            return None;
                        }
                        if redraw && !due {
                            // 間引いたフレーム。次に描いてよいフレームで必ず描く。
                            slots.drawn[panel] = None;
                            redraw = false;
                        }
                        if redraw {
                            let t = Instant::now();
                            window.draw(cx).clear(cx);
                            let scene = Instant::now();
                            // 読み戻しは GPU の完了を待たず 1 回遅れで返る。描いた絵は次のフレームで受け取る。
                            let frame = read_back(window);
                            collecting.insert(panel);
                            stages.draw += scene - t;
                            stages.render += scene.elapsed();
                            Some(frame)
                        } else if collecting.remove(&panel) {
                            let t = Instant::now();
                            let frame = collect(window);
                            stages.render += t.elapsed();
                            collected = true;
                            frame.map(Ok)
                        } else {
                            None
                        }
                    }) else {
                        // ウィンドウが OS の X などで閉じられた。そのパネルだけ閉じる（全部閉じたら下で終了する）。
                        kernel.apply(panel, PanelCommand::Close);
                        slots.last[panel] = None;
                        continue;
                    };
                    if kernel.panel(panel).closed {
                        // 閉じたパネルの最後の描画（1 枚 2MB 近い）は手放す。
                        slots.last[panel] = None;
                    }
                    if others {
                        // 状態はどのパネルにも表示されうるので、ほかのパネルも描き直す（1 フレームに 1 枚ずつ）。
                        for &p in &slots.open {
                            let t = &mut slots.drawn[p];
                            if p != panel
                                && let Some(t) = t
                            {
                                *t = t.checked_sub(IDLE_MAX).unwrap_or(*t);
                                slots.idle[p] = Duration::ZERO;
                            }
                        }
                    }
                    match frame {
                        // 開いた直後はウィンドウの大きさの反映が間に合わず 1x1 で描かれる。送らずに次のフレームで描き直す。
                        Some(Ok(f))
                            if render_sizes.get(&panel).is_some_and(|&(res, lod)| {
                                (f.width, f.height)
                                    != ((res.0 >> lod).max(1), (res.1 >> lod).max(1))
                            }) =>
                        {
                            slots.drawn[panel] = None;
                        }
                        // 画素が変わったときだけ送る（GPU 書き込みと完了待ちを省く）。
                        Some(Ok(f)) => {
                            slots.drawn[panel] = Some(Instant::now());
                            let same = slots.last[panel].as_ref() == Some(&f);
                            // 頼まれて描いた分は推測の間隔を縮めない（自分で頼むビューは推測で描かなくてよい）。
                            // 描いたフレームの絵は前の読み戻し（1 回遅れ）なので、変わったかは受け取ったフレームで決める。
                            slots.idle[panel] = if idle_due && same && !changed {
                                // ほかのパネルの状態の変化で 0 に詰めた間隔は、倍にしても 0 のまま毎フレーム描き直して
                                // 1 枚の枠を取り続け、後ろのパネルが描かれなくなる。下限から伸ばし直す。
                                (slots.idle[panel] * 2).clamp(IDLE_REDRAW, IDLE_MAX)
                            } else if !changed && (asked || (collected && same)) {
                                slots.idle[panel]
                            } else {
                                IDLE_REDRAW
                            };
                            if !same {
                                let t = Instant::now();
                                kernel.submit(panel, f.width, f.height, &f.rgba);
                                stages.upload += t.elapsed();
                                slots.last[panel] = Some(f);
                            }
                        }
                        Some(Err(e)) if !warned => {
                            logger::error!("cannot get panel render", error = e, panel = panel);
                            warned = true;
                        }
                        _ => {}
                    }
                }
                // 終了を頼まれたか、ウィンドウが全部（OS の X などで）閉じられたら終了する。
                slots.open.retain(|&p| !kernel.panel(p).closed);
                hover_pending.retain(|p| !kernel.panel(*p).closed);
                collecting.retain(|p| !kernel.panel(*p).closed);
                if cx.update(|cx| cx.default_global::<PanelControl>().quit)
                    || !slots.windows.is_empty() && slots.open.is_empty()
                {
                    cx.update(quit);
                    return;
                }
                let spawns =
                    cx.update(|cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
                // 閉じたパネルの手ごとの状態（慣性のスクロールなど）は持ち越さない。
                bridges.retain(|&(p, _), _| !kernel.panel(p).closed);
                render_sizes.retain(|&p, _| !kernel.panel(p).closed);
                for spec in spawns {
                    // 連打で開きっぱなしのパネルが際限なく増えないように。
                    if slots.open.len() >= limit {
                        logger::error!("too many open panels", max = limit);
                        break;
                    }
                    let i = slots.windows.len();
                    let Some(opened) = cx.update(|cx| open_panel(&mut kernel, spec, i, cx)) else {
                        break;
                    };
                    slots.push(opened);
                }
                let open = slots.open.len();
                cx.update(|cx| {
                    let c = cx.default_global::<PanelControl>();
                    (c.open, c.limit) = (open, limit);
                });
                sync_profiler(
                    &mut kernel,
                    &mut slots,
                    &mut profiler_panel,
                    hide_profiler,
                    &with_budget,
                    stages,
                    cx,
                );
            }
        })
        .detach();
    });
}

/// フレームループがパネルごとに持つ状態。添字は PanelId（カーネルは開いた順に 0 から PanelId を振り、窓も同じ順に積む）。
#[derive(Default)]
pub(crate) struct Slots {
    pub(crate) windows: Vec<(PanelId, AnyWindowHandle)>,
    /// 開いているパネル（閉じたものは除く）。
    open: Vec<PanelId>,
    /// 最後に送った描画（同じなら送らない）。
    last: Vec<Option<Frame>>,
    /// 最後に描いた時刻（None = 次のフレームで必ず描く）。
    pub(crate) drawn: Vec<Option<Instant>>,
    /// 入力も変化も無いときに描き直す間隔。描いても変わらなければ IDLE_MAX まで延ばす。
    idle: Vec<Duration>,
    /// 前のフレームで見えていたか（隠れていた・視野の外だったパネルは見えた瞬間に描き直す）。
    seen: Vec<bool>,
}

impl Slots {
    /// 開いたパネルを積む。
    pub(crate) fn push(&mut self, (panel, window): (PanelId, AnyWindowHandle)) {
        // 添字で引くので、ずれたら別のパネルの窓を描いてしまう。
        assert_eq!(
            panel,
            self.windows.len(),
            "PanelId must match the window index"
        );
        self.windows.push((panel, window));
        self.open.push(panel);
        self.last.push(None);
        self.drawn.push(None);
        self.idle.push(IDLE_REDRAW);
        self.seen.push(false);
    }
}

/// 終了する。プロファイル中なら集計をログへ残す。
pub(crate) fn quit(cx: &mut App) {
    log_summary(cx);
    cx.quit();
}

/// プロファイル中なら集計をログへ残す。
pub(crate) fn log_summary(cx: &App) {
    if let Some(p) = cx.try_global::<Profiler>() {
        let s = p.summary();
        let ms = |d: Duration| format!("{:.3}", d.as_secs_f64() * 1e3);
        logger::info!(
            "profile summary (recent)",
            frames = s.frames,
            dropped = s.dropped,
            fps = format!("{:.1}", s.fps),
            frame_avg_ms = ms(s.frame_avg),
            frame_p99_ms = ms(s.frame_p99),
            cpu_avg_ms = ms(s.cpu_avg),
            gpu_avg_ms = ms(s.gpu_avg)
        );
    }
}

#[cfg(test)]
mod tests;
