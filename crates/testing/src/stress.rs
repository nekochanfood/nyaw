//! 上限までパネルを出す・閉じる・ちりばめる・並べる負荷の画面（[`StressTest`]）。

use super::*;

/// 負荷試験で出すパネルの大きさ（m）。解像度は既定のまま（800 × 600）。
pub const STRESS_SIZE: Vec2 = Vec2::new(0.3, 0.225);

/// 一度に足す枚数（[`StressTest`] の `stress-add`）。
pub const STRESS_STEP: usize = 10;

/// 並べるときの列数と間隔（m）、頭からの距離（m）。
const GRID_COLS: usize = 8;

const GRID_GAP: Vec2 = Vec2::new(0.33, 0.255);

const GRID_DISTANCE: f32 = 2.0;

/// 負荷試験の画面（`stress-test`）。[`TestingAnim`]・[`TestingLod`]・[`TestingTransparent`] を順に出す。
/// `stress-fill` は開いているパネルが上限（[`ui::PanelControl::limit`]: [`ui::MAX_PANELS`] とランタイムの上限の小さい方）
/// ちょうどになるまで出す。ランタイムがそれより手前で作れなくなれば（OpenVR のほかのアプリの分など）そこで止まり、
/// 届いた数が `stress-count` に出る。
pub struct StressTest {
    /// このパネル（出すパネルの開いた元）。
    panel: PanelId,
    /// 出したパネル（閉じたものも含む）。ビルド時に積まれる。
    spawned: Rc<RefCell<Vec<PanelId>>>,
    rng: u64,
}

impl StressTest {
    pub fn new(panel: PanelId) -> Self {
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            panel,
            spawned: Rc::default(),
            rng: seed | 1,
        }
    }

    /// 出したパネルのうち閉じていないもの。
    fn alive(&self, cx: &App) -> Vec<PanelId> {
        let ctl = cx.try_global::<PanelControl>();
        self.spawned
            .borrow()
            .iter()
            .copied()
            .filter(|id| {
                ctl.and_then(|c| c.panels.get(id))
                    .is_none_or(|s| !s.status.closed)
            })
            .collect()
    }

    /// 上限を超えない範囲で、最大 `n` 枚を並べる位置に出す。
    fn spawn(&mut self, n: usize, cx: &mut App) {
        let start = self.alive(cx).len();
        let c = cx.default_global::<PanelControl>();
        // spawns に積んだまま次のフレームを待っている分も数える。
        let room = c.limit.saturating_sub(c.open + c.spawns.len());
        for i in start..start + n.min(room) {
            let ids = self.spawned.clone();
            let track = move |id| ids.borrow_mut().push(id);
            let spec = match i % 3 {
                0 => PanelSpec::spawn(self.panel, move |id, _, cx| {
                    track(id);
                    TestingAnim::new(id, cx)
                }),
                1 => PanelSpec::spawn(self.panel, move |id, _, _| {
                    track(id);
                    TestingLod::new(id)
                }),
                _ => PanelSpec::spawn(self.panel, move |id, _, _| {
                    track(id);
                    TestingTransparent::new(id)
                }),
            };
            c.spawns.push(
                // 並べて数えるので、開いた元の手前に重ねず格子に出す。
                spec.in_view(grid_pose(i))
                    .sized(STRESS_SIZE, (800, 600))
                    // 負荷を測るための埋め草で、まとめて閉じる（`stress-close`）。バーを付けると開ける数が半分になる。
                    .without_nav_bar(),
            );
        }
    }

    /// 閉じていない出したパネルに、`i` 枚目なら `f(i)` の操作を積む。
    fn each(&self, cx: &mut App, mut f: impl FnMut(usize) -> PanelCommand) {
        for (i, id) in self.alive(cx).into_iter().enumerate() {
            ui::command(id, f(i), cx);
        }
    }

    /// xorshift64 で 0〜1。
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// 並べるときの `i` 枚目の位置（頭から見た相対姿勢）。正面 [`GRID_DISTANCE`] に [`GRID_COLS`] 列で上から下へ。
pub fn grid_pose(i: usize) -> Pose {
    let (col, row) = ((i % GRID_COLS) as f32, (i / GRID_COLS) as f32);
    Pose::new(
        Vec3::new(
            (col - (GRID_COLS - 1) as f32 / 2.0) * GRID_GAP.x,
            0.6 - row * GRID_GAP.y,
            -GRID_DISTANCE,
        ),
        Quat::IDENTITY,
    )
}

/// `p` に置いたパネルが頭（原点）の方を向く姿勢。
fn facing_head(p: Vec3) -> Pose {
    Pose::new(p, Quat::from_rotation_y((-p.x).atan2(-p.z)))
}

impl Render for StressTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (open, limit) = cx
            .try_global::<PanelControl>()
            .map_or((0, 0), |c| (c.open, c.limit));
        let alive = self.alive(cx).len();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .debug_selector(|| "stress-count".into())
                    .child(i18n::tf(
                        cx,
                        "stress.count",
                        &[("open", &open), ("limit", &limit), ("spawned", &alive)],
                    )),
            )
            .child(
                row()
                    .child(
                        button(&theme, "stress-fill", i18n::t(cx, "stress.fill"), false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(usize::MAX, cx))),
                    )
                    .child(
                        button(&theme, "stress-add", format!("+{STRESS_STEP}"), false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(STRESS_STEP, cx))),
                    )
                    .child(
                        button(
                            &theme,
                            "stress-close",
                            i18n::t(cx, "stress.close_all"),
                            false,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.each(cx, |_| PanelCommand::Close)),
                        ),
                    )
                    .child(
                        button(
                            &theme,
                            "stress-scatter",
                            i18n::t(cx, "stress.scatter"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            // 左右 ±3m・上下 -1〜1.5m・前方 0.8〜8m（距離がばらけて LOD もばらける）。
                            let poses: Vec<_> = (0..this.alive(cx).len())
                                .map(|_| {
                                    facing_head(Vec3::new(
                                        this.random() * 6.0 - 3.0,
                                        this.random() * 2.5 - 1.0,
                                        -0.8 - this.random() * 7.2,
                                    ))
                                })
                                .collect();
                            this.each(cx, |i| PanelCommand::Place(poses[i]));
                        })),
                    )
                    .child(
                        button(&theme, "stress-grid", i18n::t(cx, "stress.grid"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.each(cx, |i| PanelCommand::Place(grid_pose(i)))
                            }),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
