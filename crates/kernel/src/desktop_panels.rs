use super::*;
use desktop::{Status, Stream, Touch, TouchPhase};
use std::time::{Duration, Instant};

pub(super) struct DesktopPanel {
    stream: Option<Stream>,
    pub status: Status,
    contacts: [Option<Vec2>; 2],
    original_resolution: (u32, u32),
    meter: Meter,
    pub stats: Option<DesktopStats>,
}

/// 配信の直近一秒の統計。表示用。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DesktopStats {
    /// VR へ渡した画像の数（毎秒）。
    pub fps: f32,
    /// OS が画面を取り込んでから VR へ渡し終えるまで（ms。平均）。
    pub latency_ms: f32,
    /// VR への画像の転送にかかった時間（ms。平均）。
    pub upload_ms: f32,
    pub width: u32,
    pub height: u32,
}

/// 一秒ごとに区切って [`DesktopStats`] にする。
struct Meter {
    since: Instant,
    frames: u32,
    latency: Duration,
    upload: Duration,
}

impl Meter {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            frames: 0,
            latency: Duration::ZERO,
            upload: Duration::ZERO,
        }
    }

    fn add(&mut self, latency: Duration, upload: Duration) {
        self.frames += 1;
        self.latency += latency;
        self.upload += upload;
    }

    /// 一秒たっていれば区切る。画像が来なかった区間（画面が止まっている）は、遅延と転送を前の値のまま残す。
    fn roll(
        &mut self,
        now: Instant,
        last: Option<DesktopStats>,
        size: (u32, u32),
    ) -> Option<DesktopStats> {
        let span = now.saturating_duration_since(self.since);
        if span < Duration::from_secs(1) {
            return last;
        }
        let last = last.unwrap_or_default();
        let ms = |d: Duration| d.as_secs_f32() * 1000.0 / self.frames as f32;
        let stats = DesktopStats {
            fps: self.frames as f32 / span.as_secs_f32(),
            latency_ms: if self.frames == 0 {
                last.latency_ms
            } else {
                ms(self.latency)
            },
            upload_ms: if self.frames == 0 {
                last.upload_ms
            } else {
                ms(self.upload)
            },
            width: size.0,
            height: size.1,
        };
        *self = Self::new(now);
        Some(stats)
    }
}

impl<B: VrBackend> Kernel<B> {
    /// テストでも本番と同じ受け渡しで、配信を既存のパネルへ接続する。
    pub fn set_desktop_stream(
        &mut self,
        panel: PanelId,
        stream: Stream,
    ) -> Result<(), desktop::Error> {
        let p = self.overlay.panel(panel);
        if p.closed || p.pointer.is_some() {
            return Err(desktop::Error(
                "desktop stream needs an open content panel".into(),
            ));
        }
        let original_resolution = self
            .desktop_panels
            .get(&panel)
            .map_or(p.resolution, |p| p.original_resolution);
        self.desktop_panels.insert(
            panel,
            DesktopPanel {
                stream: Some(stream),
                status: Status::Starting,
                contacts: [None; 2],
                original_resolution,
                meter: Meter::new(Instant::now()),
                stats: None,
            },
        );
        self.overlay.set_panel_capture(panel, true);
        Ok(())
    }

    pub fn desktop_streaming(&self, panel: PanelId) -> bool {
        self.desktop_panels
            .get(&panel)
            .is_some_and(|p| p.stream.is_some() && p.status == Status::Streaming)
    }

    pub(super) fn start_desktop(
        &mut self,
        panel: PanelId,
        source: Option<desktop::Source>,
        frame_rate: u32,
    ) {
        if let Some(source) = source {
            match Stream::start_at(source, frame_rate)
                .and_then(|stream| self.set_desktop_stream(panel, stream))
            {
                Ok(()) => (),
                Err(error) => {
                    logger::warn!("desktop capture failed", panel = panel, error = error);
                    let original_resolution = self
                        .desktop_panels
                        .get(&panel)
                        .map_or(self.overlay.panel(panel).resolution, |p| {
                            p.original_resolution
                        });
                    self.overlay.set_panel_capture(panel, false);
                    self.desktop_panels.insert(
                        panel,
                        DesktopPanel {
                            stream: None,
                            status: Status::Failed(error),
                            contacts: [None; 2],
                            original_resolution,
                            meter: Meter::new(Instant::now()),
                            stats: None,
                        },
                    );
                }
            }
        } else if let Some(p) = self.desktop_panels.remove(&panel) {
            self.overlay.set_panel_capture(panel, false);
            self.overlay
                .set_content_resolution(panel, p.original_resolution);
        }
    }

    pub(super) fn poll_desktop(&mut self) {
        self.desktop_panels
            .retain(|id, _| !self.overlay.panel(*id).closed);
        let now = Instant::now();
        for (&id, panel) in &mut self.desktop_panels {
            if panel.stream.is_some() && panel.status == Status::Streaming {
                let size = self.overlay.panel(id).resolution;
                panel.stats = panel.meter.roll(now, panel.stats, size);
            }
            let Some(update) = panel.stream.as_ref().and_then(Stream::poll) else {
                continue;
            };
            if panel.status != Status::Streaming && update.status == Status::Streaming {
                // 開始待ち（Linux ではポータルの選択）の時間を最初の一秒に混ぜない。
                panel.meter = Meter::new(now);
            }
            panel.status = update.status;
            if let Status::Failed(error) = &panel.status {
                logger::warn!("desktop capture stopped", panel = id, error = error);
            }
            if let Some(frame) = update.frame {
                self.overlay
                    .set_content_resolution(id, (frame.width, frame.height));
                let started = Instant::now();
                self.overlay.submit(
                    &mut self.subsystems.backend,
                    id,
                    frame.width,
                    frame.height,
                    &frame.rgba,
                );
                let done = Instant::now();
                panel.meter.add(
                    done.saturating_duration_since(frame.captured),
                    done - started,
                );
            }
            if matches!(panel.status, Status::Closed | Status::Failed(_)) {
                panel.stream = None;
                panel.stats = None;
                panel.contacts = [None; 2];
                self.overlay.set_panel_capture(id, false);
            }
        }
    }

    pub(super) fn route_desktop(&mut self, events: &mut Vec<UiEvent>) {
        // 親の閉鎖や外側クリックによって、この tick 内で閉じられた配信も止める。
        self.desktop_panels
            .retain(|id, _| !self.overlay.panel(*id).closed);
        events.retain(|event| {
            let Some(panel) = self.desktop_panels.get_mut(&event.panel) else {
                return true;
            };
            let Some(stream) = &panel.stream else {
                return true;
            };
            if panel.status != Status::Streaming {
                return true;
            }
            let id = match event.hand {
                Hand::Left => 0,
                Hand::Right => 1,
            };
            let resolution = self.overlay.panel(event.panel).resolution;
            let contact = &mut panel.contacts[id];
            let touch = match event.event {
                PointerEvent::Down { pos } if contact.is_none() => {
                    *contact = Some(pos);
                    Some((TouchPhase::Down, pos))
                }
                PointerEvent::Move { pos } if contact.is_some() => {
                    *contact = Some(pos);
                    Some((TouchPhase::Move, pos))
                }
                PointerEvent::Up { pos } if contact.take().is_some() => Some((TouchPhase::Up, pos)),
                PointerEvent::Leave => contact.take().map(|p| (TouchPhase::Cancel, p)),
                _ => None,
            };
            if let Some((phase, pos)) = touch {
                let result = stream.touch(Touch {
                    id: id as u32,
                    phase,
                    x: pos.x / resolution.0 as f32,
                    y: pos.y / resolution.1 as f32,
                });
                if let Err(error) = result {
                    logger::warn!("desktop touch failed", panel = event.panel, error = error);
                    panel.status = Status::Failed(error);
                    panel.stream = None;
                    panel.contacts = [None; 2];
                    self.overlay.set_panel_capture(event.panel, false);
                }
            }
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop::{Frame, Update, mock};
    use std::sync::Arc;
    use vr::{Quat, RawController, Vec3, mock::MockBackend};

    fn setup() -> (Kernel<MockBackend>, PanelId, mock::Controller) {
        let mut kernel = Kernel::with_backend(
            MockBackend::default(),
            Config {
                pointer_smoothing: Some(0.0),
                pointer_mode: Some(PointerMode::Mouse),
                ..Default::default()
            },
        );
        let panel = kernel
            .add_panel(
                "desktop",
                Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
                Vec2::new(1.0, 0.5),
                (100, 50),
            )
            .unwrap();
        kernel.apply(panel, PanelCommand::Animated(false));
        let (stream, control) = mock::stream();
        kernel.set_desktop_stream(panel, stream).unwrap();
        control.publish(Update {
            status: Status::Streaming,
            frame: Some(Arc::new(Frame::new(4, 2, vec![128; 32]).unwrap())),
        });
        kernel.tick();
        (kernel, panel, control)
    }

    fn hand(kernel: &mut Kernel<MockBackend>, x: f32, select: bool) {
        kernel.subsystems.backend.frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: "khr/simple_controller".into(),
            aim: Pose::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY),
            inputs: vec![("select/click".into(), if select { 1.0 } else { 0.0 })],
        }];
    }

    #[test]
    fn latest_frame_reaches_overlay_and_gpui_cannot_overwrite_it() {
        let (mut kernel, panel, control) = setup();
        assert_eq!(kernel.panel(panel).resolution, (4, 2));
        assert_eq!(kernel.panel(panel).size, Vec2::new(1.0, 0.5));
        kernel.submit(panel, 1, 1, &[255; 4]);
        assert_eq!(
            kernel.subsystems.backend.overlays[0]
                .texture
                .as_ref()
                .unwrap()
                .rgba,
            vec![128; 32]
        );
        control.publish(Update {
            status: Status::Streaming,
            frame: Some(Arc::new(Frame::new(2, 4, vec![64; 32]).unwrap())),
        });
        kernel.tick();
        assert_eq!(kernel.panel(panel).size, Vec2::new(1.0, 2.0));
        assert_eq!(kernel.status(panel).desktop, Some(Status::Streaming));
    }

    #[test]
    fn pointer_becomes_native_touch_and_keeps_capture_even_in_mouse_mode() {
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, true);
        assert!(kernel.tick().is_empty());
        assert_eq!(
            control.touches(),
            [Touch {
                id: 1,
                phase: TouchPhase::Down,
                x: 0.5,
                y: 0.5
            }]
        );
        hand(&mut kernel, 0.8, true);
        kernel.tick();
        assert_eq!(
            control.touches()[0],
            Touch {
                id: 1,
                phase: TouchPhase::Move,
                x: 1.0,
                y: 0.5
            }
        );
        hand(&mut kernel, 0.8, false);
        kernel.tick();
        assert!(control.touches().iter().any(|t| t.phase == TouchPhase::Up));
    }

    #[test]
    fn lost_controller_cancels_contact_and_closing_stops_worker() {
        let (mut kernel, panel, control) = setup();
        hand(&mut kernel, 0.0, true);
        kernel.tick();
        control.touches();
        kernel.subsystems.backend.frame.controllers.clear();
        kernel.tick();
        assert_eq!(control.touches()[0].phase, TouchPhase::Cancel);
        kernel.apply(panel, PanelCommand::Close);
        kernel.tick();
        assert!(control.stopped());
    }

    #[test]
    fn both_hands_are_independent_contacts_and_grabbing_cancels_them() {
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, true);
        let mut left = kernel.subsystems.backend.frame.controllers[0].clone();
        left.hand = Hand::Left;
        kernel.subsystems.backend.frame.controllers.push(left);
        kernel.tick();
        let touches = control.touches();
        assert_eq!(touches.len(), 2);
        assert!(
            touches
                .iter()
                .any(|t| t.id == 0 && t.phase == TouchPhase::Down)
        );
        assert!(
            touches
                .iter()
                .any(|t| t.id == 1 && t.phase == TouchPhase::Down)
        );
        // simple_controller の掴み入力は無いため、ここだけ Index の共通アクション変換を通す。
        kernel.subsystems.backend.frame.controllers[0].profile = "valve/index_controller".into();
        kernel.subsystems.backend.frame.controllers[0].inputs =
            vec![("trigger/value".into(), 1.0), ("squeeze/value".into(), 1.0)];
        kernel.tick();
        let touches = control.touches();
        assert!(
            touches
                .iter()
                .any(|t| t.id == 1 && t.phase == TouchPhase::Cancel)
        );
        assert!(
            touches
                .iter()
                .all(|t| t.id != 0 || t.phase == TouchPhase::Move)
        );
    }

    #[test]
    fn meter_averages_each_second_and_keeps_latency_while_the_screen_is_still() {
        let t0 = Instant::now();
        let mut meter = Meter::new(t0);
        meter.add(Duration::from_millis(10), Duration::from_millis(2));
        meter.add(Duration::from_millis(30), Duration::from_millis(4));
        assert_eq!(meter.roll(t0 + Duration::from_millis(500), None, (4, 2)), None);
        let stats = meter
            .roll(t0 + Duration::from_secs(2), None, (4, 2))
            .unwrap();
        assert_eq!(stats.fps, 1.0);
        assert!((stats.latency_ms - 20.0).abs() < 1e-3);
        assert!((stats.upload_ms - 3.0).abs() < 1e-3);
        assert_eq!((stats.width, stats.height), (4, 2));
        let still = meter
            .roll(t0 + Duration::from_secs(3), Some(stats), (4, 2))
            .unwrap();
        assert_eq!(still.fps, 0.0);
        assert_eq!(still.latency_ms, stats.latency_ms);
    }

    #[test]
    fn stopping_restores_original_aspect_and_failure_releases_stream() {
        let (mut kernel, panel, control) = setup();
        kernel.apply(
            panel,
            PanelCommand::Stream(None, desktop::DEFAULT_FRAME_RATE),
        );
        assert!(control.stopped());
        assert_eq!(kernel.panel(panel).resolution, (100, 50));
        assert_eq!(kernel.status(panel).desktop, None);
        let (stream, control) = mock::stream();
        kernel.set_desktop_stream(panel, stream).unwrap();
        control.publish(Update {
            status: Status::Failed(desktop::Error("source closed".into())),
            frame: None,
        });
        kernel.tick();
        assert!(control.stopped());
        assert!(matches!(
            kernel.status(panel).desktop,
            Some(Status::Failed(_))
        ));
    }
}
