//! xdg-desktop-portal（KDE Plasma、GNOME など）: ScreenCast と RemoteDesktop を 1 つのセッションで開き、確認画面で
//! 選ばれたモニターを PipeWire で受け取る。入力は RemoteDesktop の Notify* で送る。
//! セッションはプロセスで 1 つを共有し、全モニターを一度に選ばせる（確認画面はモニターの数ではなく 1 回だけ出る）。
//! 選択は復元用のトークンで保存し、次回からは確認画面を出さずに同じモニターを共有する。

use std::{
    os::fd::OwnedFd,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

use ashpd::desktop::{
    PersistMode, Session,
    remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions},
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType, StartCastOptions,
        Stream as PortalStream,
    },
};
use ashpd::enumflags2::BitFlags;

use super::{
    Button, Error, Frame, ImportFormats, Inject, Pointer, Source, Touch, Wake, pipewire, wayland,
};

impl From<ashpd::Error> for Error {
    fn from(e: ashpd::Error) -> Self {
        match e {
            ashpd::Error::Response(_) => Self(crate::failure::DENIED.into()),
            e => Self(format!("xdg-desktop-portal: {e}")),
        }
    }
}

/// ポータルのセッション。RemoteDesktop が無いポータル（xdg-desktop-portal-wlr など）では ScreenCast だけを開き、
/// 入力は送らない。
enum Kind {
    Remote {
        portal: RemoteDesktop,
        session: Session<RemoteDesktop>,
        /// ポインタの操作が許可された。
        pointer: bool,
    },
    Cast(Session<Screencast>),
}

struct Portal {
    screencast: Screencast,
    kind: Kind,
    streams: Vec<PortalStream>,
}

/// 最後に失敗した要求の時刻とエラー。
type Failure = Option<(Instant, Error)>;

/// 共有中のセッションと、最後に失敗した要求（同時に待っていた他の配信が確認画面を出し直さないため）。
static SHARED: Mutex<(Weak<Portal>, Failure)> = Mutex::new((Weak::new(), None));

/// 共有中のセッション。無ければ開く（確認画面が閉じられるまで待つ）。
fn shared() -> Result<Arc<Portal>, Error> {
    let asked = Instant::now();
    let mut guard = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(portal) = guard.0.upgrade() {
        return Ok(portal);
    }
    // 待っている間に終わった要求が失敗していれば、同じ結果を返す。
    if let Some((at, error)) = &guard.1
        && *at >= asked
    {
        return Err(error.clone());
    }
    match pollster::block_on(Portal::open()) {
        Ok(portal) => {
            let portal = Arc::new(portal);
            *guard = (Arc::downgrade(&portal), None);
            Ok(portal)
        }
        Err(error) => {
            guard.1 = Some((Instant::now(), error.clone()));
            Err(error)
        }
    }
}

/// 確認画面の選択を復元するトークンの保存先。
fn token_path() -> Option<PathBuf> {
    Some(os::data_dir()?.join("screencast-token"))
}

fn load_token() -> Option<String> {
    let token = std::fs::read_to_string(token_path()?).ok()?;
    Some(token.trim().to_owned()).filter(|t| !t.is_empty())
}

fn save_token(token: Option<&str>) {
    let (Some(path), Some(token)) = (token_path(), token) else {
        return;
    };
    if let Err(error) = os::write_file(&path, token) {
        logger::warn!("cannot save the screen sharing choice", error = error);
    }
}

impl Portal {
    async fn open() -> Result<Self, Error> {
        let screencast = Screencast::new().await?;
        let modes = screencast
            .available_cursor_modes()
            .await
            .unwrap_or_default();
        // カーソルは映像に含めない（VR のポインタが代わりに示す）。
        let cursor = if modes.contains(CursorMode::Hidden) {
            CursorMode::Hidden
        } else {
            CursorMode::Embedded
        };
        let sources = SelectSourcesOptions::default()
            .set_sources(BitFlags::from(SourceType::Monitor))
            .set_multiple(true)
            .set_cursor_mode(cursor);
        let token = load_token();
        let remote = match RemoteDesktop::with_connection(screencast.connection().clone()).await {
            Ok(remote) if remote.version() >= 2 => Some(remote),
            Ok(_) | Err(ashpd::Error::PortalNotFound(_)) => None,
            Err(e) => return Err(e.into()),
        };
        let (kind, streams) = match remote {
            Some(portal) => {
                let session = portal.create_session(Default::default()).await?;
                screencast.select_sources(&session, sources).await?;
                let devices = SelectDevicesOptions::default()
                    .set_devices(BitFlags::from(DeviceType::Pointer))
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(token.as_deref());
                portal.select_devices(&session, devices).await?;
                let selected = portal
                    .start(&session, None, Default::default())
                    .await?
                    .response()?;
                save_token(selected.restore_token());
                let pointer = selected.devices().contains(DeviceType::Pointer);
                let streams = selected.streams().to_vec();
                (
                    Kind::Remote {
                        portal,
                        session,
                        pointer,
                    },
                    streams,
                )
            }
            None => {
                logger::warn!("the desktop portal has no RemoteDesktop; screens are view-only");
                let session = screencast.create_session(Default::default()).await?;
                let sources = sources
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(token.as_deref());
                screencast.select_sources(&session, sources).await?;
                let started = screencast
                    .start(&session, None, StartCastOptions::default())
                    .await?
                    .response()?;
                save_token(started.restore_token());
                (Kind::Cast(session), started.streams().to_vec())
            }
        };
        if streams.is_empty() {
            return Err(Error(crate::failure::DENIED.into()));
        }
        logger::info!(
            "screen sharing started",
            streams = streams.len(),
            input = matches!(kind, Kind::Remote { pointer: true, .. })
        );
        Ok(Self {
            screencast,
            kind,
            streams,
        })
    }

    async fn remote(&self) -> Result<OwnedFd, Error> {
        let options = Default::default();
        Ok(match &self.kind {
            Kind::Remote { session, .. } => {
                self.screencast
                    .open_pipe_wire_remote(session, options)
                    .await?
            }
            Kind::Cast(session) => {
                self.screencast
                    .open_pipe_wire_remote(session, options)
                    .await?
            }
        })
    }

    /// `output` を映しているストリーム。位置で照合し、出力もストリームも 1 つだけならそれを使う。
    fn stream(&self, output: &wayland::Output, outputs: usize) -> Option<&PortalStream> {
        let monitors: Vec<&PortalStream> = self
            .streams
            .iter()
            .filter(|s| s.source_type().is_none_or(|t| t == SourceType::Monitor))
            .collect();
        monitors
            .iter()
            .find(|s| s.position() == Some((output.logical.0, output.logical.1)))
            .or_else(|| (monitors.len() == 1 && outputs == 1).then(|| &monitors[0]))
            .copied()
    }
}

impl Drop for Portal {
    fn drop(&mut self) {
        let closed = match &self.kind {
            Kind::Remote { session, .. } => pollster::block_on(session.close()),
            Kind::Cast(session) => pollster::block_on(session.close()),
        };
        if let Err(error) = closed {
            logger::debug!("cannot close the screen sharing session", error = error);
        }
    }
}

pub(super) struct Capture {
    portal: Arc<Portal>,
    node: u32,
    /// ストリームの論理座標系での大きさ（入力の座標の範囲）。
    size: Option<(i32, i32)>,
    stream: pipewire::Stream,
    pointer: Pointer,
}

impl Capture {
    pub fn new(source: Source, wake: Wake, import: Option<ImportFormats>) -> Result<Self, Error> {
        let (outputs, _) = wayland::outputs()?;
        let output = outputs
            .iter()
            .find(|o| o.global as isize == source.target)
            .ok_or_else(|| Error(crate::failure::NO_PIXELS.into()))?;
        let portal = shared()?;
        let stream = portal
            .stream(output, outputs.len())
            .ok_or_else(|| Error(crate::failure::NOT_SHARED.into()))?;
        let node = stream.pipe_wire_node_id();
        let size = stream.size();
        let fd = pollster::block_on(portal.remote())?;
        let stream = pipewire::Stream::open(fd, node, wake, import)?;
        Ok(Self {
            portal,
            node,
            size,
            stream,
            pointer: Pointer::default(),
        })
    }

    fn inject(&self) -> Option<Notify<'_>> {
        let Kind::Remote {
            portal,
            session,
            pointer: true,
        } = &self.portal.kind
        else {
            return None;
        };
        let size = self
            .size
            .or_else(|| self.stream.size().map(|(w, h)| (w as i32, h as i32)))?;
        Some(Notify {
            portal,
            session,
            node: self.node,
            size,
        })
    }
}

impl super::Session for Capture {
    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        self.stream.frame()
    }

    fn closed(&self) -> bool {
        self.stream.closed()
    }

    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        self.button(touch, Button::Left)
    }

    fn button(&mut self, touch: Touch, button: Button) -> Result<(), Error> {
        let mut pointer = std::mem::take(&mut self.pointer);
        let result = match self.inject() {
            Some(mut out) => pointer.send(&mut out, touch, button),
            None => Ok(()),
        };
        self.pointer = pointer;
        result
    }

    fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), Error> {
        let mut pointer = std::mem::take(&mut self.pointer);
        let result = match self.inject() {
            Some(mut out) => pointer.scroll(&mut out, x, y, dx, dy),
            None => Ok(()),
        };
        self.pointer = pointer;
        result
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let mut pointer = std::mem::take(&mut self.pointer);
        if let Some(mut out) = self.inject() {
            pointer.release(&mut out);
        }
    }
}

/// RemoteDesktop の Notify* で送る。座標はストリームの論理座標系（左上原点）。
struct Notify<'a> {
    portal: &'a RemoteDesktop,
    session: &'a Session<RemoteDesktop>,
    node: u32,
    size: (i32, i32),
}

/// evdev のボタン番号（linux/input-event-codes.h）。
fn evdev_button(button: Button) -> i32 {
    match button {
        Button::Left => 0x110,
        Button::Right => 0x111,
        Button::Middle => 0x112,
    }
}

impl Inject for Notify<'_> {
    fn motion(&mut self, x: f32, y: f32) -> Result<(), Error> {
        let (w, h) = self.size;
        let px = f64::from(x.clamp(0.0, 1.0)) * f64::from((w - 1).max(0));
        let py = f64::from(y.clamp(0.0, 1.0)) * f64::from((h - 1).max(0));
        pollster::block_on(self.portal.notify_pointer_motion_absolute(
            self.session,
            self.node,
            px,
            py,
            Default::default(),
        ))?;
        Ok(())
    }

    fn press(&mut self, button: Button, down: bool) -> Result<(), Error> {
        let state = if down {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        pollster::block_on(self.portal.notify_pointer_button(
            self.session,
            evdev_button(button),
            state,
            Default::default(),
        ))?;
        Ok(())
    }

    fn wheel(&mut self, vertical: i32, horizontal: i32) -> Result<(), Error> {
        // ポータルの軸は下・右が正。
        for (axis, steps) in [(Axis::Vertical, -vertical), (Axis::Horizontal, horizontal)] {
            if steps != 0 {
                pollster::block_on(self.portal.notify_pointer_axis_discrete(
                    self.session,
                    axis,
                    steps,
                    Default::default(),
                ))?;
            }
        }
        Ok(())
    }
}
