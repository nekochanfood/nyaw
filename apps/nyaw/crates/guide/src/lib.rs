//! 層 4: 操作ガイド。コントローラのレンダリング画像（[`controller`]）に、操作部分を円で囲み、テールで説明と結ぶ。
//! 画像とボタンの位置はリポジトリルートの `assets/renders`（ビルドに埋め込む）。機種の判定は入力層（`input::Controller`）。
//! 詳細は `AGENT.md`。

use std::sync::{Arc, OnceLock};

use design::{Theme, font_size, radius, spacing};
use gpui::{
    App, Hsla, Image, ImageFormat, IntoElement, PathBuilder, Pixels, Point, RenderOnce,
    SharedString, Window, canvas, div, img, point, prelude::*, px, rgba,
};
use kernel::Hand;
pub use kernel::subsystems::input::{Control, Controller, Model};

/// 画像の一辺（px）。ボタンの座標はこの大きさの画像の上の値。
pub const IMAGE_PX: f32 = 1024.0;

/// ボタン 1 つ（`assets/renders/buttons/<手>/<名前>.json` の `buttons`）。座標は画像の px。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Part {
    pub id: String,
    pub x: f32,
    pub y: f32,
    /// 画像上でボタンが占める半径。隠れている部品は 0。
    #[serde(default)]
    pub radius: f32,
    /// 正面から見えるか（トリガーなど裏側の部品は見えない）。
    pub visible: bool,
}

#[derive(serde::Deserialize)]
struct Sheet {
    buttons: Vec<Part>,
}

/// 埋め込んだ画像とボタンの表: (手, レンダーモデル名, PNG, JSON)。
macro_rules! assets {
    ($($side:literal $name:literal),* $(,)?) => {
        &[$((
            $side,
            $name,
            include_bytes!(concat!("../../../../../assets/renders/", $side, "/", $name, ".png")),
            include_str!(concat!("../../../../../assets/renders/buttons/", $side, "/", $name, ".json")),
        )),*]
    };
}

type Asset = (&'static str, &'static str, &'static [u8], &'static str);

const ASSETS: &[Asset] = assets![
    "left" "frame_controller_left",
    "left" "oculus_cv1_controller_left",
    "left" "oculus_quest2_controller_left",
    "left" "oculus_quest_plus_controller_left",
    "left" "oculus_quest_pro_controller_left",
    "left" "oculus_rifts_controller_left",
    "left" "pico_4_controller_left",
    "left" "pico_4_ultra_controller_left",
    "left" "valve_controller_knu_1_0_left",
    "left" "vive_focus3_controller_left",
    "left" "vr_controller_vive_1_5",
    "right" "frame_controller_right",
    "right" "oculus_cv1_controller_right",
    "right" "oculus_quest2_controller_right",
    "right" "oculus_quest_plus_controller_right",
    "right" "oculus_quest_pro_controller_right",
    "right" "oculus_rifts_controller_right",
    "right" "pico_4_controller_right",
    "right" "pico_4_ultra_controller_right",
    "right" "valve_controller_knu_1_0_right",
    "right" "vive_focus3_controller_right",
    "right" "vr_controller_vive_1_5",
];

/// 読み込んだ画像と表（使うときに 1 度だけ。デコードは GPUI のアセットがバックグラウンドで行う）。
static LOADED: [OnceLock<(Arc<Image>, Vec<Part>)>; ASSETS.len()] =
    [const { OnceLock::new() }; ASSETS.len()];

fn asset(model: Model, hand: Hand) -> &'static (Arc<Image>, Vec<Part>) {
    let side = match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    };
    let name = model.render_model(hand);
    let i = ASSETS
        .iter()
        .position(|a| a.0 == side && a.1 == name)
        .unwrap_or_else(|| panic!("no render for {side}/{name}"));
    LOADED[i].get_or_init(|| {
        let (_, _, png, json) = ASSETS[i];
        let sheet: Sheet = serde_json::from_str(json).expect("assets/renders/buttons JSON");
        (
            Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())),
            sheet.buttons,
        )
    })
}

/// `model` の `hand` のすべてのボタン。
pub fn sheet(model: Model, hand: Hand) -> &'static [Part] {
    &asset(model, hand).1
}

/// `model` の `hand` のボタンのうち、部品名 `part`（`grip` なら Vive の `grip_left` / `grip_right` も）に当たるもの。
pub fn parts(model: Model, hand: Hand, part: &str) -> Vec<Part> {
    sheet(model, hand)
        .iter()
        .filter(|p| {
            p.id == part
                || p.id
                    .strip_prefix(part)
                    .is_some_and(|s| matches!(s, "_left" | "_right"))
        })
        .cloned()
        .collect()
}

/// 操作の名前（表示用）。
pub fn control_name(cx: &App, control: Control) -> SharedString {
    match control {
        Control::Select => i18n::t(cx, "guide.control.select"),
        Control::Grab => i18n::t(cx, "guide.control.grab"),
        Control::Menu => i18n::t(cx, "guide.control.menu"),
        Control::Shortcut => i18n::t(cx, "guide.control.shortcut"),
        Control::Scroll => i18n::t(cx, "guide.control.scroll"),
    }
}

/// 操作で何が起きるか（表示用）。
pub fn control_detail(cx: &App, control: Control) -> SharedString {
    match control {
        Control::Select => i18n::t(cx, "guide.detail.select"),
        Control::Grab => i18n::t(cx, "guide.detail.grab"),
        Control::Menu => i18n::t(cx, "guide.detail.menu"),
        Control::Shortcut => i18n::t(cx, "guide.detail.shortcut"),
        Control::Scroll => i18n::t(cx, "guide.detail.scroll"),
    }
}

/// 部品の名前（表示用。表に無い部品は ID を大文字にしたもの）。
pub fn part_name(cx: &App, part: &str) -> SharedString {
    match part {
        "trigger" => i18n::t(cx, "guide.part.trigger"),
        "grip" | "grip_left" | "grip_right" => i18n::t(cx, "guide.part.grip"),
        "thumbstick" => i18n::t(cx, "guide.part.thumbstick"),
        "trackpad" => i18n::t(cx, "guide.part.trackpad"),
        "menu" => i18n::t(cx, "guide.part.menu"),
        "system" => i18n::t(cx, "guide.part.system"),
        "bumper" => i18n::t(cx, "guide.part.bumper"),
        "view" => i18n::t(cx, "guide.part.view"),
        other => other.replace('_', " ").to_uppercase().into(),
    }
}

/// `controller` の `hand` で割り当てのある操作ごとの説明（画像に部品のある操作だけ）。札の ID は `guide-<操作>`。
pub fn bindings(cx: &App, controller: Controller, hand: Hand) -> Vec<(Control, Callout)> {
    Control::ALL
        .into_iter()
        .filter_map(|control| {
            let found: Vec<&'static str> = controller
                .parts(control, hand)
                .into_iter()
                .filter(|p| !parts(controller.model, hand, p).is_empty())
                .collect();
            if found.is_empty() {
                return None;
            }
            let names: Vec<SharedString> = found.iter().map(|p| part_name(cx, p)).collect();
            Some((
                control,
                Callout {
                    id: format!("guide-{}", control_id(control)).into(),
                    parts: found,
                    title: control_name(cx, control),
                    detail: Some(names.join(" / ").into()),
                },
            ))
        })
        .collect()
}

/// 操作の ID（`debug_selector` 用）。
pub fn control_id(control: Control) -> &'static str {
    match control {
        Control::Select => "select",
        Control::Grab => "grab",
        Control::Menu => "menu",
        Control::Shortcut => "shortcut",
        Control::Scroll => "scroll",
    }
}

/// 説明 1 つ: 囲む部品（[`Controller::parts`]）と、テールの先に出す見出し・説明文。
#[derive(Debug, Clone)]
pub struct Callout {
    /// 説明の札の `debug_selector`。
    pub id: SharedString,
    pub parts: Vec<&'static str>,
    pub title: SharedString,
    pub detail: Option<SharedString>,
}

/// コントローラの画像と説明。`controller(model, hand).size(px(400.)).callouts(..)`。
#[derive(IntoElement)]
pub struct ControllerGuide {
    model: Model,
    hand: Hand,
    size: Pixels,
    callouts: Vec<Callout>,
    focus: Option<usize>,
}

pub fn controller(model: Model, hand: Hand) -> ControllerGuide {
    ControllerGuide {
        model,
        hand,
        size: px(400.0),
        callouts: Vec::new(),
        focus: None,
    }
}

impl ControllerGuide {
    /// 画像の一辺。説明の列は左右に [`GUTTER`] の割合ずつ足す。
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn callouts(mut self, callouts: Vec<Callout>) -> Self {
        self.callouts = callouts;
        self
    }

    /// 強調する説明（ほかは薄くする）。`None` ならすべて同じ強さ。
    pub fn focus(mut self, focus: Option<usize>) -> Self {
        self.focus = focus;
        self
    }
}

/// 説明の列の幅（画像の一辺に対する割合）。
pub const GUTTER: f32 = 0.5;
/// 説明の札の高さ（px）。札どうしはこれ以上離す。列に入りきらなければ詰め、説明文を省く（[`TITLE_ONLY_H`] 未満）。
const LABEL_H: f32 = 52.0;
const LABEL_GAP: f32 = 6.0;
const TITLE_ONLY_H: f32 = 44.0;
/// 円をボタンの縁からどれだけ離すか（表示の px）。線の太さ（[`RING_W`]）を引いても隙間が残り、ボタンの形と刻印が見える。
const RING_PAD: f32 = 9.0;
const RING_W: f32 = 3.0;
/// 小さく写るボタンでも、円の内側に収める最小の半径（表示の px）。
const RING_MIN: f32 = 7.0;
/// 握り全体のような大きな部品でも画像を覆わないよう、ボタンの半径をこれ（画像の px）で打ち切る。
const RING_MAX: f32 = 110.0;

/// 円 1 つ（表示の px。部品が見えないときは無い）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    pub center: Point<f32>,
    pub radius: f32,
}

/// 説明 1 つの配置（表示の px、左上が原点）。
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// 左の列か。
    pub left: bool,
    /// 札の縦の中心と高さ。
    pub y: f32,
    pub h: f32,
    /// 部品ごとの、円（見えない部品は `None`）とテールの終点（円の縁、見えない部品は中心）。
    pub targets: Vec<(Option<Ring>, Point<f32>)>,
}

/// 画像の一辺 `size`（px）のときの説明の配置。部品が 1 つも無い説明は `None`。
/// 札は部品の高さに置き、重なる分だけ下へ（下端を越えたら上へ）ずらす。左右は部品が画像のどちら側か。
pub fn layout(model: Model, hand: Hand, size: f32, callouts: &[Callout]) -> Vec<Option<Placed>> {
    let scale = size / IMAGE_PX;
    let gutter = size * GUTTER;
    let mut placed: Vec<Option<Placed>> = callouts
        .iter()
        .map(|c| {
            let found: Vec<Part> = c.parts.iter().flat_map(|p| parts(model, hand, p)).collect();
            if found.is_empty() {
                return None;
            }
            let n = found.len() as f32;
            let x = found.iter().map(|p| p.x).sum::<f32>() / n * scale;
            let y = found.iter().map(|p| p.y).sum::<f32>() / n * scale;
            let targets = found
                .iter()
                .map(|p| {
                    let center = point(gutter + p.x * scale, p.y * scale);
                    let radius = (p.radius.min(RING_MAX) * scale).max(RING_MIN) + RING_PAD;
                    (p.visible.then_some(Ring { center, radius }), center)
                })
                .collect();
            Some(Placed {
                left: x < size / 2.0,
                y,
                h: LABEL_H,
                targets,
            })
        })
        .collect();
    // 片側の列に入りきらなければ、中央寄りの部品の札から反対側へ回す（部品の x は円の中心から）。
    let step = LABEL_H + LABEL_GAP;
    let capacity = ((size + LABEL_GAP) / step).floor().max(1.0) as usize;
    let x_of =
        |p: &Placed| p.targets.iter().map(|(_, c)| c.x).sum::<f32>() / p.targets.len() as f32;
    for left in [true, false] {
        loop {
            let count = |side: bool| placed.iter().flatten().filter(|p| p.left == side).count();
            if count(left) <= capacity || count(!left) >= capacity {
                break;
            }
            let inner = placed
                .iter_mut()
                .flatten()
                .filter(|p| p.left == left)
                .max_by(|a, b| {
                    let (a, b) = (x_of(a), x_of(b));
                    if left {
                        a.total_cmp(&b)
                    } else {
                        b.total_cmp(&a)
                    }
                })
                .expect("column is not empty");
            inner.left = !left;
        }
    }
    for left in [true, false] {
        let mut column: Vec<&mut Placed> = placed
            .iter_mut()
            .flatten()
            .filter(|p| p.left == left)
            .collect();
        column.sort_by(|a, b| a.y.total_cmp(&b.y));
        // それでも入らなければ、札を低くして詰める。
        let step = step.min(size / column.len().max(1) as f32);
        let h = step - LABEL_GAP;
        let mut floor = h / 2.0;
        for p in column.iter_mut() {
            p.h = h;
            p.y = p.y.max(floor);
            floor = p.y + step;
        }
        let mut ceiling = size - h / 2.0;
        for p in column.iter_mut().rev() {
            p.y = p.y.min(ceiling);
            ceiling = p.y - step;
        }
    }
    // テールは札の脇から画像の余白（折れ点）まで水平に出て、そこから円の縁（見えない部品は中心）へ向かう。
    for p in placed.iter_mut().flatten() {
        let bend = elbow(p, size);
        for (ring, end) in &mut p.targets {
            *end = edge(*ring, *end, bend);
        }
    }
    placed
}

/// テールの折れ点（画像の余白の中、札と同じ高さ）。
fn elbow(p: &Placed, size: f32) -> Point<f32> {
    let gutter = size * GUTTER;
    let x = if p.left {
        gutter + ELBOW_INSET
    } else {
        gutter + size - ELBOW_INSET
    };
    point(x, p.y)
}

/// 折れ点を画像の端からどれだけ内側に置くか（px）。画像の余白（5%）に収まる。
const ELBOW_INSET: f32 = 8.0;

/// 円の縁のうち `from` に向いた点（円が無ければ中心）。
fn edge(ring: Option<Ring>, center: Point<f32>, from: Point<f32>) -> Point<f32> {
    let Some(r) = ring else { return center };
    let (dx, dy) = (from.x - center.x, from.y - center.y);
    let d = (dx * dx + dy * dy).sqrt().max(1e-3);
    point(center.x + dx / d * r.radius, center.y + dy / d * r.radius)
}

impl RenderOnce for ControllerGuide {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let size = f32::from(self.size);
        let gutter = size * GUTTER;
        let placed = layout(self.model, self.hand, size, &self.callouts);
        let accent = Hsla::from(rgba(theme.accent_solid));
        let strength = |i: usize| match self.focus {
            Some(f) if f != i => 0.3,
            _ => 1.0,
        };
        // テール（線）は 1 枚のキャンバスに描く。見えない部品へは破線。
        // (強さ, 札の脇, 折れ点, [(見えない部品か, 終点)])
        type Tail = (f32, Point<f32>, Point<f32>, Vec<(bool, Point<f32>)>);
        let tails: Vec<Tail> = placed
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let p = p.as_ref()?;
                let x = if p.left {
                    gutter - spacing::SM
                } else {
                    gutter + size + spacing::SM
                };
                Some((
                    strength(i),
                    point(x, p.y),
                    elbow(p, size),
                    p.targets
                        .iter()
                        .map(|(ring, end)| (ring.is_none(), *end))
                        .collect(),
                ))
            })
            .collect();
        let lines = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let at = |p: Point<f32>| bounds.origin + point(px(p.x), px(p.y));
                for (alpha, start, elbow, targets) in &tails {
                    let (color, elbow) = (accent.opacity(*alpha), *elbow);
                    let mut lead = PathBuilder::stroke(px(2.0));
                    lead.move_to(at(*start));
                    lead.line_to(at(elbow));
                    if let Ok(path) = lead.build() {
                        window.paint_path(path, color);
                    }
                    for (hidden, end) in targets {
                        let mut tail = PathBuilder::stroke(px(2.0));
                        if *hidden {
                            tail = tail.dash_array(&[px(5.0), px(4.0)]);
                        }
                        tail.move_to(at(elbow));
                        tail.line_to(at(*end));
                        if let Ok(path) = tail.build() {
                            window.paint_path(path, color);
                        }
                    }
                }
            },
        )
        .absolute()
        .inset_0();
        let (image, _) = asset(self.model, self.hand);
        let mut root = div()
            .relative()
            .flex_none()
            .w(px(size + 2.0 * gutter))
            .h(self.size)
            .child(
                img(image.clone())
                    .absolute()
                    .left(px(gutter))
                    .top_0()
                    .size(self.size),
            )
            .child(lines);
        for (i, (callout, p)) in self.callouts.iter().zip(&placed).enumerate() {
            let Some(p) = p else { continue };
            let alpha = strength(i);
            for (ring, _) in &p.targets {
                let Some(r) = ring else { continue };
                // ボタンの縁から離した円。外側に薄い影を重ね、白い本体の上でも読めるようにする。
                let id = format!("{}-ring", callout.id);
                let circle = |d: f32, w: f32, color: Hsla| {
                    div()
                        .absolute()
                        .left(px(r.center.x - d / 2.0))
                        .top(px(r.center.y - d / 2.0))
                        .size(px(d))
                        .rounded_full()
                        .border(px(w))
                        .border_color(color)
                };
                root = root
                    .child(circle(
                        2.0 * r.radius + 4.0,
                        RING_W + 4.0,
                        gpui::black().opacity(0.28 * alpha),
                    ))
                    .child(
                        circle(2.0 * r.radius, RING_W, accent.opacity(alpha))
                            .debug_selector(move || id.clone()),
                    );
            }
            let width = gutter - 2.0 * spacing::SM;
            let id = callout.id.clone();
            let label = div()
                .debug_selector(move || id.to_string())
                .absolute()
                .top(px(p.y - p.h / 2.0))
                .h(px(p.h))
                .max_w(px(width))
                .when(p.left, |d| d.right(px(size + gutter + spacing::SM)))
                .when(!p.left, |d| d.left(px(gutter + size + spacing::SM)))
                .opacity(alpha)
                .px(px(spacing::SM))
                .flex()
                .flex_col()
                .justify_center()
                .rounded(px(radius::LG))
                .bg(rgba(theme.card))
                .border_1()
                .border_color(if self.focus == Some(i) {
                    accent
                } else {
                    rgba(theme.neutral_border_subtle).into()
                })
                .when(p.left, |d| d.items_end())
                .child(
                    div()
                        .text_size(px(font_size::MD))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgba(theme.neutral_text))
                        .truncate()
                        .child(callout.title.clone()),
                )
                .children(
                    callout
                        .detail
                        .clone()
                        .filter(|_| p.h >= TITLE_ONLY_H)
                        .map(|d| {
                            div()
                                .text_size(px(font_size::SM))
                                .text_color(rgba(theme.neutral_text_low))
                                .truncate()
                                .child(d)
                        }),
                );
            root = root.child(label);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// どの機種・手にも画像とボタンの表があり、表は読める。
    #[test]
    fn every_model_has_a_render() {
        for model in Model::ALL {
            for hand in [Hand::Left, Hand::Right] {
                let (image, parts) = asset(model, hand);
                assert!(!image.bytes().is_empty());
                assert!(parts.iter().any(|p| p.visible), "{model:?} {hand:?}");
            }
        }
    }

    /// 既定の割り当ての操作は、どの機種でも画像の部品に当たる（メニューは右手。Steam Frame の左手には Touch のボタンが無い）。
    #[test]
    fn default_bindings_point_at_parts() {
        for model in Model::ALL {
            let c = Controller::of(model);
            for hand in [Hand::Left, Hand::Right] {
                let menu = (hand == Hand::Right).then_some(Control::Menu);
                for control in [Control::Select, Control::Grab, Control::Scroll]
                    .into_iter()
                    .chain(menu)
                {
                    let found = c
                        .parts(control, hand)
                        .iter()
                        .flat_map(|p| parts(model, hand, p))
                        .count();
                    assert!(found > 0, "{model:?} {hand:?} {control:?}");
                }
            }
        }
    }

    fn callout(id: &'static str, parts: &[&'static str]) -> Callout {
        Callout {
            id: id.into(),
            parts: parts.to_vec(),
            title: id.into(),
            detail: None,
        }
    }

    /// 札は重ならず画像の高さに収まり、見える部品は円で、隠れた部品（トリガー）は円を持たない。円はボタンより大きい。
    #[test]
    fn layout_rings_visible_parts_and_stacks_labels() {
        let size = 400.0;
        let all = [
            "trigger",
            "grip",
            "a",
            "b",
            "thumbstick",
            "system",
            "x",
            "y",
            "menu",
        ];
        let callouts: Vec<_> = all.iter().map(|p| callout(p, &[p])).collect();
        for model in Model::ALL {
            for hand in [Hand::Left, Hand::Right] {
                let placed = layout(model, hand, size, &callouts);
                for left in [true, false] {
                    let mut ys: Vec<f32> = placed
                        .iter()
                        .flatten()
                        .filter(|p| p.left == left)
                        .map(|p| p.y)
                        .collect();
                    ys.sort_by(f32::total_cmp);
                    for w in ys.windows(2) {
                        assert!(w[1] - w[0] >= TITLE_ONLY_H, "{model:?} {hand:?} {ys:?}");
                    }
                    let h = TITLE_ONLY_H / 2.0 - 0.01;
                    assert!(ys.iter().all(|y| *y >= h && *y <= size - h), "{ys:?}");
                }
                for (c, p) in callouts.iter().zip(&placed) {
                    let Some(p) = p else { continue };
                    for ((ring, _), part) in p
                        .targets
                        .iter()
                        .zip(c.parts.iter().flat_map(|id| parts(model, hand, id)))
                    {
                        assert_eq!(ring.is_some(), part.visible, "{model:?} {}", part.id);
                        if let Some(r) = ring {
                            assert!(
                                r.radius - RING_W > part.radius.min(RING_MAX) * size / IMAGE_PX
                            );
                        }
                    }
                }
                let trigger = placed[0].as_ref().unwrap();
                assert!(trigger.targets.iter().all(|(r, _)| r.is_none()));
            }
        }
    }
}
