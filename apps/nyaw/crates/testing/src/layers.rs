//! パネルのレイヤー（描画順と重なりの奥行き。`crates/overlay` の `layers.rs`）を確かめる画面（[`LayerTest`]）。

use super::*;

/// 面に重ねる子の範囲（親の面を 1 とした `[x, y, 幅, 高さ]`）。出すたびにこの量ずつ右下へずらし、重ねて出す。
/// ずらす量は、下の見本の閉じるボタンが、上の見本の影の余白（透明でもレーザーを受ける）にも隠れない大きさ。
const OVER_AREA: [f32; 4] = [0.45, 0.5, 0.45, 0.45];
const OVER_STEP: f32 = 0.18;
/// ずらすのを最初に戻すまでの枚数。
const OVER_CYCLE: usize = 3;
/// 見本のパネルの解像度（px）と、1px の大きさ（m）。
const SAMPLE_PX: (u32, u32) = (360, 220);
const SAMPLE_M_PER_PX: f32 = 0.001;
/// HUD は操作のボタンに被らないよう小さくする。
const HUD_SCALE: f32 = 0.6;
/// 操作のボタンの上の余白（px）。左上に重ねる HUD がここに収まる。
const TOP_SPACE: f32 = 150.0;

/// 見本の種類。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerKind {
    /// 親の面の一部に重ねる子。出すたびに 1 枚増え、重なった分だけ手前にずれる。
    Over,
    /// ポップオーバー。親と同じ奥行きを要望するので、ナビゲーションバーより手前にずれる。外を押すと閉じる。
    Popover,
    /// ダイアログ。親を暗くする。
    Dialog,
    /// 親の左上に重ねる表示専用の子。押せないので「全部閉じる」で閉じる。
    Hud,
    /// 親より 30cm 奥の独立したパネル。そのナビゲーションバーは、手前のこの画面より下に描かれる。
    Far,
}

impl LayerKind {
    const ALL: [Self; 5] = [
        Self::Over,
        Self::Popover,
        Self::Dialog,
        Self::Hud,
        Self::Far,
    ];

    /// ボタンの `debug_selector`。
    fn id(self) -> &'static str {
        match self {
            Self::Over => "layer-over",
            Self::Popover => "layer-popover",
            Self::Dialog => "layer-dialog",
            Self::Hud => "layer-hud",
            Self::Far => "layer-far",
        }
    }

    /// 名前の文言キー。
    fn name(self) -> &'static str {
        match self {
            Self::Over => "layer_test.over",
            Self::Popover => "layer_test.popover",
            Self::Dialog => "layer_test.dialog",
            Self::Hud => "layer_test.hud",
            Self::Far => "layer_test.far",
        }
    }
}

/// レイヤーを確かめる画面。見本（[`LayerSample`]）を出して、重なり方と、それぞれに表示される確定した描画順・
/// 手前にずらした量を見る。
#[derive(Default)]
pub struct LayerTest {
    panel: PanelId,
    /// 出した見本（閉じたものも残る）。
    pub samples: Vec<(LayerKind, PanelId)>,
}

impl LayerTest {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }

    /// パネル `from` の隣に開く起動仕様。
    pub fn spec(from: PanelId) -> PanelSpec {
        PanelSpec::spawn(from, |id, _, _| LayerTest::new(id)).titled("testing.open.layers")
    }

    /// 見本 `kind` の起動仕様。`overs` は今までに出した面に重ねる子の数。
    fn sample(&self, kind: LayerKind, overs: usize, me: WeakEntity<Self>) -> PanelSpec {
        let panel = self.panel;
        let number = self.samples.iter().filter(|(k, _)| *k == kind).count() + 1;
        let build = move |id, _: &mut Window, cx: &mut Context<LayerSample>| {
            me.update(cx, |t, _| t.samples.push((kind, id))).ok();
            // 描画順とずらした量は動かすたびに変わるので、毎フレーム描き直す。
            cx.default_global::<PanelControl>()
                .panels
                .entry(id)
                .or_default()
                .live = true;
            LayerSample {
                panel: id,
                kind,
                number,
            }
        };
        let size = Vec2::new(SAMPLE_PX.0 as f32, SAMPLE_PX.1 as f32) * SAMPLE_M_PER_PX;
        let at = |x, y, z| Pose::new(Vec3::new(x, y, z), Quat::IDENTITY);
        let spec = match kind {
            LayerKind::Over => {
                let shift = (overs % OVER_CYCLE) as f32 * OVER_STEP;
                let [x, y, w, h] = OVER_AREA;
                PanelSpec::over(panel, [x + shift, y + shift, w, h], build)
            }
            LayerKind::Popover => PanelSpec::popover(panel, build).offset(at(-0.15, -0.3, 0.0)),
            LayerKind::Dialog => PanelSpec::child(panel, false, build)
                .without_dismiss()
                .without_nav_bar()
                .offset(at(0.15, 0.0, 0.0)),
            LayerKind::Hud => PanelSpec::child(panel, false, build)
                .hud()
                .without_nav_bar(),
            LayerKind::Far => PanelSpec::spawn(panel, build)
                .offset(at(0.45, 0.1, -0.3))
                .titled("layer_test.far"),
        };
        let scale = if kind == LayerKind::Hud {
            HUD_SCALE
        } else {
            1.0
        };
        spec.sized(size * scale, SAMPLE_PX)
    }
}

impl Render for LayerTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let me = cx.entity().downgrade();
        let buttons = LayerKind::ALL.map(|kind| {
            let me = me.clone();
            button(&theme, kind.id(), i18n::t(cx, kind.name()), false).on_click(move |_, _, cx| {
                let Some(spec) = me
                    .read_with(cx, |t, _| {
                        let overs = t.samples.iter().filter(|(k, _)| *k == LayerKind::Over);
                        t.sample(kind, overs.count(), me.clone())
                    })
                    .ok()
                else {
                    return;
                };
                cx.default_global::<PanelControl>().spawns.push(spec)
            })
        });
        let clear = {
            let me = me.clone();
            button(
                &theme,
                "layer-clear",
                i18n::t(cx, "layer_test.clear"),
                false,
            )
            .on_click(move |_, _, cx| {
                let samples = me
                    .update(cx, |t, _| std::mem::take(&mut t.samples))
                    .unwrap_or_default();
                for (_, id) in samples {
                    ui::command(id, PanelCommand::Close, cx);
                }
            })
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .pt(px(TOP_SPACE))
            .text_color(rgba(theme.neutral_text))
            .child(row().children(buttons).child(clear))
            .child(
                div()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "layer_test.hint")),
            )
    }
}

/// [`LayerTest`] が出す見本。種類と、確定した描画順（大きいほど手前）・手前にずらした量を表示する。
pub struct LayerSample {
    panel: PanelId,
    kind: LayerKind,
    /// 同じ種類で何枚目か。
    number: usize,
}

impl Render for LayerSample {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let layer = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map_or_else(Default::default, |p| p.layer);
        let title = format!("{} {}", i18n::t(cx, self.kind.name()), self.number);
        let readout = i18n::tf(
            cx,
            "layer_test.readout",
            &[
                ("order", &layer.order),
                ("lifted", &format!("{:.1}", layer.lifted * 1000.0)),
            ],
        );
        // 閉じるボタンは左上に置く（右下へずらして重ねた見本の下になっても、左上は見えて押せる）。
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .p(px(spacing::SM))
            .bg(rgba(theme.control))
            .text_color(rgba(theme.neutral_text))
            .child(
                row()
                    .when(self.kind != LayerKind::Hud, |top| {
                        top.child(
                            button(
                                &theme,
                                "layer-close",
                                i18n::t(cx, "layer_test.close"),
                                false,
                            )
                            .on_click(command(self.panel, PanelCommand::Close)),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(font_size::LG))
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "layer-readout".into())
                    .child(readout),
            )
    }
}
