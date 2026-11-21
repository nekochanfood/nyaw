//! PipeWire の映像ストリームの受信（portal が渡すノード）。ランタイムが DMA-BUF を取り込めればその修飾子で交渉し、
//! 受け取ったバッファを CPU で触らずに渡す。取り込めなければ共有メモリで受け取り、RGBA に並べ替えて渡す。
//! PipeWire のループは専用のスレッドで回し、ランタイムが読み終えたバッファはチャンネルで返却する。

use std::{
    collections::HashMap,
    os::fd::{BorrowedFd, OwnedFd},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Instant,
};

use pipewire as pw;
use pw::spa::{
    self,
    param::{ParamType, format::FormatProperties, video::VideoFormat},
    pod::{ChoiceValue, Object, Pod, Property, PropertyFlags, Value, serialize::PodSerializer},
    utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Id, Rectangle, SpaTypes},
};
use rustix::mm;
use vr::{ExternalImage, Memory};

use super::{Error, Frame, ImportFormats, PixelFormat, Wake};

impl From<pw::Error> for Error {
    fn from(e: pw::Error) -> Self {
        Self(format!("PipeWire: {e}"))
    }
}

const FORMATS: [(PixelFormat, VideoFormat); 4] = [
    (PixelFormat::Bgrx, VideoFormat::BGRx),
    (PixelFormat::Bgra, VideoFormat::BGRA),
    (PixelFormat::Rgbx, VideoFormat::RGBx),
    (PixelFormat::Rgba, VideoFormat::RGBA),
];

fn pixel_format(format: VideoFormat) -> Option<PixelFormat> {
    FORMATS.iter().find(|(_, v)| *v == format).map(|(p, _)| *p)
}

enum Message {
    /// ランタイムが読み終えたバッファ（`pw_buffer` のアドレス）を返却する。
    Release(usize),
    Stop,
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Option<Frame>>,
    size: Mutex<Option<(u32, u32)>>,
    closed: AtomicBool,
}

pub(super) struct Stream {
    shared: Arc<Shared>,
    sender: pw::channel::Sender<Message>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Stream {
    /// `fd`（portal の PipeWire の接続）でノード `node` を受信する。
    pub fn open(
        fd: OwnedFd,
        node: u32,
        wake: Wake,
        import: Option<ImportFormats>,
    ) -> Result<Self, Error> {
        let shared = Arc::new(Shared::default());
        let (sender, receiver) = pw::channel::channel();
        let (started, start) = std::sync::mpsc::sync_channel(1);
        let thread = {
            let (shared, sender) = (shared.clone(), sender.clone());
            thread::Builder::new()
                .name("desktop-pipewire".into())
                .spawn(move || {
                    let result = run(
                        fd,
                        node,
                        wake.clone(),
                        import,
                        &shared,
                        sender,
                        receiver,
                        &started,
                    );
                    if let Err(error) = &result {
                        logger::warn!("PipeWire stream stopped", error = error);
                        let _ = started.try_send(result);
                    }
                    shared.closed.store(true, Ordering::Release);
                    wake.wake();
                })
                .map_err(|e| Error(e.to_string()))?
        };
        let stream = Self {
            shared,
            sender,
            thread: Some(thread),
        };
        start
            .recv()
            .map_err(|_| Error("PipeWire stream did not start".into()))??;
        Ok(stream)
    }

    pub fn frame(&mut self) -> Result<Option<Frame>, Error> {
        Ok(self.shared.latest.lock().unwrap().take())
    }

    pub fn closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }

    /// 交渉した画像の大きさ（画素）。
    pub fn size(&self) -> Option<(u32, u32)> {
        *self.shared.size.lock().unwrap()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// 交渉が済んだ形式。
#[derive(Debug, Clone, Copy)]
struct Negotiated {
    format: PixelFormat,
    width: u32,
    height: u32,
    modifier: Option<u64>,
}

/// 自分で写像した memfd（PipeWire の写像はストリームの終了で外れるが、ランタイムは読み終えるまで使う）。
struct Mapping {
    ptr: *mut u8,
    len: usize,
}
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}
impl Drop for Mapping {
    fn drop(&mut self) {
        let _ = unsafe { mm::munmap(self.ptr.cast(), self.len) };
    }
}

struct Data {
    import: Option<ImportFormats>,
    format: Option<Negotiated>,
    /// memfd のバッファ（`pw_buffer` のアドレス）ごとの写像。取り込める境界に揃っていなければ `None`。
    mappings: HashMap<usize, Option<Arc<Mapping>>>,
}

fn serialize(value: Value) -> Vec<u8> {
    PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &value)
        .expect("PipeWire parameters serialize into memory")
        .0
        .into_inner()
}

/// 受け取れる映像の形式。`modifiers` があれば DMA-BUF で、無ければ共有メモリで受け取る形式。
fn format_pod(format: VideoFormat, modifiers: Option<&[u64]>) -> Vec<u8> {
    let mut properties = vec![
        Property::new(
            FormatProperties::MediaType.as_raw(),
            Value::Id(Id(spa::param::format::MediaType::Video.as_raw())),
        ),
        Property::new(
            FormatProperties::MediaSubtype.as_raw(),
            Value::Id(Id(spa::param::format::MediaSubtype::Raw.as_raw())),
        ),
        Property::new(
            FormatProperties::VideoFormat.as_raw(),
            Value::Id(Id(format.as_raw())),
        ),
        Property::new(
            FormatProperties::VideoSize.as_raw(),
            Value::Choice(ChoiceValue::Rectangle(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Range {
                    default: Rectangle {
                        width: 1920,
                        height: 1080,
                    },
                    min: Rectangle {
                        width: 1,
                        height: 1,
                    },
                    max: Rectangle {
                        width: 16384,
                        height: 16384,
                    },
                },
            ))),
        ),
        Property::new(
            FormatProperties::VideoFramerate.as_raw(),
            Value::Choice(ChoiceValue::Fraction(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Range {
                    default: Fraction { num: 60, denom: 1 },
                    min: Fraction { num: 0, denom: 1 },
                    max: Fraction {
                        num: 1000,
                        denom: 1,
                    },
                },
            ))),
        ),
    ];
    if let Some(modifiers) = modifiers {
        let value = match modifiers {
            [only] => Value::Long(*only as i64),
            _ => Value::Choice(ChoiceValue::Long(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default: modifiers[0] as i64,
                    alternatives: modifiers.iter().map(|m| *m as i64).collect(),
                },
            ))),
        };
        properties.push(Property {
            key: FormatProperties::VideoModifier.as_raw(),
            flags: if modifiers.len() > 1 {
                PropertyFlags::MANDATORY | PropertyFlags::DONT_FIXATE
            } else {
                PropertyFlags::MANDATORY
            },
            value,
        });
    }
    serialize(Value::Object(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties,
    }))
}

/// 受け取れる形式の一覧。DMA-BUF を優先する。
fn format_pods(import: Option<&ImportFormats>) -> Vec<Vec<u8>> {
    let dmabuf = FORMATS.iter().filter_map(|&(pixel, format)| {
        let modifiers = import?.modifiers(pixel);
        (!modifiers.is_empty()).then(|| format_pod(format, Some(modifiers)))
    });
    let shm = FORMATS.iter().map(|&(_, format)| format_pod(format, None));
    dmabuf.chain(shm).collect()
}

/// バッファの種類と数。DMA-BUF を交渉した場合は DMA-BUF のみ、それ以外は memfd か CPU のメモリ。
fn buffers_pod(dmabuf: bool) -> Vec<u8> {
    let types = if dmabuf {
        1 << spa::buffer::DataType::DmaBuf.as_raw()
    } else {
        (1 << spa::buffer::DataType::MemFd.as_raw()) | (1 << spa::buffer::DataType::MemPtr.as_raw())
    };
    serialize(Value::Object(Object {
        type_: SpaTypes::ObjectParamBuffers.as_raw(),
        id: ParamType::Buffers.as_raw(),
        properties: vec![
            Property::new(
                spa::sys::SPA_PARAM_BUFFERS_buffers,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: 4,
                        min: 2,
                        max: 16,
                    },
                ))),
            ),
            Property::new(spa::sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
            Property::new(
                spa::sys::SPA_PARAM_BUFFERS_dataType,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags {
                        default: types,
                        flags: vec![],
                    },
                ))),
            ),
        ],
    }))
}

/// 修飾子がまだ一つに決まっていない（DONT_FIXATE の選択肢のまま）なら、その選択肢の先頭。
fn unfixed_modifier(param: &Pod) -> Option<i64> {
    let (_, Value::Object(object)) =
        spa::pod::deserialize::PodDeserializer::deserialize_any_from(param.as_bytes()).ok()?
    else {
        return None;
    };
    let property = object
        .properties
        .iter()
        .find(|p| p.key == FormatProperties::VideoModifier.as_raw())?;
    if !property.flags.contains(PropertyFlags::DONT_FIXATE) {
        return None;
    }
    match &property.value {
        Value::Choice(ChoiceValue::Long(Choice(_, ChoiceEnum::Enum { default, .. }))) => {
            Some(*default)
        }
        Value::Long(only) => Some(*only),
        _ => None,
    }
}

/// `pod`（交渉の結果）の修飾子を `modifier` に決めた形式。
fn fixate(pod: &Pod, modifier: i64) -> Option<Vec<u8>> {
    let (_, Value::Object(mut object)) =
        spa::pod::deserialize::PodDeserializer::deserialize_any_from(pod.as_bytes()).ok()?
    else {
        return None;
    };
    object.id = ParamType::EnumFormat.as_raw();
    for property in &mut object.properties {
        if property.key == FormatProperties::VideoModifier.as_raw() {
            property.flags = PropertyFlags::MANDATORY;
            property.value = Value::Long(modifier);
        }
    }
    Some(serialize(Value::Object(object)))
}

#[allow(clippy::too_many_arguments)]
fn run(
    fd: OwnedFd,
    node: u32,
    wake: Wake,
    import: Option<ImportFormats>,
    shared: &Arc<Shared>,
    sender: pw::channel::Sender<Message>,
    receiver: pw::channel::Receiver<Message>,
    started: &std::sync::mpsc::SyncSender<Result<(), Error>>,
) -> Result<(), Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd_rc(fd, None)?;
    let stream = pw::stream::StreamRc::new(
        core,
        "nyaw-desktop",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;
    let data = Data {
        import: import.clone(),
        format: None,
        mappings: HashMap::new(),
    };
    let quit = mainloop.clone();
    let failed = shared.clone();
    let failing = wake.clone();
    let delivered = shared.clone();
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(move |_, _, _, state| {
            if matches!(
                state,
                pw::stream::StreamState::Error(_) | pw::stream::StreamState::Unconnected
            ) {
                if let pw::stream::StreamState::Error(e) = &state {
                    logger::warn!("PipeWire stream failed", error = e);
                }
                failed.closed.store(true, Ordering::Release);
                failing.wake();
                quit.quit();
            }
        })
        .param_changed(move |stream, data, id, param| {
            let Some(param) = param else { return };
            if id != ParamType::Format.as_raw() {
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::new();
            if info.parse(param).is_err() {
                return;
            }
            let Some(format) = pixel_format(info.format()) else {
                return;
            };
            if let Some(modifier) = unfixed_modifier(param) {
                // 修飾子の候補が残っている。一つに決めて交渉し直す。
                if let Some(fixed) = fixate(param, modifier) {
                    let mut params = [Pod::from_bytes(&fixed).unwrap()];
                    let _ = stream.update_params(&mut params);
                }
                return;
            }
            let has_modifier = info
                .flags()
                .contains(spa::param::video::VideoFlags::MODIFIER);
            let negotiated = Negotiated {
                format,
                width: info.size().width,
                height: info.size().height,
                modifier: has_modifier.then(|| info.modifier()),
            };
            logger::info!(
                "PipeWire stream negotiated",
                format = ?negotiated.format,
                width = negotiated.width,
                height = negotiated.height,
                modifier = ?negotiated.modifier
            );
            data.format = Some(negotiated);
            *delivered.size.lock().unwrap() = Some((negotiated.width, negotiated.height));
            let buffers = buffers_pod(has_modifier);
            let mut params = [Pod::from_bytes(&buffers).unwrap()];
            let _ = stream.update_params(&mut params);
        })
        .add_buffer(|_, data, buffer| {
            // memfd は自分でも写像し、ランタイムが読み終えるまで外さない。
            let key = buffer as usize;
            let mapping = unsafe { map_memfd(buffer, data.import.as_ref()) };
            data.mappings.insert(key, mapping);
        })
        .remove_buffer(|_, data, buffer| {
            data.mappings.remove(&(buffer as usize));
        })
        .process({
            let shared = shared.clone();
            let wake = wake.clone();
            let sender = sender.clone();
            move |stream, data| unsafe { process(stream, data, &shared, &wake, &sender) }
        })
        .register()?;
    let mut params: Vec<Vec<u8>> = format_pods(import.as_ref());
    let mut pods: Vec<&Pod> = params
        .iter_mut()
        .map(|p| Pod::from_bytes(p).unwrap())
        .collect();
    stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut pods,
    )?;
    let _ = started.try_send(Ok(()));
    let releasing = stream.clone();
    let stopping = mainloop.clone();
    let _receiver = receiver.attach(mainloop.loop_(), move |message| match message {
        Message::Release(buffer) => unsafe {
            releasing.queue_raw_buffer(buffer as *mut pw::sys::pw_buffer)
        },
        Message::Stop => stopping.quit(),
    });
    mainloop.run();
    // 返却を待っているバッファはここで捨てる（取り込んだ側は写像と fd を自分で持っている）。
    shared.latest.lock().unwrap().take();
    let _ = stream.disconnect();
    Ok(())
}

/// memfd のバッファを写像する。ランタイムが取り込める境界に揃わなければ `None`（CPU で並べ替える）。
unsafe fn map_memfd(
    buffer: *mut pw::sys::pw_buffer,
    import: Option<&ImportFormats>,
) -> Option<Arc<Mapping>> {
    let import = import?;
    let spa_buffer = unsafe { (*buffer).buffer.as_ref()? };
    if spa_buffer.n_datas < 1 {
        return None;
    }
    let data = unsafe { &*spa_buffer.datas };
    if data.type_ != spa::buffer::DataType::MemFd.as_raw() || data.fd < 0 || data.mapoffset != 0 {
        return None;
    }
    let align = import.host_alignment?.max(rustix::param::page_size());
    let len = (data.maxsize as usize).div_ceil(align) * align;
    let fd = unsafe { BorrowedFd::borrow_raw(data.fd as i32) };
    let ptr = unsafe {
        mm::mmap(
            std::ptr::null_mut(),
            len,
            mm::ProtFlags::READ,
            mm::MapFlags::SHARED,
            fd,
            0,
        )
    }
    .ok()? as *mut u8;
    let mapping = Arc::new(Mapping { ptr, len });
    import.host(ptr, len).then_some(mapping)
}

/// 届いたバッファを最新の画像にする。ランタイムへ渡すバッファは読み終えるまで返却しない。
unsafe fn process(
    stream: &pw::stream::Stream,
    data: &mut Data,
    shared: &Shared,
    wake: &Wake,
    sender: &pw::channel::Sender<Message>,
) {
    let raw = unsafe { stream.dequeue_raw_buffer() };
    if raw.is_null() {
        return;
    }
    let requeue = || unsafe { stream.queue_raw_buffer(raw) };
    let (Some(format), Some(spa_buffer)) = (data.format, unsafe { (*raw).buffer.as_ref() }) else {
        return requeue();
    };
    if spa_buffer.n_datas < 1 {
        return requeue();
    }
    let d = unsafe { &*spa_buffer.datas };
    let Some(chunk) = (unsafe { d.chunk.as_ref() }) else {
        return requeue();
    };
    // カーソルだけの更新や壊れたバッファは画像を持たない。
    if chunk.size == 0 || chunk.flags & spa::sys::SPA_CHUNK_FLAG_CORRUPTED as i32 != 0 {
        return requeue();
    }
    let stride = if chunk.stride > 0 {
        chunk.stride as u32
    } else {
        format.width * 4
    };
    let (width, height) = (format.width, format.height);
    let lend = |memory: Memory| {
        let sender = sender.clone();
        let release = raw as usize;
        Frame::external(ExternalImage::new(
            width,
            height,
            format.format,
            memory,
            move || {
                let _ = sender.send(Message::Release(release));
            },
        ))
    };
    let frame = if d.type_ == spa::buffer::DataType::DmaBuf.as_raw() {
        let fd = unsafe { BorrowedFd::borrow_raw(d.fd as i32) }.try_clone_to_owned();
        match (fd, format.modifier) {
            (Ok(fd), Some(modifier)) => Ok(lend(Memory::DmaBuf {
                fd,
                modifier,
                offset: chunk.offset,
                stride,
            })),
            (Err(e), _) => Err(Error(format!("dup DMA-BUF: {e}"))),
            (_, None) => Err(Error("DMA-BUF without a modifier".into())),
        }
    } else if let Some(Some(mapping)) = data.mappings.get(&(raw as usize)) {
        let mapping = mapping.clone();
        let (ptr, len) = (mapping.ptr as *const u8, mapping.len);
        let sender = sender.clone();
        let release = raw as usize;
        Ok(Frame::external(ExternalImage::new(
            width,
            height,
            format.format,
            Memory::Host {
                ptr,
                len,
                offset: chunk.offset as usize,
                stride,
            },
            move || {
                drop(mapping);
                let _ = sender.send(Message::Release(release));
            },
        )))
    } else if !d.data.is_null() {
        let len = (d.maxsize as usize).min((chunk.offset + chunk.size) as usize);
        let bytes = unsafe { std::slice::from_raw_parts(d.data as *const u8, len) };
        let bytes = &bytes[(chunk.offset as usize).min(len)..];
        let frame = Frame::from_pixels(width, height, stride as usize, bytes, format.format);
        requeue();
        frame
    } else {
        requeue();
        return;
    };
    match frame {
        Ok(mut frame) => {
            frame.captured = Instant::now();
            // 取りに来なかった前の画像は破棄して、そのバッファを返却する。
            shared.latest.lock().unwrap().replace(frame);
            wake.wake();
        }
        Err(error) => logger::warn!("cannot read a PipeWire buffer", error = error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifier_property(pod: &[u8]) -> Option<Property> {
        let (_, Value::Object(object)) =
            spa::pod::deserialize::PodDeserializer::deserialize_any_from(pod).unwrap()
        else {
            panic!("not an object");
        };
        object
            .properties
            .into_iter()
            .find(|p| p.key == FormatProperties::VideoModifier.as_raw())
    }

    /// DMA-BUF の形式を先に並べ、修飾子が複数あれば選択肢のまま送って相手に絞らせる。
    #[test]
    fn offers_dmabuf_formats_first_with_their_modifiers() {
        let import = ImportFormats {
            dmabuf: vec![
                (PixelFormat::Bgrx, vec![0, 7]),
                (PixelFormat::Rgba, vec![3]),
            ],
            host_alignment: Some(4096),
        };
        let pods = format_pods(Some(&import));
        assert_eq!(pods.len(), 2 + FORMATS.len());
        let first = modifier_property(&pods[0]).unwrap();
        assert!(first.flags.contains(PropertyFlags::DONT_FIXATE));
        let second = modifier_property(&pods[1]).unwrap();
        assert_eq!(second.value, Value::Long(3));
        assert!(pods[2..].iter().all(|p| modifier_property(p).is_none()));
        assert_eq!(format_pods(None).len(), FORMATS.len());
    }

    /// 候補の残った交渉結果は、先頭の修飾子に決めて送り直す。
    #[test]
    fn fixates_the_first_offered_modifier() {
        let offered = format_pod(VideoFormat::BGRx, Some(&[5, 9]));
        let pod = Pod::from_bytes(&offered).unwrap();
        assert_eq!(unfixed_modifier(pod), Some(5));
        let fixed = fixate(pod, 5).unwrap();
        let property = modifier_property(&fixed).unwrap();
        assert_eq!(property.value, Value::Long(5));
        assert!(!property.flags.contains(PropertyFlags::DONT_FIXATE));
        assert_eq!(unfixed_modifier(Pod::from_bytes(&fixed).unwrap()), None);
    }
}
