use super::{Error, Frame, Session, Source, SourceKind, Touch, TouchPhase};
use ashpd::desktop::{
    Session as PortalSession,
    remote_desktop::{DeviceType, RemoteDesktop, SelectDevicesOptions},
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use futures_lite::future::{self, block_on};
use pipewire::{self as pw, properties::properties, spa};
use std::{
    cell::RefCell,
    io::Cursor,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

impl From<ashpd::Error> for Error {
    fn from(e: ashpd::Error) -> Self {
        Self(e.to_string())
    }
}
impl From<pw::Error> for Error {
    fn from(e: pw::Error) -> Self {
        Self(e.to_string())
    }
}

pub(super) fn sources() -> Result<Vec<Source>, Error> {
    block_on(async {
        let cast = Screencast::new().await?;
        let types = cast.available_source_types().await?;
        let mut sources = Vec::new();
        // Wayland の一覧は取得せず、Portal の選択ダイアログに対象の選択と許可を任せる。
        for (native, kind, name) in [
            (SourceType::Monitor, SourceKind::Screen, "Select a screen…"),
            (SourceType::Window, SourceKind::Window, "Select a window…"),
        ] {
            if types.contains(native) {
                sources.push(Source {
                    name: name.into(),
                    kind,
                    icon: None,
                    app: None,
                    target: 0,
                });
            }
        }
        Ok(sources)
    })
}

struct Portal {
    remote: RemoteDesktop,
    session: PortalSession<RemoteDesktop>,
    contacts: [bool; 2],
}
impl Portal {
    async fn new() -> Result<Self, Error> {
        let remote = RemoteDesktop::new().await?;
        let session = remote.create_session(Default::default()).await?;
        Ok(Self {
            remote,
            session,
            contacts: [false; 2],
        })
    }
}
impl Drop for Portal {
    fn drop(&mut self) {
        block_on(async {
            for (id, active) in self.contacts.iter().enumerate() {
                if *active {
                    let _ = self
                        .remote
                        .notify_touch_up(&self.session, id as u32, Default::default())
                        .await;
                }
            }
            let _ = self.session.close().await;
        });
    }
}

#[derive(Default)]
struct Video {
    format: spa::param::video::VideoInfoRaw,
    frame: Option<Frame>,
    error: Option<Error>,
    closed: bool,
}

pub(super) struct Capture {
    // リスナーをストリームより先に、ストリームをループより先に解放する。
    _listener: pw::stream::StreamListener<Rc<RefCell<Video>>>,
    _stream: pw::stream::StreamRc,
    main_loop: pw::main_loop::MainLoopRc,
    video: Rc<RefCell<Video>>,
    portal: Portal,
    node: u32,
    logical_size: Option<(i32, i32)>,
}

impl Capture {
    pub fn new(source: Source, stop: Arc<AtomicBool>) -> Result<Self, Error> {
        let portal = block_on(Portal::new())?;
        let (fd, node, logical_size) = block_on(future::race(
            async {
                let cast = Screencast::new().await?;
                portal
                    .remote
                    .select_devices(
                        &portal.session,
                        SelectDevicesOptions::default()
                            .set_devices(Some(DeviceType::Touchscreen.into())),
                    )
                    .await?
                    .response()?;
                let source = match source.kind {
                    SourceKind::Screen => SourceType::Monitor,
                    SourceKind::Window => SourceType::Window,
                };
                cast.select_sources(
                    &portal.session,
                    SelectSourcesOptions::default()
                        .set_sources(Some(source.into()))
                        .set_multiple(false)
                        .set_cursor_mode(CursorMode::Embedded),
                )
                .await?
                .response()?;
                let response = portal
                    .remote
                    .start(&portal.session, None, Default::default())
                    .await?
                    .response()?;
                if !response.devices().contains(DeviceType::Touchscreen) {
                    return Err(Error(
                        "desktop touch control was not granted by the portal".into(),
                    ));
                }
                let stream = response
                    .streams()
                    .first()
                    .ok_or_else(|| Error("portal returned no capture stream".into()))?;
                let fd = cast
                    .open_pipe_wire_remote(&portal.session, Default::default())
                    .await?;
                Ok((fd, stream.pipe_wire_node_id(), stream.size()))
            },
            async {
                while !stop.load(Ordering::Acquire) {
                    async_io::Timer::after(Duration::from_millis(20)).await;
                }
                Err(Error(
                    "desktop capture stopped before selection completed".into(),
                ))
            },
        ))?;
        pw::init();
        let main_loop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&main_loop, None)?;
        let core = context.connect_fd_rc(fd, None)?;
        let stream = pw::stream::StreamRc::new(
            core,
            "nyaw desktop",
            properties! {
                *pw::keys::MEDIA_TYPE=>"Video",
                *pw::keys::MEDIA_CATEGORY=>"Capture",
                *pw::keys::MEDIA_ROLE=>"Screen",
            },
        )?;
        let video = Rc::new(RefCell::new(Video::default()));
        let listener = stream
            .add_local_listener_with_user_data(video.clone())
            .state_changed(|_, video, old, new| {
                let mut video = video.borrow_mut();
                match new {
                    pw::stream::StreamState::Error(error) => {
                        video.error = Some(Error(error.to_string()))
                    }
                    pw::stream::StreamState::Unconnected
                        if old != pw::stream::StreamState::Unconnected =>
                    {
                        video.closed = true
                    }
                    _ => (),
                }
            })
            .param_changed(|_, video, id, param| {
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                if let Some(param) = param {
                    let mut video = video.borrow_mut();
                    if let Err(error) = video.format.parse(param) {
                        video.error =
                            Some(Error(format!("invalid PipeWire video format: {error:?}")));
                    }
                }
            })
            .process(|stream, video| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let mut video = video.borrow_mut();
                let Some(data) = buffer.datas_mut().first_mut() else {
                    return;
                };
                let (offset, size, stride) = (
                    data.chunk().offset(),
                    data.chunk().size(),
                    data.chunk().stride(),
                );
                if size == 0 {
                    return;
                }
                let Some(bytes) = data.data() else {
                    video.error = Some(Error(
                        "PipeWire did not provide a mapped video buffer".into(),
                    ));
                    return;
                };
                match read_frame(&video.format, offset, size, stride, bytes) {
                    Ok(frame) => video.frame = Some(frame),
                    Err(error) => video.error = Some(error),
                }
            })
            .register()?;
        // CPU にマップできる 32bit の形式だけを提示する。DMA-BUF は要求しない。
        let object = spa::pod::object!(
            spa::utils::SpaTypes::ObjectParamFormat,
            spa::param::ParamType::EnumFormat,
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaType,
                Id,
                spa::param::format::MediaType::Video
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaSubtype,
                Id,
                spa::param::format::MediaSubtype::Raw
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::BGRA
            )
        );
        let params = spa::pod::serialize::PodSerializer::serialize(
            Cursor::new(Vec::new()),
            &spa::pod::Value::Object(object),
        )
        .map_err(|e| Error(format!("cannot serialize PipeWire video format: {e:?}")))?
        .0
        .into_inner();
        let param = spa::pod::Pod::from_bytes(&params)
            .ok_or_else(|| Error("invalid PipeWire format pod".into()))?;
        stream.connect(
            spa::utils::Direction::Input,
            Some(node),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut [param],
        )?;
        Ok(Self {
            _listener: listener,
            _stream: stream,
            main_loop,
            video,
            portal,
            node,
            logical_size,
        })
    }
}

fn read_frame(
    format: &spa::param::video::VideoInfoRaw,
    offset: u32,
    size: u32,
    stride: i32,
    bytes: &[u8],
) -> Result<Frame, Error> {
    let end = (offset as usize)
        .checked_add(size as usize)
        .ok_or_else(|| Error("PipeWire buffer overflow".into()))?;
    let bytes = bytes
        .get(offset as usize..end)
        .ok_or_else(|| Error("PipeWire chunk is outside the mapped buffer".into()))?;
    let stride =
        usize::try_from(stride).map_err(|_| Error("PipeWire video stride is negative".into()))?;
    let kind = format.format();
    if kind != spa::param::video::VideoFormat::BGRx && kind != spa::param::video::VideoFormat::BGRA
    {
        return Err(Error(
            "PipeWire selected an unsupported video format".into(),
        ));
    }
    let size = format.size();
    let mut frame = Frame::from_bgra(size.width, size.height, stride, bytes)?;
    if kind == spa::param::video::VideoFormat::BGRx {
        for pixel in frame.rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    } else if !format
        .flags()
        .contains(spa::param::video::VideoFlags::PREMULTIPLIED_ALPHA)
    {
        for pixel in frame.rgba.as_chunks_mut::<4>().0 {
            let alpha = pixel[3] as u16;
            for color in &mut pixel[..3] {
                *color = ((*color as u16 * alpha + 127) / 255) as u8;
            }
        }
    }
    Ok(frame)
}

impl Session for Capture {
    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        if self.main_loop.loop_().iterate(pw::loop_::Timeout::None) < 0 {
            return Err(Error("PipeWire loop failed".into()));
        }
        let mut video = self.video.borrow_mut();
        if let Some(error) = video.error.take() {
            return Err(error);
        }
        Ok(video.frame.take())
    }

    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        let id = touch.id as usize;
        let active = self.portal.contacts[id];
        if touch.phase == TouchPhase::Down && active {
            return Err(Error("duplicate touch down".into()));
        }
        if touch.phase != TouchPhase::Down && !active {
            return Ok(());
        }
        let size = self.video.borrow().format.size();
        let (width, height) = self
            .logical_size
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or((size.width as i32, size.height as i32));
        if width <= 0 || height <= 0 {
            return Err(Error("desktop touch needs a negotiated video size".into()));
        }
        let (x, y) = (
            (width - 1) as f64 * touch.x as f64,
            (height - 1) as f64 * touch.y as f64,
        );
        let portal = &self.portal;
        block_on(async {
            match touch.phase {
                TouchPhase::Down => {
                    portal
                        .remote
                        .notify_touch_down(
                            &portal.session,
                            self.node,
                            touch.id,
                            x,
                            y,
                            Default::default(),
                        )
                        .await
                }
                TouchPhase::Move => {
                    portal
                        .remote
                        .notify_touch_motion(
                            &portal.session,
                            self.node,
                            touch.id,
                            x,
                            y,
                            Default::default(),
                        )
                        .await
                }
                TouchPhase::Up | TouchPhase::Cancel => {
                    portal
                        .remote
                        .notify_touch_up(&portal.session, touch.id, Default::default())
                        .await
                }
            }
        })?;
        self.portal.contacts[id] = matches!(touch.phase, TouchPhase::Down | TouchPhase::Move);
        Ok(())
    }

    fn closed(&self) -> bool {
        self.video.borrow().closed
    }
}

/// デスクトップエントリ（`applications/*.desktop`）のアプリを並べる。隠すもの（`NoDisplay`・`Hidden`）は除く。
// ponytail: アイコンは読まない（テーマの探し方が要る）。要れば media の desktop_icon と同じ探し方を足す。
pub(super) fn installed_apps() -> Result<Vec<super::InstalledApp>, Error> {
    let home = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
        std::env::var("HOME")
            .ok()
            .map(|h| format!("{h}/.local/share"))
    });
    let dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut seen = std::collections::HashSet::new();
    let mut apps = Vec::new();
    for root in home.into_iter().chain(dirs.split(':').map(str::to_owned)) {
        let Ok(entries) = std::fs::read_dir(std::path::Path::new(&root).join("applications"))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // 同じ名前のエントリは、先に見つけた（利用者の）ものを使う。
            if path.extension().is_none_or(|e| e != "desktop") || !seen.insert(entry.file_name()) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| desktop_entry(&t))
            {
                apps.push(app);
            }
        }
    }
    Ok(apps)
}

/// デスクトップエントリの `[Desktop Entry]` の項目から、起動できるアプリを作る。
fn desktop_entry(text: &str) -> Option<super::InstalledApp> {
    let mut fields = std::collections::HashMap::new();
    let mut main = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
        } else if main && let Some((k, v)) = line.split_once('=') {
            fields.entry(k.trim()).or_insert(v.trim());
        }
    }
    let hidden = |k| fields.get(k).is_some_and(|v| *v == "true");
    if fields.get("Type") != Some(&"Application") || hidden("NoDisplay") || hidden("Hidden") {
        return None;
    }
    // %f・%U などの引数の置き場は、引数を渡さないので消す。
    let exec: Vec<&str> = fields
        .get("Exec")?
        .split_whitespace()
        .filter(|w| !(w.len() == 2 && w.starts_with('%')))
        .collect();
    Some(super::InstalledApp {
        name: fields.get("Name")?.to_string(),
        id: exec.join(" "),
        exe: None,
        icon: None,
    })
}

/// デスクトップエントリの Exec でアプリを起動する（待たない）。
pub(super) fn launch(exec: &str) -> Result<(), Error> {
    std::process::Command::new("sh")
        .args(["-c", exec])
        .spawn()
        .map(drop)
        .map_err(|e| Error(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 起動できるアプリだけを読み、Exec の引数の置き場を消す。
    #[test]
    fn reads_launchable_desktop_entries() {
        let app = desktop_entry(
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n[Desktop Action new]\nName=New\n",
        )
        .unwrap();
        assert_eq!((app.name.as_str(), app.id.as_str()), ("Firefox", "firefox"));
        assert!(
            desktop_entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n")
                .is_none()
        );
        assert!(desktop_entry("[Desktop Entry]\nType=Link\nName=X\nURL=y\n").is_none());
    }
    #[test]
    fn validates_pipewire_chunk_and_makes_bgrx_opaque() {
        let mut format = spa::param::video::VideoInfoRaw::new();
        format.set_format(spa::param::video::VideoFormat::BGRx);
        format.set_size(spa::utils::Rectangle {
            width: 1,
            height: 1,
        });
        assert_eq!(
            read_frame(&format, 2, 4, 4, &[0, 0, 10, 20, 30, 0])
                .unwrap()
                .rgba,
            [30, 20, 10, 255]
        );
        assert!(read_frame(&format, 2, 8, 4, &[0; 6]).is_err());
        assert!(read_frame(&format, 0, 4, -4, &[0; 4]).is_err());
        format.set_format(spa::param::video::VideoFormat::BGRA);
        assert_eq!(
            read_frame(&format, 0, 4, 4, &[10, 20, 30, 40])
                .unwrap()
                .rgba,
            [5, 3, 2, 40]
        );
        format.set_flags(spa::param::video::VideoFlags::PREMULTIPLIED_ALPHA);
        assert_eq!(
            read_frame(&format, 0, 4, 4, &[10, 20, 30, 40])
                .unwrap()
                .rgba,
            [30, 20, 10, 40]
        );
    }
}
