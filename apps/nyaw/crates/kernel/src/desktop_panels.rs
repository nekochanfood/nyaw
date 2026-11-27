use super::*;
use desktop::{Button, Status, Stream, Touch, TouchPhase};
use std::time::{Duration, Instant};
use subsystems::input::ClickButton;

pub(super) struct DesktopPanel {
    stream: Option<Stream>,
    /// 配信対象（[`Kernel::start_desktop`] で開始した場合のみ。FPS 変更時に再起動する）。
    source: Option<desktop::Source>,
    pub status: Status,
    contacts: [Option<Vec2>; 2],
    hovered: [Option<Vec2>; 2],
    /// OS のマウスを操作する手。両手で競合しないよう、もう片方の手の入力は送信しない。既定は利き手で、もう片方の手で
    /// 一度押下するとその手に切り替わる（その押下自体は送信しない）。
    active: Hand,
    original_resolution: (u32, u32),
    meter: Meter,
    pub stats: Option<DesktopStats>,
    /// 映している取得元のメニューと、それを映す子パネル（[`sync_popups`]）。配信を差し替えても引き継ぎ、次の tick で合わせる。
    popups: Vec<(desktop::Popup, PanelId)>,
    /// 最後に合わせたメニューの一覧（[`Stream::popups`] が変わったかを見る）。
    popups_seen: Arc<[desktop::Popup]>,
}

/// 配信パネル `parent` の子パネル `shown` を、取得元の手前に出ているメニュー `popups`（手前から順）に合わせる。
/// メニューは取得元とは別のウィンドウで映像に含まれないので、親の面の同じ所に重ねる子パネル（[`Parent::over`]）で見せる。
/// 消えたメニューの子パネルは閉じ、動いた・描き変わったメニューは置き直して送り直す。画像はメニューのウィンドウだけを
/// 取り込んだもの（透明な所は透ける）なので、加工せずに送る。
fn sync_popups(
    overlay: &mut Overlay,
    vr: &mut dyn VrBackend,
    parent: PanelId,
    shown: &mut Vec<(desktop::Popup, PanelId)>,
    popups: &[desktop::Popup],
) {
    shown.retain(|(popup, panel)| {
        let open = !overlay.panel(*panel).closed && popups.iter().any(|p| p.id == popup.id);
        if !open {
            overlay.close(vr, *panel);
        }
        open
    });
    // 後から開いた子パネルほど上に重なる（`Parent::over`）ので、奥のメニューから開く。
    for popup in popups.iter().rev() {
        let at = shown.iter().position(|(p, _)| p.id == popup.id);
        if at.is_some_and(|i| shown[i].0 == *popup) {
            continue;
        }
        let image = &popup.image;
        let (w, h) = (image.width, image.height);
        let panel = match at {
            Some(i) => {
                shown[i].0 = popup.clone();
                shown[i].1
            }
            None => {
                let pose = overlay.panel(parent).pose;
                let key = format!("nyaw.popup.{}", overlay.len());
                let panel = match overlay.add_panel(vr, &key, pose, Vec2::ONE, (w, h)) {
                    Ok(panel) => panel,
                    Err(error) => {
                        logger::warn!("cannot show a desktop popup", panel = parent, error = ?error);
                        continue;
                    }
                };
                // OS のメニュー自体に出入りの動きがあるので、アニメーションは付けない。
                overlay.set_animated(vr, panel, false);
                overlay.set_panel_capture(panel, true);
                if overlay.panel(parent).hidden {
                    overlay.set_hidden(vr, panel, true);
                }
                shown.push((popup.clone(), panel));
                panel
            }
        };
        overlay.set_content_resolution(panel, (w, h));
        overlay.set_parent(
            vr,
            panel,
            Some(Parent {
                panel: parent,
                over: Some(popup.area),
                ..Default::default()
            }),
        );
        overlay.submit(vr, panel, w, h, &image.rgba);
    }
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
    /// ワーカーで GPU から読み出して並べ替えるのに掛かった時間（ms。平均）。
    pub capture_ms: f32,
    pub width: u32,
    pub height: u32,
}

/// 一秒ごとに区切って [`DesktopStats`] にする。
struct Meter {
    since: Instant,
    frames: u32,
    latency: Duration,
    upload: Duration,
    capture: Duration,
}

impl Meter {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            frames: 0,
            latency: Duration::ZERO,
            upload: Duration::ZERO,
            capture: Duration::ZERO,
        }
    }

    fn add(&mut self, latency: Duration, upload: Duration, capture: Duration) {
        self.frames += 1;
        self.latency += latency;
        self.upload += upload;
        self.capture += capture;
    }

    /// 1 秒経過していれば区切る。画像が到着しなかった区間（画面が静止している）は、遅延と転送を前の値のまま保持する。
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
            capture_ms: if self.frames == 0 {
                last.capture_ms
            } else {
                ms(self.capture)
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
        self.replace_desktop(panel, Some(stream), Status::Starting);
        self.overlay.set_panel_capture(panel, true);
        Ok(())
    }

    /// `panel` の配信を `stream` に差し替える。配信を始める前の解像度と、映しているメニューは引き継ぐ。
    fn replace_desktop(&mut self, panel: PanelId, stream: Option<Stream>, status: Status) {
        let (original_resolution, popups) = self
            .desktop_panels
            .remove(&panel)
            .map_or((self.overlay.panel(panel).resolution, Vec::new()), |p| {
                (p.original_resolution, p.popups)
            });
        self.desktop_panels.insert(
            panel,
            DesktopPanel {
                stream,
                source: None,
                status,
                contacts: [None; 2],
                hovered: [None; 2],
                active: self.config.kernel.dominant_hand.hand(),
                original_resolution,
                meter: Meter::new(Instant::now()),
                stats: None,
                popups,
                popups_seen: Arc::from([]),
            },
        );
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
            let window = source.kind == desktop::SourceKind::Window;
            match Stream::start_with(
                source.clone(),
                frame_rate,
                self.subsystems.backend.import_formats(),
            )
            .and_then(|stream| self.set_desktop_stream(panel, stream))
            {
                Ok(()) => {
                    if let Some(p) = self.desktop_panels.get_mut(&panel) {
                        p.source = Some(source);
                    }
                    // 配信開始時にウィンドウを前面化する（そのまま操作できるようにする）。
                    if window {
                        self.focus_desktop(panel);
                    }
                }
                Err(error) => {
                    logger::warn!("desktop capture failed", panel = panel, error = error);
                    self.overlay.set_panel_capture(panel, false);
                    self.replace_desktop(panel, None, Status::Failed(error));
                }
            }
        } else if let Some(mut p) = self.desktop_panels.remove(&panel) {
            sync_popups(
                &mut self.overlay,
                &mut self.subsystems.backend,
                panel,
                &mut p.popups,
                &[],
            );
            self.overlay.set_panel_capture(panel, false);
            self.overlay.set_os_cursor(panel, None);
            self.overlay
                .set_content_resolution(panel, p.original_resolution);
        }
    }

    /// 配信中のミラーを `frame_rate` で再起動する（ウィンドウの前面化は行わない）。
    pub(super) fn restart_desktops(&mut self, frame_rate: u32) {
        let running: Vec<_> = self
            .desktop_panels
            .iter()
            .filter(|(_, p)| p.stream.is_some())
            .filter_map(|(&id, p)| Some((id, p.source.clone()?)))
            .collect();
        for (panel, source) in running {
            match Stream::start_with(
                source.clone(),
                frame_rate,
                self.subsystems.backend.import_formats(),
            )
            .and_then(|stream| self.set_desktop_stream(panel, stream))
            {
                Ok(()) => {
                    if let Some(p) = self.desktop_panels.get_mut(&panel) {
                        p.source = Some(source);
                    }
                }
                Err(error) => logger::warn!("desktop restart failed", panel = panel, error = error),
            }
        }
    }

    /// パネル `panel` が配信中のウィンドウ（画面や未配信時は `None`）。
    pub(super) fn mirrored_window(&self, panel: PanelId) -> Option<u64> {
        self.desktop_panels.get(&panel)?.stream.as_ref()?.window()
    }

    /// パネル `panel` で配信中のウィンドウを前面化する。配信中でなければ何もしない。
    pub(super) fn focus_desktop(&mut self, panel: PanelId) {
        if let Some(stream) = self
            .desktop_panels
            .get(&panel)
            .and_then(|p| p.stream.as_ref())
            && let Err(error) = stream.focus()
        {
            logger::warn!("cannot focus desktop source", panel = panel, error = error);
        }
    }

    pub(super) fn minimize_desktop(&mut self, panel: PanelId) {
        if let Some(stream) = self
            .desktop_panels
            .get(&panel)
            .and_then(|p| p.stream.as_ref())
            && let Err(error) = stream.minimize()
        {
            logger::warn!(
                "cannot minimize desktop source",
                panel = panel,
                error = error
            );
        }
    }

    pub(super) fn poll_desktop(&mut self) {
        self.desktop_panels
            .retain(|id, _| !self.overlay.panel(*id).closed);
        // 非表示の配信は取得を一時停止する。再表示すると停止中の最新の画像から再開する。
        for (&id, panel) in &self.desktop_panels {
            if let Some(stream) = &panel.stream {
                stream.set_paused(!self.panel_seen(id));
            }
        }
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
                // 開始待ちの時間を最初の一秒に混ぜない。
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
                match &frame.external {
                    // 表示サーバーの画像は、ランタイムの GPU が直接取り込む（Linux）。
                    Some(image) => self.overlay.submit_external(
                        &mut self.subsystems.backend,
                        id,
                        image.clone(),
                    ),
                    None => self.overlay.submit(
                        &mut self.subsystems.backend,
                        id,
                        frame.width,
                        frame.height,
                        &frame.rgba,
                    ),
                }
                let done = Instant::now();
                panel.meter.add(
                    done.saturating_duration_since(frame.captured),
                    done - started,
                    frame.cost,
                );
            }
            if matches!(
                panel.status,
                Status::Closed | Status::Minimized | Status::Failed(_)
            ) {
                panel.stream = None;
                panel.stats = None;
                panel.contacts = [None; 2];
                panel.hovered = [None; 2];
                self.overlay.set_panel_capture(id, false);
            }
        }
        // 配信が止まったら（失敗も含む）メニューも消す。
        for (&id, panel) in &mut self.desktop_panels {
            let popups = panel
                .stream
                .as_ref()
                .filter(|_| panel.status == Status::Streaming)
                .map(Stream::popups);
            let vr = &mut self.subsystems.backend;
            match popups {
                Some(popups) if !Arc::ptr_eq(&popups, &panel.popups_seen) => {
                    sync_popups(&mut self.overlay, vr, id, &mut panel.popups, &popups);
                    panel.popups_seen = popups;
                }
                None if !panel.popups.is_empty() => {
                    sync_popups(&mut self.overlay, vr, id, &mut panel.popups, &[]);
                }
                _ => {}
            }
        }
        // OS のカーソルは映像に含めず、指している手のカーソルを差し替えて描画する。
        for (&id, panel) in &self.desktop_panels {
            let cursor = panel
                .stream
                .as_ref()
                .filter(|_| panel.status == Status::Streaming)
                .and_then(Stream::cursor)
                .map(|c| (panel.active, c));
            for p in std::iter::once(id).chain(panel.popups.iter().map(|(_, p)| *p)) {
                self.overlay.set_os_cursor(p, cursor.clone());
            }
        }
    }

    pub(super) fn route_desktop(&mut self, events: &mut Vec<UiEvent>) {
        // 親の閉鎖や外側クリックによって、この tick 内で閉じられた配信も止める。
        self.desktop_panels
            .retain(|id, _| !self.overlay.panel(*id).closed);
        // メニューの子パネルへの入力は、配信パネルの画素に移して配信パネルへの入力にする（はみ出したメニューは面の外）。
        // 配信パネルへの入力は面の中に留める（押したまま外へ出たら端を指す）。
        for event in events.iter_mut() {
            let Some((parent, area)) = self.desktop_panels.iter().find_map(|(&id, p)| {
                if id == event.panel {
                    return Some((id, None));
                }
                p.popups
                    .iter()
                    .find(|(_, panel)| *panel == event.panel)
                    .map(|(popup, _)| (id, Some(popup.area)))
            }) else {
                continue;
            };
            let size = |id| {
                let (w, h) = self.overlay.panel(id).resolution;
                Vec2::new(w as f32, h as f32)
            };
            let (outer, face) = (size(parent), size(event.panel));
            if let PointerEvent::Move { pos }
            | PointerEvent::Down { pos }
            | PointerEvent::Up { pos }
            | PointerEvent::DownButton { pos, .. }
            | PointerEvent::UpButton { pos, .. }
            | PointerEvent::Scroll { pos, .. } = &mut event.event
            {
                *pos = match area {
                    None => pos.clamp(Vec2::ZERO, outer),
                    Some([x, y, w, h]) => {
                        let at = *pos / face;
                        Vec2::new(x + at.x * w, y + at.y * h) * outer
                    }
                };
            }
            event.panel = parent;
        }
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
            let index = |hand| match hand {
                Hand::Left => 0,
                Hand::Right => 1,
            };
            let id = index(event.hand);
            let resolution = self.overlay.panel(event.panel).resolution;
            if event.hand != panel.active {
                // 操作していない手は送らない。押したらその手に替え、その押下（と離す）は送らない。
                if matches!(
                    event.event,
                    PointerEvent::Down { .. } | PointerEvent::DownButton { .. }
                ) {
                    let old = index(panel.active);
                    panel.active = event.hand;
                    panel.hovered[old] = None;
                    if let Some(pos) = panel.contacts[old].take()
                        && let Err(error) = stream.touch(Touch {
                            id: old as u32,
                            phase: TouchPhase::Cancel,
                            x: pos.x / resolution.0 as f32,
                            y: pos.y / resolution.1 as f32,
                        })
                    {
                        logger::warn!("desktop input failed", panel = event.panel, error = error);
                    }
                }
                return false;
            }
            // どちらの手も乗っていなかったパネルにポインタが入ったら、そのウィンドウを前に出す（配信元を操作できるように）。
            if matches!(
                event.event,
                PointerEvent::Move { .. }
                    | PointerEvent::Down { .. }
                    | PointerEvent::DownButton { .. }
            ) && panel.hovered == [None; 2]
                && let Err(error) = stream.focus()
            {
                logger::warn!(
                    "cannot focus desktop source",
                    panel = event.panel,
                    error = error
                );
            }
            let contact = &mut panel.contacts[id];
            let hovered = &mut panel.hovered[id];
            let touch = match event.event {
                PointerEvent::Down { pos } | PointerEvent::DownButton { pos, .. }
                    if contact.is_none() =>
                {
                    *contact = Some(pos);
                    *hovered = Some(pos);
                    Some((TouchPhase::Down, pos))
                }
                PointerEvent::Move { pos } if *hovered != Some(pos) => {
                    *hovered = Some(pos);
                    if contact.is_some() {
                        *contact = Some(pos);
                    }
                    (contact.is_some() || desktop::capabilities().hover)
                        .then_some((TouchPhase::Move, pos))
                }
                PointerEvent::Up { pos } | PointerEvent::UpButton { pos, .. }
                    if contact.take().is_some() =>
                {
                    *hovered = Some(pos);
                    Some((TouchPhase::Up, pos))
                }
                PointerEvent::Leave => {
                    *hovered = None;
                    contact.take().map(|p| (TouchPhase::Cancel, p))
                }
                _ => None,
            };
            let result = if let Some((phase, pos)) = touch {
                let touch = Touch {
                    id: id as u32,
                    phase,
                    x: pos.x / resolution.0 as f32,
                    y: pos.y / resolution.1 as f32,
                };
                match event.event {
                    PointerEvent::DownButton { button, .. }
                    | PointerEvent::UpButton { button, .. } => stream.button(
                        touch,
                        match button {
                            ClickButton::Left => Button::Left,
                            ClickButton::Middle => Button::Middle,
                            ClickButton::Right => Button::Right,
                        },
                    ),
                    _ => stream.touch(touch),
                }
            } else if let PointerEvent::Scroll { pos, delta } = event.event {
                stream.scroll(
                    pos.x / resolution.0 as f32,
                    pos.y / resolution.1 as f32,
                    delta.x,
                    delta.y,
                )
            } else {
                return false;
            };
            if let Err(error) = result {
                logger::warn!("desktop input failed", panel = event.panel, error = error);
                panel.status = Status::Failed(error);
                panel.stream = None;
                panel.contacts = [None; 2];
                panel.hovered = [None; 2];
                self.overlay.set_panel_capture(event.panel, false);
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
        let mut config = Config::default();
        config.overlay.pointer_smoothing = Some(0.0);
        config.kernel.pointer_mode = Some(PointerMode::Mouse);
        let mut kernel = Kernel::with_backend(MockBackend::default(), config);
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
            model: String::new(),
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

    /// 表示サーバーの画像は RGBA に並べ替えずランタイムへ渡し、ランタイムが手放したら書き込み元へ返す。
    #[test]
    fn external_frames_go_to_the_runtime_without_rgba() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (mut kernel, panel, control) = setup();
        let returned = Arc::new(AtomicBool::new(false));
        let flag = returned.clone();
        let pixels: &'static [u8] =
            Box::leak(vec![10, 20, 30, 0, 40, 50, 60, 0].into_boxed_slice());
        let image = vr::ExternalImage::new(
            2,
            1,
            vr::PixelFormat::Bgrx,
            vr::Memory::Host {
                ptr: pixels.as_ptr(),
                len: pixels.len(),
                offset: 0,
                stride: 8,
            },
            move || flag.store(true, Ordering::Release),
        );
        kernel.subsystems.backend.import = Some(Default::default());
        control.publish(Update {
            status: Status::Streaming,
            frame: Some(Arc::new(Frame::external(image))),
        });
        kernel.tick();
        let overlay = &kernel.subsystems.backend.overlays[0];
        assert_eq!(overlay.external_submissions, 1);
        assert_eq!(
            overlay.texture.as_ref().unwrap().rgba,
            [30, 20, 10, 255, 60, 50, 40, 255]
        );
        assert_eq!(kernel.panel(panel).resolution, (2, 1));
        assert!(
            !returned.load(Ordering::Acquire),
            "held while the GPU reads it"
        );
        kernel.subsystems.backend.held = None;
        assert!(returned.load(Ordering::Acquire));
    }

    #[test]
    fn pointer_becomes_desktop_input_and_keeps_capture_even_in_mouse_mode() {
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, true);
        assert!(kernel.tick().is_empty());
        assert_eq!(
            control.touches().last(),
            Some(&Touch {
                id: 1,
                phase: TouchPhase::Down,
                x: 0.5,
                y: 0.5
            })
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

    /// 100×100 画素のメニュー（取得元の幅・高さを 1 とした `area` に出ている）。
    fn popup(id: u64, area: [f32; 4]) -> desktop::Popup {
        desktop::Popup {
            id,
            area,
            image: desktop::Icon {
                width: 100,
                height: 100,
                rgba: [200; 100 * 100 * 4].into(),
            },
        }
    }

    /// 空間の点 (`x`, `y`) を正面から押して離す。届いた押下を返す。
    fn press_at(
        kernel: &mut Kernel<MockBackend>,
        control: &mock::Controller,
        x: f32,
        y: f32,
    ) -> Touch {
        kernel.subsystems.backend.frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: "khr/simple_controller".into(),
            model: String::new(),
            aim: Pose::new(Vec3::new(x, y, 0.0), Quat::IDENTITY),
            inputs: vec![("select/click".into(), 1.0)],
        }];
        kernel.tick();
        let down = control
            .touches()
            .into_iter()
            .find(|t| t.phase == TouchPhase::Down)
            .expect("the press reaches the source");
        kernel.subsystems.backend.frame.controllers[0].inputs[0].1 = 0.0;
        kernel.tick();
        down
    }

    /// 取得元の手前にメニューが出たら、配信パネルを親にして面の同じ所に重ねる子パネルを開き、メニューの絵をそのまま送る。子パネルへの
    /// 入力は取得元の座標（ウィンドウの外へはみ出した所も）で送る。
    /// メニューが消えたら、また配信が止まったら子パネルを閉じる。
    #[test]
    fn popups_become_child_panels_over_the_stream_and_take_input() {
        let (mut kernel, panel, control) = setup();
        // 配信パネルのナビバー。下へはみ出すメニューはこれより上に描く。
        let bar = kernel
            .add_panel("bar", Pose::IDENTITY, Vec2::new(1.0, 0.1), (100, 10))
            .unwrap();
        kernel
            .overlay
            .set_nav_bar(&mut kernel.subsystems.backend, bar, panel);
        // 配信パネルは幅 1m・高さ 0.5m で、中心が (0, 0)。メニューは右下へはみ出す（取得元の幅・高さを 1 として
        // x 0.75〜1.25, y 0.5〜1.5。空間では x 0.25〜0.75m, y 0〜-0.5m）。
        let menu = popup(9, [0.75, 0.5, 0.5, 1.0]);
        control.set_popups(vec![menu.clone()]);
        kernel.tick();
        let child = kernel.overlay.len() - 1;
        let p = kernel.panel(child);
        assert_eq!(
            p.parent.map(|q| (q.panel, q.over)),
            Some((panel, Some(menu.area)))
        );
        assert_eq!(p.resolution, (100, 100));
        let texture = kernel
            .subsystems
            .backend
            .overlays
            .iter()
            .find(|o| o.key.starts_with("nyaw.popup."))
            .and_then(|o| o.texture.as_ref())
            .map(|t| t.rgba.clone())
            .unwrap();
        assert_eq!(
            texture[..],
            menu.image.rgba[..],
            "the menu as captured, nothing added"
        );
        // メニューの右下寄り（配信パネルの外）を押すと、取得元の外の座標で届く。
        let down = press_at(&mut kernel, &control, 0.6, -0.4);
        assert!(
            (down.x - 1.1).abs() < 1e-4 && (down.y - 1.3).abs() < 1e-4,
            "{down:?}"
        );
        // メニューの左端寄り（配信パネルの上）を押すと、メニューの面がそのまま `area` に重なっている所に届く。
        let down = press_at(&mut kernel, &control, 0.27, -0.1);
        assert!(
            (down.x - 0.77).abs() < 1e-4 && (down.y - 0.7).abs() < 1e-4,
            "{down:?}"
        );
        control.set_popups(Vec::new());
        kernel.tick();
        assert!(kernel.panel(child).closed);
        // 手前のメニュー（一覧の先頭）ほど後に開き、上に重なる。
        let submenu = popup(10, [1.25, 0.6, 0.5, 1.0]);
        control.set_popups(vec![submenu.clone(), menu]);
        kernel.tick();
        let child = kernel.overlay.len() - 1;
        assert!(!kernel.panel(child).closed);
        assert_eq!(
            kernel.panel(child).parent.and_then(|q| q.over),
            Some(submenu.area)
        );
        // 描画順は下から、配信パネル・ナビバー・メニュー・サブメニュー。
        kernel.tick();
        let order = [panel, bar, child - 1, child].map(|id| kernel.panel(id).draw_order());
        assert!(order.is_sorted_by(|a, b| a < b), "{order:?}");
        control.publish(Update {
            status: Status::Closed,
            frame: None,
        });
        kernel.tick();
        assert!(kernel.panel(child).closed);
    }

    #[test]
    fn mirror_receives_stick_scroll() {
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, false);
        let c = &mut kernel.subsystems.backend.frame.controllers[0];
        c.profile = "oculus/touch_controller".into();
        c.inputs = vec![("thumbstick/y".into(), 1.0)];
        kernel.tick();
        assert_eq!(control.scrolls(), [(0.5, 0.5, 0.0, 1.0)]);
    }

    #[test]
    fn mirror_keeps_the_button_chosen_on_press() {
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, true);
        let c = &mut kernel.subsystems.backend.frame.controllers[0];
        c.profile = "oculus/touch_controller".into();
        c.inputs = vec![("trigger/value".into(), 1.0), ("b/touch".into(), 1.0)];
        kernel.tick();
        assert_eq!(control.buttons()[0].1, Button::Middle);
        kernel.subsystems.backend.frame.controllers[0]
            .inputs
            .clear();
        kernel.tick();
        assert_eq!(control.buttons()[0].1, Button::Middle);
    }

    #[test]
    fn desktop_hover_moves_without_pressing_when_supported() {
        if !desktop::capabilities().hover {
            return;
        }
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, false);
        kernel.tick();
        assert_eq!(
            control.touches().as_slice(),
            &[Touch {
                id: 1,
                phase: TouchPhase::Move,
                x: 0.5,
                y: 0.5,
            }]
        );
        kernel.tick();
        assert!(
            control.touches().is_empty(),
            "stationary hover should not resend movement"
        );
    }

    /// ポインタが入ったときに一度だけ、配信しているウィンドウを前に出す。
    #[test]
    fn pointer_entering_focuses_the_source_once() {
        let (mut kernel, _, control) = setup();
        control.focuses();
        hand(&mut kernel, 0.0, false);
        kernel.tick();
        assert_eq!(control.focuses(), 1);
        hand(&mut kernel, 0.4, false);
        kernel.tick();
        assert_eq!(control.focuses(), 0);
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

    /// OS のマウスは利き手（既定は右）だけが動かす。もう片方の手は一度押すと操作する手に替わり、その押下と離すは送らない
    /// （前の手の接点は取り消す）。
    #[test]
    fn off_hand_takes_over_with_one_unsent_press() {
        let (mut kernel, panel, control) = setup();
        hand(&mut kernel, 0.0, true);
        let mut left = kernel.subsystems.backend.frame.controllers[0].clone();
        left.hand = Hand::Left;
        left.inputs.clear();
        kernel.subsystems.backend.frame.controllers.push(left);
        kernel.tick();
        assert!(control.touches().iter().all(|t| t.id == 1));
        kernel.subsystems.backend.frame.controllers[1].inputs = vec![("select/click".into(), 1.0)];
        kernel.tick();
        let pressed = |touches: Vec<Touch>| -> Vec<Touch> {
            touches
                .into_iter()
                .filter(|t| t.phase != TouchPhase::Move)
                .collect()
        };
        assert_eq!(
            pressed(control.touches()),
            [Touch {
                id: 1,
                phase: TouchPhase::Cancel,
                x: 0.5,
                y: 0.5
            }]
        );
        let set = |kernel: &mut Kernel<MockBackend>, i: usize, on: bool| {
            kernel.subsystems.backend.frame.controllers[i].inputs =
                vec![("select/click".into(), if on { 1.0 } else { 0.0 })];
            kernel.tick();
        };
        set(&mut kernel, 0, false);
        set(&mut kernel, 1, false);
        assert!(
            pressed(control.touches()).is_empty(),
            "右の離す・左の離すは送らない"
        );
        set(&mut kernel, 1, true);
        assert!(
            control
                .touches()
                .iter()
                .any(|t| t.id == 0 && t.phase == TouchPhase::Down)
        );
        // OS のカーソルも操作している手（左）に出す。
        let cursor = desktop::Cursor {
            image: desktop::Icon {
                width: 1,
                height: 1,
                rgba: [255; 4].into(),
            },
            hotspot: (0, 0),
        };
        control.set_cursor(Some(cursor));
        kernel.tick();
        assert_eq!(
            kernel.panel(panel).os_cursor.as_ref().map(|(h, _)| *h),
            Some(Hand::Left)
        );
    }

    /// 掴むと押していた接点を取り消す。
    #[test]
    fn grabbing_cancels_the_contact() {
        let (mut kernel, _, control) = setup();
        hand(&mut kernel, 0.0, true);
        kernel.tick();
        control.touches();
        // simple_controller の掴み入力は無いため、ここだけ Index の共通アクション変換を通す。
        kernel.subsystems.backend.frame.controllers[0].profile = "valve/index_controller".into();
        kernel.subsystems.backend.frame.controllers[0].inputs =
            vec![("trigger/value".into(), 1.0), ("squeeze/value".into(), 1.0)];
        kernel.tick();
        assert!(
            control
                .touches()
                .iter()
                .any(|t| t.id == 1 && t.phase == TouchPhase::Cancel)
        );
    }

    /// 配信中は OS のカーソルをパネルに渡し、止めたら外す。
    #[test]
    fn os_cursor_follows_the_stream() {
        let (mut kernel, panel, control) = setup();
        let cursor = desktop::Cursor {
            image: desktop::Icon {
                width: 1,
                height: 1,
                rgba: [255; 4].into(),
            },
            hotspot: (0, 0),
        };
        control.set_cursor(Some(cursor.clone()));
        kernel.tick();
        assert_eq!(
            kernel
                .panel(panel)
                .os_cursor
                .as_ref()
                .map(|(h, c)| (*h, &**c)),
            Some((Hand::Right, &cursor))
        );
        kernel.apply(
            panel,
            PanelCommand::Stream(None, desktop::DEFAULT_FRAME_RATE),
        );
        assert_eq!(kernel.panel(panel).os_cursor, None);
    }

    #[test]
    fn meter_averages_each_second_and_keeps_latency_while_the_screen_is_still() {
        let t0 = Instant::now();
        let mut meter = Meter::new(t0);
        meter.add(
            Duration::from_millis(10),
            Duration::from_millis(2),
            Duration::from_millis(5),
        );
        meter.add(
            Duration::from_millis(30),
            Duration::from_millis(4),
            Duration::from_millis(7),
        );
        assert_eq!(
            meter.roll(t0 + Duration::from_millis(500), None, (4, 2)),
            None
        );
        let stats = meter
            .roll(t0 + Duration::from_secs(2), None, (4, 2))
            .unwrap();
        assert_eq!(stats.fps, 1.0);
        assert!((stats.latency_ms - 20.0).abs() < 1e-3);
        assert!((stats.upload_ms - 3.0).abs() < 1e-3);
        assert!((stats.capture_ms - 6.0).abs() < 1e-3);
        assert_eq!((stats.width, stats.height), (4, 2));
        let still = meter
            .roll(t0 + Duration::from_secs(3), Some(stats), (4, 2))
            .unwrap();
        assert_eq!(still.fps, 0.0);
        assert_eq!(still.latency_ms, stats.latency_ms);
    }

    /// `FocusSource` は配信しているウィンドウを前に出すよう配信に頼む。配信していないパネルでは何もしない。
    #[test]
    fn focus_source_asks_the_stream_to_raise_its_window() {
        let (mut kernel, panel, control) = setup();
        kernel.apply(panel, PanelCommand::FocusSource);
        assert_eq!(control.focuses(), 1);
        kernel.apply(
            panel,
            PanelCommand::Stream(None, desktop::DEFAULT_FRAME_RATE),
        );
        kernel.apply(panel, PanelCommand::FocusSource);
        assert_eq!(control.focuses(), 0);
    }

    #[test]
    fn hidden_panel_pauses_its_stream() {
        let (mut kernel, panel, control) = setup();
        assert!(!control.paused());
        kernel.apply(panel, PanelCommand::Hide(true));
        kernel.tick();
        assert!(control.paused());
        kernel.apply(panel, PanelCommand::Hide(false));
        kernel.tick();
        assert!(!control.paused());
    }

    #[test]
    fn minimize_source_asks_the_stream_and_releases_it_on_os_minimize() {
        let (mut kernel, panel, control) = setup();
        kernel.apply(panel, PanelCommand::MinimizeSource);
        assert_eq!(control.minimizes(), 1);
        control.publish(Update {
            status: Status::Minimized,
            frame: None,
        });
        kernel.tick();
        assert!(control.stopped());
        assert_eq!(kernel.status(panel).desktop, Some(Status::Minimized));
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
