//! 案内（ツアー）: 複数のパネルにまたがって操作を案内する仕組み。案内する画面（ダッシュボードのチュートリアル）が焦点
//! （[`Focus`]）を置くと、焦点の外のパネルは暗くなって入力を受けなくなる。焦点のパネルでは、強調する要素
//! （`design::spot`）を輪で囲み、その要素だけが押せる。手順と文言は案内する画面が持つ。

use gpui::{AnyElement, Bounds, HitboxBehavior, Pixels, SharedString, canvas, point, quad, size};

use super::*;

/// 強調の輪を要素の縁から離す幅と、線の太さ（px）。
const RING_PAD: f32 = 6.0;
const RING_W: f32 = 3.0;
/// 押せる要素の輪の外へ広がって消える光の、1 周期（秒）と広がる幅（px）。
const PULSE: f32 = 1.6;
const PULSE_GROW: f32 = 14.0;

/// 案内の状態（GPUI のグローバル）。
#[derive(Default)]
pub struct Tour {
    /// 案内を頼まれた回数（[`start`]）。案内する画面は、増えたら始める。
    pub asked: u32,
    focus: Option<Focus>,
}

impl gpui::Global for Tour {}

/// 案内の焦点。ここに無いパネルは暗くなり、入力を受けない。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Focus {
    /// 暗くせず、入力もそのまま通すパネル（案内のパネル、練習用のウィンドウ）。
    pub free: Vec<PanelId>,
    /// 暗くせず、強調して押せる要素（`spots`）にだけ入力を通すパネル。
    pub lit: Vec<PanelId>,
    /// 強調して押せる要素（`design::spot` の ID）。
    pub spots: Vec<SharedString>,
    /// 強調するが押せない要素（見せて説明するだけのもの）。
    pub marks: Vec<SharedString>,
    /// ダッシュボードの開閉（ショートカットの二回押し・何も無い所の押下）を許す。
    pub toggle: bool,
}

/// 案内を頼む（ダッシュボードが表示されたら、チュートリアルが始まる）。
pub fn start(cx: &mut App) {
    cx.default_global::<Tour>().asked += 1;
}

/// 案内の最中か。
pub fn running(cx: &App) -> bool {
    focus(cx).is_some()
}

/// 今の焦点（案内の最中でなければ `None`）。
pub fn focus(cx: &App) -> Option<&Focus> {
    cx.try_global::<Tour>().and_then(|t| t.focus.as_ref())
}

/// ダッシュボードを開閉してよいか（案内の最中は、焦点が許すときだけ）。
pub fn may_toggle(cx: &App) -> bool {
    focus(cx).is_none_or(|f| f.toggle)
}

/// 焦点を置き換える（`None` で案内を終える）。変われば、すべてのパネルを描き直す。
pub fn set(focus: Option<Focus>, cx: &mut App) {
    let tour = cx.default_global::<Tour>();
    if tour.focus == focus {
        return;
    }
    let ids = focus
        .iter()
        .flat_map(|f| f.spots.iter().chain(&f.marks).cloned())
        .collect();
    tour.focus = focus;
    design::spot::want(ids, cx);
    let panels: Vec<PanelId> = cx
        .try_global::<PanelControl>()
        .map(|c| c.panels.keys().copied().collect())
        .unwrap_or_default();
    for p in panels {
        redraw(p, cx);
    }
    cx.refresh_windows();
}

/// `panel` を暗くするか（案内の最中で、焦点に無いパネル）。
pub(crate) fn dims(panel: PanelId, cx: &App) -> bool {
    focus(cx).is_some_and(|f| !f.free.contains(&panel) && !f.lit.contains(&panel))
}

/// `panel` のウィンドウに重ねる覆い。焦点のパネルでは強調する要素を輪で囲み、押せる要素の外への入力を止める。
/// 焦点に無いパネルでは、すべての入力を止める。案内の最中でないか、入力をそのまま通すパネルなら `None`。
pub(crate) fn mask(panel: PanelId, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let f = focus(cx)?;
    if f.free.contains(&panel) {
        return None;
    }
    let lit = f.lit.contains(&panel);
    let (spots, marks) = (f.spots.clone(), f.marks.clone());
    let accent = gpui::Hsla::from(gpui::rgba(design::Theme::current(cx).accent_solid));
    if lit {
        design::drive::keep_drawing(window, cx);
    }
    let pulse = (design::drive::now()
        .duration_since(*EPOCH.get_or_init(Instant::now))
        .as_secs_f32()
        / PULSE)
        .fract();
    Some(
        canvas(
            move |bounds, window, cx| {
                // 見つかった要素と、押せるか（押せる要素の輪だけ光を広げる）。
                let found: Vec<_> = design::spot::take(window, cx)
                    .into_iter()
                    .filter(|_| lit)
                    .filter_map(|s| {
                        let press = spots.contains(&s.id);
                        (press || marks.contains(&s.id)).then_some((s, press))
                    })
                    .collect();
                let open: Vec<_> = found
                    .iter()
                    .filter(|(_, press)| *press)
                    .map(|(s, _)| s.bounds)
                    .collect();
                for r in outside(bounds, &open) {
                    window.insert_hitbox(r, HitboxBehavior::BlockMouse);
                }
                found
            },
            move |_, found, window, _| {
                for (s, press) in found {
                    let ring = |grow: f32, color: gpui::Hsla| {
                        let pad = RING_PAD + grow;
                        let b = s.bounds.dilate(px(pad));
                        let r = (f32::from(s.radius) + pad)
                            .min(f32::from(b.size.width.min(b.size.height)) / 2.0);
                        quad(
                            b,
                            px(r),
                            gpui::transparent_black(),
                            px(RING_W),
                            color,
                            Default::default(),
                        )
                        .superellipse(design::corner::SUPERELLIPSE)
                    };
                    if press {
                        let halo = accent.opacity(0.6 * (1.0 - pulse));
                        window.paint_quad(ring(PULSE_GROW * pulse, halo));
                    }
                    window.paint_quad(ring(0.0, accent));
                }
            },
        )
        .absolute()
        .inset_0()
        .into_any_element(),
    )
}

/// 光の周期の起点（位相をそろえるだけなので、最初に描いた時刻でよい）。
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// `bounds` のうち、`holes` のどれにも入らない部分を、重ならない矩形に分けたもの。
fn outside(bounds: Bounds<Pixels>, holes: &[Bounds<Pixels>]) -> Vec<Bounds<Pixels>> {
    let holes: Vec<_> = holes.iter().map(|h| h.intersect(&bounds)).collect();
    let edges = |lo: Pixels, hi: Pixels, of: &dyn Fn(&Bounds<Pixels>) -> [Pixels; 2]| {
        let mut e: Vec<Pixels> = holes.iter().flat_map(of).chain([lo, hi]).collect();
        e.sort_by(|a, b| f32::from(*a).total_cmp(&f32::from(*b)));
        e.dedup();
        e
    };
    let xs = edges(bounds.left(), bounds.right(), &|h| [h.left(), h.right()]);
    let ys = edges(bounds.top(), bounds.bottom(), &|h| [h.top(), h.bottom()]);
    let mut out = Vec::new();
    for y in ys.windows(2) {
        for x in xs.windows(2) {
            let cell = Bounds::new(point(x[0], y[0]), size(x[1] - x[0], y[1] - y[0]));
            if cell.size.width > px(0.0)
                && cell.size.height > px(0.0)
                && !holes.iter().any(|h| h.contains(&cell.center()))
            {
                out.push(cell);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    /// 穴の外を、穴と重ならず隙間も無く覆う（面積の和が外側の面積と一致し、穴の中心はどれにも入らない）。
    #[test]
    fn outside_covers_everything_but_the_holes() {
        let area = |b: &Bounds<Pixels>| f32::from(b.size.width) * f32::from(b.size.height);
        let bounds = rect(0.0, 0.0, 100.0, 50.0);
        let holes = [rect(10.0, 10.0, 20.0, 10.0), rect(60.0, 5.0, 30.0, 60.0)];
        let cells = outside(bounds, &holes);
        let covered: f32 = cells.iter().map(area).sum();
        // 2 つ目の穴は下へはみ出すので、内側（30 × 45）だけを引く。
        assert_eq!(covered, 100.0 * 50.0 - 20.0 * 10.0 - 30.0 * 45.0);
        for h in &holes {
            assert!(!cells.iter().any(|c| c.contains(&h.center())));
        }
        assert_eq!(outside(bounds, &[]), vec![bounds]);
    }
}
