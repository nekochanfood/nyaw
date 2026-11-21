//! Wayland の出力（モニター）の列挙。wlroots と portal の両方で、取り込み元の一覧に使う。

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

use super::{Error, Source};
use crate::SourceKind;

impl From<wayland_client::ConnectError> for Error {
    fn from(e: wayland_client::ConnectError) -> Self {
        Self(format!("Wayland: {e}"))
    }
}
impl From<wayland_client::DispatchError> for Error {
    fn from(e: wayland_client::DispatchError) -> Self {
        Self(format!("Wayland: {e}"))
    }
}

/// Wayland の出力。
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Output {
    /// レジストリのグローバル名（[`Source`] の `target`）。
    pub global: u32,
    /// コネクタ名（"DP-1" など）。
    pub connector: String,
    /// 製品名（取れなければ `None`）。
    pub product: Option<String>,
    /// 論理座標系での位置と大きさ（x, y, 幅, 高さ）。
    pub logical: (i32, i32, i32, i32),
}

#[derive(Default)]
struct State {
    outputs: Vec<(wl_output::WlOutput, Output)>,
    xdg: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    screencopy: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_output" => {
                let output = registry.bind(name, version.min(4), qh, name);
                let info = Output {
                    global: name,
                    ..Default::default()
                };
                state.outputs.push((output, info));
            }
            "zxdg_output_manager_v1" => {
                state.xdg = Some(registry.bind(name, version.min(3), qh, ()));
            }
            "zwlr_screencopy_manager_v1" => state.screencopy = true,
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, o)) = state.outputs.iter_mut().find(|(_, o)| o.global == *global) else {
            return;
        };
        match event {
            wl_output::Event::Geometry { x, y, model, .. } => {
                (o.logical.0, o.logical.1) = (x, y);
                let model = model.trim();
                if !model.is_empty() && !model.eq_ignore_ascii_case("unknown") {
                    o.product = Some(model.to_owned());
                }
            }
            wl_output::Event::Mode {
                flags: wayland_client::WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) && o.logical.2 == 0 => {
                (o.logical.2, o.logical.3) = (width, height);
            }
            wl_output::Event::Name { name } => o.connector = name,
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, o)) = state.outputs.iter_mut().find(|(_, o)| o.global == *global) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => (o.logical.0, o.logical.1) = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                (o.logical.2, o.logical.3) = (width, height)
            }
            zxdg_output_v1::Event::Name { name } if o.connector.is_empty() => o.connector = name,
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// 出力の一覧と、wlr-screencopy を提供しているか。
pub(super) fn outputs() -> Result<(Vec<Output>, bool), Error> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    // 1 回目でグローバルを束縛し、2 回目で出力の情報が届く。
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;
    if let Some(xdg) = state.xdg.clone() {
        for (output, info) in &state.outputs {
            xdg.get_xdg_output(output, &qh, info.global);
        }
        queue.roundtrip(&mut state)?;
    }
    let mut outputs: Vec<Output> = state.outputs.into_iter().map(|(_, o)| o).collect();
    for (i, o) in outputs.iter_mut().enumerate() {
        if o.connector.is_empty() {
            o.connector = format!("Display {}", i + 1);
        }
    }
    Ok((outputs, state.screencopy))
}

pub(super) fn has_screencopy() -> bool {
    outputs().is_ok_and(|(_, screencopy)| screencopy)
}

pub(super) fn sources() -> Result<Vec<Source>, Error> {
    let (outputs, _) = outputs()?;
    let names = super::label(
        &outputs
            .iter()
            .map(|o| (o.connector.clone(), o.product.clone()))
            .collect::<Vec<_>>(),
    );
    Ok(outputs
        .into_iter()
        .zip(names)
        .map(|(o, name)| Source {
            name,
            kind: SourceKind::Screen,
            icon: None,
            app: None,
            target: o.global as isize,
        })
        .collect())
}
