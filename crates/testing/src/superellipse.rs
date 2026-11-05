use super::*;

/// 角丸のスーパーエリプス（CSS `superellipse(k)`）を確かめる画面。`superellipse-radius` で角の大きさ、
/// `superellipse-k` で強さ k（0 = 面取り、1 = 円、2 = スクワークル、負 = えぐれ）を動かすと、
/// `superellipse-preview`（塗り）と `superellipse-outline`（枠線）が変わる。下の見本は k を固定して並べる。
pub struct SuperellipseTest {
    radius: f32,
    k: f32,
}

/// 強さ k のスライダーの範囲。
const K_MIN: f32 = -2.0;
const K_MAX: f32 = 4.0;
const RADIUS_MAX: f32 = 80.0;

impl SuperellipseTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            radius: 48.0,
            k: 1.2,
        }
    }
}

fn sample(
    theme: &Theme,
    id: &'static str,
    size: f32,
    radius: f32,
    k: f32,
    outline: bool,
) -> Stateful<Div> {
    let d = div()
        .id(id)
        .debug_selector(move || id.into())
        .w(px(size))
        .h(px(size))
        .rounded(px(radius))
        .corner_superellipse(k);
    if outline {
        d.border_4().border_color(rgba(theme.accent_solid))
    } else {
        d.bg(rgba(theme.accent_solid))
    }
}

impl Render for SuperellipseTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity().downgrade();
        let (view_r, view_k) = (view.clone(), view.clone());
        let presets = [
            ("bevel", 0.0),
            ("circle", 1.0),
            ("squircle", 2.0),
            ("notch", -1.0),
        ]
        .map(|(name, k)| {
            let id: &'static str = match name {
                "bevel" => "superellipse-bevel",
                "circle" => "superellipse-circle",
                "squircle" => "superellipse-squircle",
                _ => "superellipse-notch",
            };
            let view = view.clone();
            themed_button(&theme, id, name, (self.k - k).abs() < 1e-3).on_click(move |_, _, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.k = k;
                    cx.notify();
                });
            })
        });
        let fixed = [-1.0, 0.0, 1.0, 2.0, 3.0].map(|k| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(spacing::SM))
                .child(sample(&theme, "superellipse-fixed", 72.0, 32.0, k, false))
                .child(format!("k={k}"))
        });
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(
                div()
                    .debug_selector(|| "superellipse-value".into())
                    .child(format!("radius {:.0} px · k {:.2}", self.radius, self.k)),
            )
            .child(
                components::slider(
                    &theme,
                    "superellipse-radius",
                    self.radius / RADIUS_MAX,
                    move |v, _, cx| {
                        let _ = view_r.update(cx, |this, cx| {
                            this.radius = v * RADIUS_MAX;
                            cx.notify();
                        });
                    },
                )
                .length(400.),
            )
            .child(
                components::slider(
                    &theme,
                    "superellipse-k",
                    (self.k - K_MIN) / (K_MAX - K_MIN),
                    move |v, _, cx| {
                        let _ = view_k.update(cx, |this, cx| {
                            this.k = K_MIN + v * (K_MAX - K_MIN);
                            cx.notify();
                        });
                    },
                )
                .length(400.),
            )
            .child(row().children(presets))
            .child(
                row()
                    .gap(px(spacing::LG))
                    .child(sample(
                        &theme,
                        "superellipse-preview",
                        160.,
                        self.radius,
                        self.k,
                        false,
                    ))
                    .child(sample(
                        &theme,
                        "superellipse-outline",
                        160.,
                        self.radius,
                        self.k,
                        true,
                    )),
            )
            .child(row().gap(px(spacing::LG)).children(fixed))
    }
}
