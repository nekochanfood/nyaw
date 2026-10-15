//! Components: 役割（[`Theme`]）と Constants だけで組む部品。生の色は書かない。
//! 部品は `Stateful<Div>` を返すので、呼び出し側が `on_click` などを足す。

use gpui::{App, Div, SharedString, Stateful, Window, div, prelude::*, px, rgba};

use crate::{Theme, font_size, radius, spacing};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Primary,
    Danger,
}

/// 押せるボタン。`id` はそのまま `debug_selector` にもなる。
pub fn button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    variant: Variant,
) -> Stateful<Div> {
    let (bg, hover, active, fg) = match variant {
        Variant::Primary => (
            theme.primary,
            theme.primary_hover,
            theme.primary_active,
            theme.on_primary,
        ),
        Variant::Danger => (
            theme.danger,
            theme.danger_hover,
            theme.danger_active,
            theme.on_danger,
        ),
    };
    div()
        .id(id)
        .debug_selector(move || id.into())
        .px(px(spacing::LG))
        .py(px(spacing::SM))
        .rounded(px(radius::MD))
        .text_size(px(font_size::MD))
        .text_color(rgba(fg))
        .bg(rgba(bg))
        .hover(move |s| s.bg(rgba(hover)))
        .active(move |s| s.bg(rgba(active)))
        .child(label.into())
}

/// オン / オフのスイッチ。押したときの処理は呼び出し側が `on_click` で足す。
pub fn switch(theme: &Theme, id: &'static str, on: bool) -> Stateful<Div> {
    let (w, h, pad) = (44.0, 24.0, 2.0);
    let knob = h - pad * 2.0;
    let mut track = div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .w(px(w))
        .h(px(h))
        .px(px(pad))
        .rounded(px(radius::FULL))
        .bg(rgba(if on { theme.primary } else { theme.track }));
    if on {
        track = track.justify_end();
    }
    track.child(
        div()
            .size(px(knob))
            .rounded(px(radius::FULL))
            .bg(rgba(if on { theme.on_primary } else { theme.surface })),
    )
}

/// `steps` 個の段に分けたスライダー（`value` は 0..=1）。段を押すとその段の値で `on_change` が呼ばれる。
/// 段は `{id}-{i}` で指せる。
// ponytail: ドラッグで連続に動かすのではなく段を押す方式。連続値が要るならポインタの位置から値を出す版にする。
pub fn slider(
    theme: &Theme,
    id: &'static str,
    value: f32,
    steps: usize,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    assert!(steps >= 2, "スライダーは 2 段以上");
    let on_change = std::rc::Rc::new(on_change);
    let filled = (value.clamp(0.0, 1.0) * (steps - 1) as f32).round() as usize;
    let mut row = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(spacing::XS))
        .h(px(24.0));
    for i in 0..steps {
        let v = i as f32 / (steps - 1) as f32;
        let on_change = on_change.clone();
        let name = format!("{id}-{i}");
        row = row.child(
            div()
                .id(SharedString::from(name.clone()))
                .debug_selector(move || name.clone())
                .w(px(20.0))
                .h(px(if i == filled { 24.0 } else { 8.0 }))
                .rounded(px(radius::SM))
                .bg(rgba(if i <= filled {
                    theme.primary
                } else {
                    theme.track
                }))
                .on_click(move |_, w, cx| on_change(v, w, cx)),
        );
    }
    row
}

/// 画面やパネルの地。文字色も役割から決まる。
pub fn surface(theme: &Theme) -> Div {
    div()
        .bg(rgba(theme.surface))
        .text_color(rgba(theme.on_surface))
        .text_size(px(font_size::MD))
        .line_height(px(font_size::MD * 1.4))
        .text_decoration_color(rgba(theme.focus))
}
