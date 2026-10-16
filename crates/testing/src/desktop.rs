use super::*;
use kernel::desktop::{DEFAULT_FRAME_RATE, MAX_FRAME_RATE, Source, SourceKind, Sources, Status};
use std::cell::Cell;

/// 取得元を選び、独立したパネルへの配信と停止・再接続を試す。
pub struct DesktopTest {
    panel: PanelId,
    sources: Vec<Source>,
    pending: Option<Sources>,
    error: Option<String>,
    viewer: Rc<Cell<Option<PanelId>>>,
    scroll: ScrollHandle,
    frame_rate: u32,
}

impl Default for DesktopTest {
    fn default() -> Self {
        Self {
            panel: 0,
            sources: Vec::new(),
            pending: None,
            error: None,
            viewer: Rc::default(),
            scroll: ScrollHandle::new(),
            frame_rate: DEFAULT_FRAME_RATE,
        }
    }
}

impl DesktopTest {
    pub fn new(panel: PanelId) -> Self {
        let mut this = Self {
            panel,
            ..Default::default()
        };
        this.refresh();
        this
    }

    fn refresh(&mut self) {
        match Sources::refresh() {
            Ok(pending) => {
                self.pending = Some(pending);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn status_text(status: Option<&Status>, cx: &App) -> SharedString {
    match status {
        None => i18n::t(cx, "desktop.idle"),
        Some(Status::Starting) => i18n::t(cx, "desktop.starting"),
        Some(Status::Streaming) => i18n::t(cx, "desktop.streaming"),
        Some(Status::Closed) => i18n::t(cx, "desktop.closed"),
        Some(Status::Failed(error)) => i18n::tf(cx, "desktop.failed", &[("error", &error)]),
    }
}

struct DesktopViewer {
    panel: PanelId,
    name: String,
}
impl Render for DesktopViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let status = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .and_then(|s| s.status.desktop.as_ref());
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(self.name.clone())
            .child(
                div()
                    .debug_selector(|| "desktop-viewer-status".into())
                    .child(status_text(status, cx)),
            )
    }
}

/// 統計の大きさ（m）と解像度（px）。文字が収まる最小限にし、余白は透明にする。
const STATS_SIZE: Vec2 = Vec2::new(0.16, 0.1);
const STATS_RESOLUTION: (u32, u32) = (160, 100);

/// 配信パネルの左上の角に重ねる統計（FPS・遅延・取得・転送・解像度）。行ごとに薄い下地を敷く。
struct StreamStats {
    panel: PanelId,
}
impl Render for StreamStats {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        let stats = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .and_then(|s| s.status.desktop_stats);
        let lines = match stats {
            None => vec![i18n::t(cx, "desktop.stats_measuring")],
            Some(s) => vec![
                i18n::tf(cx, "desktop.stats_fps", &[("fps", &format!("{:.1}", s.fps))]),
                i18n::tf(
                    cx,
                    "desktop.stats_latency",
                    &[("ms", &format!("{:.1}", s.latency_ms))],
                ),
                i18n::tf(
                    cx,
                    "desktop.stats_capture",
                    &[("ms", &format!("{:.1}", s.capture_ms))],
                ),
                i18n::tf(
                    cx,
                    "desktop.stats_upload",
                    &[("ms", &format!("{:.1}", s.upload_ms))],
                ),
                i18n::tf(
                    cx,
                    "desktop.stats_size",
                    &[("width", &s.width), ("height", &s.height)],
                ),
            ],
        };
        div()
            .debug_selector(|| "desktop-stats".into())
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .text_color(rgba(theme.on_tooltip))
            .font(design::font::mono())
            .text_size(px(font_size::SM))
            .children(lines.into_iter().map(|line| {
                div()
                    .px(px(spacing::XS))
                    .bg(rgba(theme.tooltip & !0xff | 0x80))
                    .child(line)
            }))
    }
}

impl Render for DesktopTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(result) = self.pending.as_ref().and_then(Sources::poll) {
            self.pending = None;
            match result {
                Ok(mut sources) => {
                    sources.sort_by(|a, b| a.name.cmp(&b.name));
                    self.sources = sources;
                    self.error = None;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        let theme = Theme::current(cx);
        let viewer = self.viewer.get();
        let frame_rate = self.frame_rate;
        let view = cx.entity().downgrade();
        let status = viewer
            .and_then(|id| {
                cx.try_global::<PanelControl>()
                    .and_then(|c| c.panels.get(&id))
            })
            .filter(|s| !s.status.closed)
            .and_then(|s| s.status.desktop.as_ref());
        let status = status_text(status, cx);
        let rows = self
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let kind = match source.kind {
                    SourceKind::Screen => i18n::t(cx, "desktop.screen"),
                    SourceKind::Window => i18n::t(cx, "desktop.window"),
                };
                let label = format!("{kind} · {}", source.name);
                let source = source.clone();
                let viewer = self.viewer.clone();
                let manager = self.panel;
                div().id(("desktop-source-row", index)).child(
                    button(&theme, "desktop-source", label, false)
                        .debug_selector(move || format!("desktop-source-{index}"))
                        .on_click(move |_, _, cx| {
                            if let Some(panel) = viewer.take() {
                                cx.default_global::<PanelControl>()
                                    .panels
                                    .entry(panel)
                                    .or_default()
                                    .commands
                                    .push(PanelCommand::Close);
                            }
                            let source = source.clone();
                            let viewer = viewer.clone();
                            cx.default_global::<PanelControl>().spawns.push(
                                PanelSpec::new(Pose::IDENTITY, move |panel, _, cx| {
                                    let ctl = cx.default_global::<PanelControl>();
                                    if let Some(previous) = viewer.replace(Some(panel)) {
                                        ctl.panels
                                            .entry(previous)
                                            .or_default()
                                            .commands
                                            .push(PanelCommand::Close);
                                    }
                                    let closed =
                                        ctl.panels.get(&manager).is_some_and(|p| p.status.closed);
                                    ctl.panels
                                        .entry(panel)
                                        .or_default()
                                        .commands
                                        .push(if closed {
                                            PanelCommand::Close
                                        } else {
                                            PanelCommand::Stream(Some(source.clone()), frame_rate)
                                        });
                                    if !closed {
                                        ctl.spawns.push(
                                            PanelSpec::child(panel, true, move |_, _, _| {
                                                StreamStats { panel }
                                            })
                                            .hud()
                                            .sized(STATS_SIZE, STATS_RESOLUTION),
                                        );
                                    }
                                    DesktopViewer {
                                        panel,
                                        name: source.name,
                                    }
                                })
                                .in_view(Pose::new(Vec3::new(0.75, 0.0, -1.2), Quat::IDENTITY)),
                            );
                        }),
                )
            })
            .collect::<Vec<_>>();
        let panel = self.panel;
        let viewer = self.viewer.clone();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(i18n::t(cx, "desktop.title"))
                    .child(
                        button(
                            &theme,
                            "close-desktop-test",
                            i18n::t(cx, "common.close"),
                            false,
                        )
                        .on_click(move |_, _, cx| {
                            let ctl = cx.default_global::<PanelControl>();
                            if let Some(id) = viewer.take() {
                                ctl.panels
                                    .entry(id)
                                    .or_default()
                                    .commands
                                    .push(PanelCommand::Close);
                            }
                            ctl.panels
                                .entry(panel)
                                .or_default()
                                .commands
                                .push(PanelCommand::Close);
                        }),
                    ),
            )
            .child(div().child(i18n::t(cx, "desktop.instructions")))
            .child(
                row()
                    .child(
                        div()
                            .debug_selector(|| "desktop-frame-rate-value".into())
                            .child(i18n::tf(cx, "desktop.frame_rate", &[("rate", &frame_rate)])),
                    )
                    .child(
                        components::slider(
                            &theme,
                            "desktop-frame-rate",
                            (frame_rate - 1) as f32 / (MAX_FRAME_RATE - 1) as f32,
                            move |value, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.frame_rate =
                                        1 + (value * (MAX_FRAME_RATE - 1) as f32).round() as u32;
                                    cx.notify();
                                });
                            },
                        )
                        .length(240.),
                    ),
            )
            .child(
                row()
                    .child(
                        button(
                            &theme,
                            "desktop-refresh",
                            i18n::t(cx, "desktop.refresh"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh();
                            cx.notify();
                        })),
                    )
                    .child(
                        button(&theme, "desktop-stop", i18n::t(cx, "desktop.stop"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(panel) = this.viewer.take() {
                                    cx.default_global::<PanelControl>()
                                        .panels
                                        .entry(panel)
                                        .or_default()
                                        .commands
                                        .push(PanelCommand::Close);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "desktop-status".into())
                    .child(status),
            )
            .when(self.pending.is_some(), |d| {
                d.child(i18n::t(cx, "desktop.loading"))
            })
            .when(
                self.pending.is_none() && self.sources.is_empty() && self.error.is_none(),
                |d| d.child(i18n::t(cx, "desktop.empty")),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(div().child(i18n::tf(cx, "desktop.failed", &[("error", &error)])))
            })
            .child(
                div()
                    .id("desktop-sources")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(spacing::SM))
                    .children(rows),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};
    use kernel::desktop::mock;

    #[gpui::test]
    fn source_selection_queues_a_stream_and_stop_closes_the_viewer(cx: &mut TestAppContext) {
        let source = mock::source("Test window", SourceKind::Window);
        let (_view, cx) = cx.add_window_view(|_, _| DesktopTest {
            sources: vec![source.clone()],
            ..Default::default()
        });
        cx.run_until_parked();
        let at = cx.debug_bounds("desktop-source-0").unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.update(|window, cx| {
            let spec = cx
                .default_global::<PanelControl>()
                .spawns
                .pop()
                .expect("stream viewer requested");
            let _ = spec.build(1, window, cx);
            assert_eq!(
                cx.global::<PanelControl>().panels[&1].commands,
                [PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE)]
            );
        });
        let at = cx.debug_bounds("desktop-stop").unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.update(|_, cx| {
            assert!(
                cx.global::<PanelControl>().panels[&1]
                    .commands
                    .contains(&PanelCommand::Close)
            )
        });
    }

    #[gpui::test]
    fn repeated_selection_keeps_one_viewer_and_closed_manager_does_not_start_capture(
        cx: &mut TestAppContext,
    ) {
        let source = mock::source("Test window", SourceKind::Window);
        let (_view, cx) = cx.add_window_view(|_, _| DesktopTest {
            sources: vec![source],
            ..Default::default()
        });
        cx.run_until_parked();
        let at = cx.debug_bounds("desktop-source-0").unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.simulate_click(at, Modifiers::none());
        cx.update(|window, cx| {
            let specs = std::mem::take(&mut cx.default_global::<PanelControl>().spawns);
            assert_eq!(specs.len(), 2);
            for (index, spec) in specs.into_iter().enumerate() {
                let _ = spec.build(index + 1, window, cx);
            }
            assert!(
                cx.global::<PanelControl>().panels[&1]
                    .commands
                    .contains(&PanelCommand::Close)
            );
        });
        cx.simulate_click(at, Modifiers::none());
        cx.update(|window, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(0)
                .or_default()
                .status
                .closed = true;
            let spec = cx.default_global::<PanelControl>().spawns.pop().unwrap();
            let _ = spec.build(3, window, cx);
            assert_eq!(
                cx.global::<PanelControl>().panels[&3].commands,
                [PanelCommand::Close]
            );
        });
    }

    #[gpui::test]
    fn frame_rate_slider_is_applied_to_the_next_stream(cx: &mut TestAppContext) {
        let source = mock::source("Test window", SourceKind::Window);
        let (view, cx) = cx.add_window_view(|_, _| DesktopTest {
            sources: vec![source.clone()],
            ..Default::default()
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |v, _| v.frame_rate), DEFAULT_FRAME_RATE);
        for (right, expected) in [(true, MAX_FRAME_RATE), (false, 1)] {
            let handle = cx
                .debug_bounds("desktop-frame-rate-handle")
                .unwrap()
                .center();
            let track = cx.debug_bounds("desktop-frame-rate").unwrap();
            let end = gpui::point(if right { track.right() } else { track.left() }, handle.y);
            cx.simulate_mouse_down(handle, gpui::MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_move(end, gpui::MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
            assert_eq!(view.read_with(cx, |v, _| v.frame_rate), expected);
        }
        let at = cx.debug_bounds("desktop-source-0").unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.update(|window, cx| {
            let spec = cx.default_global::<PanelControl>().spawns.pop().unwrap();
            let _ = spec.build(1, window, cx);
            assert_eq!(
                cx.global::<PanelControl>().panels[&1].commands,
                [PanelCommand::Stream(Some(source), 1)]
            );
        });
    }
}
