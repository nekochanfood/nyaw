//! プロファイリング中のみ表示するプロファイラパネル。

use super::*;

/// プロファイラパネルのグラフ再描画間隔（約 10Hz）。
const PROFILER_REDRAW: Duration = Duration::from_millis(100);
/// プロファイラパネルの配置位置。起動時の頭部正面 1m、やや上方（Testing パネルと重ならないように）。
const PROFILER_POSE: Pose = Pose::new(Vec3::new(0.0, 0.25, -1.0), Quat::IDENTITY);

/// プロファイリングとプロファイラパネルを [`PanelControl::profiling`] に同期し、今フレームのステージ別時間を記録する。
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
        // OS の閉じるボタン等で閉じられた。
        *profiler_panel = None;
        cx.update(|cx| cx.default_global::<PanelControl>().profiling = false);
    }
    let profiling = cx.update(|cx| cx.default_global::<PanelControl>().profiling);
    if profiling != cx.update(|cx| cx.has_global::<Profiler>()) {
        cx.update(|cx| {
            logger::info!("profiler toggled", profiling = profiling);
            if profiling {
                // --profile なしで起動した場合は CSV に書き出さない。
                let p = Profiler::new(None).expect("cannot fail without an output path");
                cx.set_global(with_budget(p));
            } else {
                log_summary(cx);
                cx.remove_global::<Profiler>();
            }
            // トグルボタンの表示を更新するため、全パネルを再描画する。
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
                // 起動に失敗。毎フレーム再試行しないようプロファイリングごと停止する。
                None => cx.update(|cx| cx.default_global::<PanelControl>().profiling = false),
            }
        }
        Some(p) if !profiling => {
            // 次フレームの sync_panel が閉じてウィンドウを破棄する。
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
            // グラフは毎フレーム変化するが、判読には PROFILER_REDRAW 間隔で十分。毎フレーム描画すると描画・リードバック・
            // 合成のコストでプロファイリング自体が重くなる（プロファイラウィンドウだけで 1 フレーム約 20ms を要した）。
            let _ = cx.update_window(slots.windows[p].1, |_, w, _| w.refresh());
            slots.drawn[p] = None;
        }
    });
}

/// プロファイラパネル。閉じるのはナビゲーションバー経由（閉じるとプロファイリングも停止する）。
struct ProfilerPanel {
    view: Entity<ProfilerView>,
}

impl Render for ProfilerPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x17181c)).child(self.view.clone())
    }
}
