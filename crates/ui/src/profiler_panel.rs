//! 計測中だけ出すプロファイラのパネル。

use super::*;

/// プロファイラパネルのグラフを描き直す間隔（約 10Hz）。
const PROFILER_REDRAW: Duration = Duration::from_millis(100);
/// プロファイラパネルの位置。開いた瞬間の頭から見て正面 1m、少し上（Testing と重ならないように）。
const PROFILER_POSE: Pose = Pose::new(Vec3::new(0.0, 0.25, -1.0), Quat::IDENTITY);

/// 計測とプロファイラパネルを [`PanelControl::profiling`] に合わせ、このフレームの段階ごとの時間を記録する。
pub(crate) fn sync_profiler(
    kernel: &mut Kernel,
    slots: &mut Slots,
    profiler_panel: &mut Option<PanelId>,
    hide_profiler: bool,
    with_budget: &impl Fn(Profiler) -> Profiler,
    stages: Stages,
    cx: &mut gpui::AsyncApp,
) {
    if let Some(p) = *profiler_panel
        && kernel.panel(p).closed
    {
        // OS の X などで閉じられた。
        *profiler_panel = None;
        cx.update(|cx| cx.default_global::<PanelControl>().profiling = false);
    }
    let profiling = cx.update(|cx| cx.default_global::<PanelControl>().profiling);
    if profiling != cx.update(|cx| cx.has_global::<Profiler>()) {
        cx.update(|cx| {
            logger::info!("profiler toggled", profiling = profiling);
            if profiling {
                // --profile 無しで始めたときは CSV に書かない。
                let p = Profiler::new(None).expect("cannot fail without an output path");
                cx.set_global(with_budget(p));
            } else {
                log_summary(cx);
                cx.remove_global::<Profiler>();
            }
            // 切り替えボタンの表示を変えるため、どのパネルも描き直させる。
            cx.refresh_windows();
        });
        slots.drawn.iter_mut().for_each(|t| *t = None);
    }
    match *profiler_panel {
        None if profiling && !hide_profiler => {
            let spec = PanelSpec::root(Pose::IDENTITY, |_, _, cx| ProfilerPanel {
                view: cx.new(|_| ProfilerView),
            })
            .titled("testing.panel.profiler")
            .sized(Vec2::new(1.1, 0.56), (1100, 560))
            .in_view(PROFILER_POSE);
            let i = slots.windows.len();
            match cx.update(|cx| open_panel(kernel, spec, i, cx)) {
                Some(opened) => {
                    let panel = opened.0;
                    logger::info!(
                        "profiler panel opened",
                        panel = panel,
                        pose = ?kernel.panel(panel).pose
                    );
                    slots.push(opened);
                    *profiler_panel = Some(panel);
                }
                // 開けなかった。毎フレーム作り直そうとしないよう計測ごと止める。
                None => cx.update(|cx| cx.default_global::<PanelControl>().profiling = false),
            }
        }
        Some(p) if !profiling => {
            // 次のフレームの sync_panel が閉じてウィンドウを消す。
            cx.update(|cx| command(p, PanelCommand::Close, cx));
            *profiler_panel = None;
        }
        _ => {}
    }
    cx.update(|cx| {
        if cx.has_global::<Profiler>() {
            cx.global_mut::<Profiler>().record(Instant::now(), stages);
        }
        if let Some(p) = *profiler_panel
            && slots.drawn[p].is_none_or(|t| t.elapsed() >= PROFILER_REDRAW)
        {
            // グラフは毎フレーム変わるが、読むには PROFILER_REDRAW ごとで足りる。毎フレーム描くと、描画・読み戻し・
            // 合成のコストで計測そのものを重くする（プロファイラの窓だけで 1 フレーム 20ms 近かった）。
            let _ = cx.update_window(slots.windows[p].1, |_, w, _| w.refresh());
            slots.drawn[p] = None;
        }
    });
}

/// プロファイラのパネル。閉じるのはナビゲーションバー（閉じると計測も止まる）。
struct ProfilerPanel {
    view: Entity<ProfilerView>,
}

impl Render for ProfilerPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x17181c)).child(self.view.clone())
    }
}
