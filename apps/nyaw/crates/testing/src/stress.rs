//! パネルを上限まで生成・閉じる・ランダム配置・グリッド整列するストレステスト画面（[`StressTest`]）。

use super::*;

/// ストレステストで生成するパネルのサイズ（m）。解像度はデフォルト（800 × 600）。
pub const STRESS_SIZE: Vec2 = Vec2::new(0.3, 0.225);

/// 1 回の追加で生成する枚数（[`StressTest`] の `stress-add`）。
pub const STRESS_STEP: usize = 10;

/// グリッド配置の列数と間隔（m）、HMD からの距離（m）。
const GRID_COLS: usize = 8;

const GRID_GAP: Vec2 = Vec2::new(0.33, 0.255);

const GRID_DISTANCE: f32 = 2.0;

/// ストレステスト画面（`stress-test`）。[`TestingAnim`]・[`TestingLod`]・[`TestingTransparent`] を順に生成する。
/// `stress-fill` は開いているパネルが上限（[`ui::PanelControl::limit`]: [`ui::MAX_PANELS`] とランタイム上限の小さい方）
/// に達するまで生成する。ランタイムがそれより前に生成不可になった場合（OpenVR の他アプリの占有分等）はそこで停止し、
/// 到達数が `stress-count` に表示される。
pub struct StressTest {
    /// このパネル（生成するパネルの起動元）。
    panel: PanelId,
    /// 生成済みパネル（閉じたものを含む）。生成時に追加される。
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

    /// 生成済みパネルのうち未閉じのもの。
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

    /// 上限を超えない範囲で、最大 `n` 枚をグリッド位置に生成する。
    fn spawn(&mut self, n: usize, cx: &mut App) {
        let start = self.alive(cx).len();
        let c = cx.default_global::<PanelControl>();
        // spawns にキューイングされて次フレーム待ちの分もカウントする。
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
                // グリッドで数えるため、起動元の手前に重ねずグリッド位置に配置する。
                spec.in_view(grid_pose(i))
                    .sized(STRESS_SIZE, (800, 600))
                    // 負荷計測用のフィラーパネルで、一括閉じ（`stress-close`）する。ナビバーを付けると生成可能数が半減する。
                    .without_nav_bar(),
            );
        }
    }

    /// 未閉じの生成済みパネルに、`i` 番目に対して `f(i)` のコマンドをキューイングする。
    fn each(&self, cx: &mut App, mut f: impl FnMut(usize) -> PanelCommand) {
        for (i, id) in self.alive(cx).into_iter().enumerate() {
            ui::command(id, f(i), cx);
        }
    }

    /// xorshift64 で 0〜1 の乱数を生成する。
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// グリッド配置時の `i` 番目のパネル位置（HMD からの相対姿勢）。正面 [`GRID_DISTANCE`] に [`GRID_COLS`] 列で上から下へ配置。
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

/// 位置 `p` に配置されたパネルが原点（HMD）方向を向く姿勢。
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
                            // 左右 ±3m、上下 -1〜1.5m、前方 0.8〜8m（距離の分散により LOD も分散する）。
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
