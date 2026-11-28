//! フレームループ（[`run`] / [`pump`]）: カーネルを進行させ、パネルウィンドウの開閉を管理し、入力を配送して描画を読み戻す。

use super::*;

/// ホバーのみの入力で再描画する間隔の下限（約 40Hz）。
const HOVER_REDRAW: Duration = Duration::from_millis(25);
/// 再描画しても画像が変わらないパネルの再描画間隔の上限（時計の分の切り替わりはこの遅延で視認可能）。
const IDLE_MAX: Duration = Duration::from_secs(1);

/// ウィンドウごとのツールチップ要求（[`design::components::Tooltips`]）を、パネルを指している手に振り分け、
/// 手ごとの向き（[`Kernel::tooltip_anchor`]）とともに [`PanelControl`] へ書き込む。変更があれば `true` を返す（ツールチップを再描画する）。
/// 全体設定でツールチップが無効の場合は振り分けない。入力配送後に毎フレーム呼び出す。本番と testkit で共通。
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

/// 1 フレーム分の処理: 下位パイプラインを実行し、パネル宛てイベントを GPUI 入力に変換して返す。
/// 本番の [`run`] とテスト用 testkit が共通で通る経路。
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
        let event = if kernel.panel_mouse_buttons(e.panel) {
            e.event
        } else {
            e.event.primary()
        };
        let input = bridge.translate(event, mode);
        if bridge.held() {
            // 長押しでドラッグに遷移した。他パネルへ移動可能にするため、オーバーレイのキャプチャを解除する。
            kernel.release_capture(e.hand);
        }
        // GPUI のマウスはウィンドウに 1 つ。この手が移動させたため、同じパネルのもう一方の手は
        // 次の入力で必ず位置を再送出する（再送出しないと、押下がもう一方の手の位置に届いて押せない場合があった）。
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

/// テスト等がフレームループに挿入するフック。
#[derive(Default)]
pub struct Hooks {
    /// 毎フレーム、メインスレッドで `App` にアクセスする処理を受け取る（別スレッドのテストが GPUI を操作する）。
    pub remote: Option<std::sync::mpsc::Receiver<Remote>>,
    /// 計測は実行するがプロファイラパネルは表示しない（ベンチマーク自身の描画が計測を汚染しないようにする）。
    pub hide_profiler: bool,
}

/// フレームループ（メインスレッド）で実行する処理。
pub type Remote = Box<dyn FnOnce(&mut App) + Send>;

/// パネルを描画する GPUI アプリケーション。プラットフォームの選択は GPUI に委ねる。
pub fn application() -> gpui::Application {
    gpui_platform::application()
}

/// メインスレッドを占有する。GPUI のイベントループがランタイムのフレームループも駆動する。
/// どのランタイム（OpenVR / OpenXR / novr）でも同一の経路を通る（周期は `poll` が vsync まで待機して生成する）。
/// パネルに載せる画面（上位層）は `panels` で受け取る。UI 層はどの画面かを関知しない。
/// `profiler` を指定すると起動時から計測を開始し（`PanelControl::profiling`）、段階ごとの所要時間をプロファイラパネルにグラフ表示する。
pub fn run(kernel: Kernel, panels: Vec<PanelSpec>, profiler: Option<Profiler>) {
    run_with(kernel, panels, profiler, Hooks::default())
}

/// [`run`] にテスト用フック（[`Hooks`]）を追加したもの。
pub fn run_with(
    mut kernel: Kernel,
    panels: Vec<PanelSpec>,
    profiler: Option<Profiler>,
    hooks: Hooks,
) {
    let remote = hooks.remote;
    let hide_profiler = hooks.hide_profiler;
    // novr はフレーム周期が固定であるため、ドロップをその長さで計数する（間隔からの推定はジッターに弱い）。
    let budget = kernel.frame_interval();
    let with_budget = move |p: Profiler| match budget {
        Some(b) => p.with_budget(b),
        None => p,
    };
    application().run(move |cx: &mut App| {
        // 画面生成時から実際の設定（テーマ・言語・利き手）を参照可能にするため、パネルを開く前に一度反映する。
        // トランジション有効化前のため、起動時のテーマはアニメーションなしで確定する。
        sync_settings(&mut kernel, cx);
        // 部品のトランジションを有効化する（テストキットでは設定しない）。
        cx.set_global(design::Motion::default());
        design::font::install(cx);
        design::corner::install();
        // キーボード初回表示時にディスクから配列を読み始めると、完了までキーのないカードが表示される。事前に読み込む。
        kernel::subsystems::keyboard::preload();
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
            // ホバーのみの Move を受け取ったが、まだ再描画していないパネル。
            let mut hover_pending = std::collections::HashSet::new();
            // 描画して読み戻しを発行したが、まだ結果を受け取っていないパネル。
            let mut collecting = std::collections::HashSet::new();
            let mut render_sizes = HashMap::new();
            let limit = kernel
                .max_overlays()
                .map_or(MAX_PANELS, |m| m.min(MAX_PANELS));
            // ドラッグ中の手と、ドラッグの見た目を描画するパネル。
            let mut drag: Option<(Hand, PanelId)> = None;
            // VR では poll がブロックして周期を生成する。待機前に一度 GPUI へ制御を返す。
            // 0 だとこのループが即座に再スケジュールされ、後続の GPUI タスクが実行されない。
            // 途中で開いたパネルのリサイズ（SetWindowPos はタスクで実行される）が反映されず、1x1 のまま表示されなかった。
            // poll は次の vsync までの残り時間を待機するため、ここで 1ms 譲っても周期に影響しない。
            let wait = Duration::from_millis(1);
            loop {
                let begin = Instant::now();
                // GPUI のタイマーは Windows では粗い（1ms を要求しても 6〜12ms 掛かり、フレームレートが 60fps 台で頭打ちになる）。
                // バックグラウンドスレッドで待機してから復帰する（std の sleep は高精度）。
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
                // タッチの長押し進捗を手ごとに記録し、変更があればカーソルを再描画する（リングが充填されていく）。
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
                // 前フレームの配送でホバーした要素のツールチップを手に振り分ける。変更があればツールチップパネルを再描画する。
                let tooltip_changed = cx.update(|cx| {
                    let ids = slots.open.iter().map(|&p| (p, slots.window(p).window_id()));
                    sync_tooltips(&kernel, &bridges, ids, cx)
                });
                for &p in &slots.open {
                    let w = slots.window(p);
                    let part = kernel.panel(p).pointer.map(|(_, part)| part);
                    if hold_changed && part == Some(Pointer::Cursor) {
                        slots.drawn[p] = None;
                    }
                    if tooltip_changed && part == Some(Pointer::Tooltip) {
                        let _ = cx.update_window(w, |_, w, _| w.refresh());
                        slots.drawn[p] = None;
                    }
                }
                // ドラッグの見た目は中心がカーソル位置に配置されるため、そのウィンドウのマウスを中心に設定して毎フレーム描画する。
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
                // wait = ランタイムの待機時間（VR は vsync、novr はエミュレーターの周期）。オーバーレイ層と合成は別途計測する。
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
                // WindowHandle::update はルートビューを借用したままにするため、draw やクリック処理で
                // ビューを更新すると二重借用でパニックする。AnyWindowHandle 経由で借用を回避する。
                // アイドル状態のパネルの再描画は 1 フレームに 1 枚まで（同一フレームに重なると読み戻しの待機が積み上がる）。
                let mut idle_slot = true;
                for panel in slots.open.iter().copied() {
                    let window = slots.window(panel);
                    let inputs = inputs.remove(&panel).unwrap_or_default();
                    if kernel.panel(panel).closed {
                        cx.update(|cx| {
                            sync_panel(&mut kernel, panel, cx);
                            let _ = cx.update_window(window, |_, window, _| window.remove_window());
                        });
                        slots.last[panel] = None;
                        continue;
                    }
                    // 描画と読み戻し（GPU 完了待ちを含む）は高コストのため、入力・状態の変化があった場合と、
                    // 前回描画から idle[panel] 経過した場合（1 フレームに 1 枚）のみ実行する。
                    // 非表示パネル（他パネルに遮蔽・視野外）はアイドル時に再描画しない（カリング）。
                    let visible = kernel.panel_seen(panel);
                    if visible && !slots.seen[panel] {
                        slots.drawn[panel] = None;
                    }
                    slots.seen[panel] = visible;
                    // ボタン非押下の Move のみの場合、変化し得るのはホバーの見た目のみ（カーソルは別パネルで毎フレーム移動する）。
                    // 再描画を HOVER_REDRAW ごとに間引き、間引いた分は後続フレームで描画する（読み戻しはゲーム中 1 回数 ms 掛かる）。
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
                    // 遠距離（LOD1 以上）のパネルはフレームを間引いて描画する。
                    let due = kernel.panel_frame_due(panel);
                    // 配信中のパネルは GPUI で描画せず描画時刻が進まないため、1 枚の枠を毎フレーム消費させない。
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
                        // LOD・ポインタ状態・ドックヒント・角の線の色はそのパネル固有の状態。変更のたびに他パネルまで再描画させない。
                        let status = kernel.status(panel);
                        others = changed
                            && before.is_none_or(|b| {
                                PanelStatus {
                                    lod: status.lod,
                                    pointed: status.pointed,
                                    dock_hint: status.dock_hint,
                                    corner_held: status.corner_held,
                                    ..b
                                } != status
                            });
                        // 部品やテーマ切替のトランジション実行中は毎フレーム再描画する。
                        let transitioning = cx.has_global::<design::Motion>()
                            && cx
                                .global_mut::<design::Motion>()
                                .animating
                                .remove(&window.window_handle().window_id())
                            || design::Theme::fading(cx);
                        // 要求された再描画は非表示であれば破棄する（表示された瞬間に再描画する）。
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
                            // ビューが描画ごとに変化する。GPUI が再描画するよう無効化する。`refresh` だと共有ビュー（Markdown 等）まで再描画するため、ルートのみ無効化する。
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
                        // 配信画像はカーネルが直接送出する。GPUI の描画・読み戻しで毎フレーム GPU を待機しない。
                        if kernel.desktop_streaming(panel) {
                            return None;
                        }
                        if redraw && !due {
                            // 間引かれたフレーム。次に描画可能なフレームで必ず描画する。
                            slots.drawn[panel] = None;
                            redraw = false;
                        }
                        if redraw {
                            let t = Instant::now();
                            window.draw(cx).clear(cx);
                            let scene = Instant::now();
                            // 読み戻しは GPU 完了を待たず 1 フレーム遅れで返す。描画結果は次のフレームで受け取る。
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
                        // ウィンドウが OS の閉じるボタン等で閉じられた。該当パネルのみ閉じる（全パネル閉鎖後に下段で終了する）。
                        kernel.apply(panel, PanelCommand::Close);
                        slots.last[panel] = None;
                        continue;
                    };
                    if kernel.panel(panel).closed {
                        // 閉じたパネルの最終描画（1 枚約 2MB）を解放する。
                        slots.last[panel] = None;
                    }
                    if others {
                        // 状態は任意のパネルに表示されうるため、他パネルも再描画する（1 フレームに 1 枚ずつ）。
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
                        // 開直後はウィンドウサイズの反映が間に合わず 1x1 で描画される。送出せず次フレームで再描画する。
                        Some(Ok(f))
                            if render_sizes.get(&panel).is_some_and(|&(res, lod)| {
                                (f.width, f.height)
                                    != ((res.0 >> lod).max(1), (res.1 >> lod).max(1))
                            }) =>
                        {
                            slots.drawn[panel] = None;
                        }
                        // ピクセルが変化した場合のみ送出する（GPU 書き込みと完了待ちを省略する）。
                        Some(Ok(f)) => {
                            slots.drawn[panel] = Some(Instant::now());
                            let same = slots.last[panel].as_ref() == Some(&f);
                            // 要求による描画は推測間隔を短縮しない（自発的に再描画を要求するビューは推測描画が不要）。
                            // 描画したフレームの画像は前回の読み戻し（1 フレーム遅れ）のため、変化の判定は受け取ったフレームで行う。
                            slots.idle[panel] = if idle_due && same && !changed {
                                // 他パネルの状態変化で 0 にリセットした間隔は、倍にしても 0 のまま毎フレーム再描画して
                                // 1 枚の枠を占有し続け、後続パネルが描画されなくなる。下限から再度延伸する。
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
                // 終了が要求されたか、全ウィンドウが（OS の閉じるボタン等で）閉じられた場合に終了する。
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
                // 閉じたパネルの手ごとの状態（慣性スクロール等）は持ち越さない。
                bridges.retain(|&(p, _), _| !kernel.panel(p).closed);
                render_sizes.retain(|&p, _| !kernel.panel(p).closed);
                for spec in spawns {
                    // 連打により開いたままのパネルが際限なく増加することを防止する。
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

/// フレームループがパネルごとに保持する状態。`windows` 以外の添字は PanelId（カーネルが開いた順に 0 から割り当てる）。
/// カーネルが自分で開くパネル（配信しているウィンドウのメニュー）にはウィンドウが無いので、その PanelId は欠番になる。
#[derive(Default)]
pub(crate) struct Slots {
    /// 開いたウィンドウ（PanelId の順）。
    pub(crate) windows: Vec<(PanelId, AnyWindowHandle)>,
    /// 開いているパネル（閉じたものは除く）。
    open: Vec<PanelId>,
    /// 最後に送出した描画（同一なら送出しない）。
    last: Vec<Option<Frame>>,
    /// 最後に描画した時刻（None = 次のフレームで必ず描画する）。
    pub(crate) drawn: Vec<Option<Instant>>,
    /// 入力も変化もない場合に再描画する間隔。描画しても変化がなければ IDLE_MAX まで延伸する。
    idle: Vec<Duration>,
    /// 前フレームで表示されていたか（遮蔽・視野外だったパネルは表示された瞬間に再描画する）。
    seen: Vec<bool>,
}

impl Slots {
    /// 開いたパネルを追加する。PanelId は開いた順に増える。
    pub(crate) fn push(&mut self, (panel, window): (PanelId, AnyWindowHandle)) {
        // 添字でアクセスするため、戻ると別パネルの状態を上書きしてしまう。
        assert!(panel >= self.last.len(), "PanelId must grow");
        self.windows.push((panel, window));
        self.open.push(panel);
        // 間のウィンドウの無いパネルの分も空けておく。
        let n = panel + 1;
        self.last.resize_with(n, || None);
        self.drawn.resize(n, None);
        self.idle.resize(n, IDLE_REDRAW);
        self.seen.resize(n, false);
    }

    /// `panel` のウィンドウ。
    pub(crate) fn window(&self, panel: PanelId) -> AnyWindowHandle {
        let i = self
            .windows
            .binary_search_by_key(&panel, |&(id, _)| id)
            .expect("the panel has a window");
        self.windows[i].1
    }
}

/// 終了する。プロファイル実行中であれば集計をログに出力する。
pub(crate) fn quit(cx: &mut App) {
    log_summary(cx);
    cx.quit();
}

/// プロファイル実行中であれば集計をログに出力する。
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
