//! ウィザード: ユーザーに順番に操作させる画面。どこでも使えるよう、状態（[`Wizard`]）と見た目（[`wizard_welcome`] / [`wizard_step`]）を分ける。
//! 画面は中身だけ（地は無い）。パネルの地（[`wizard_panel`]。背景色）と `navigation`（右から入る遷移・スワイプバック）で包んで使う。
//! 見た目は全画面で揃える: 縁の余白は [`WIZARD_MARGIN`]、内容は上下中央寄せ、ボタンは下の帯（フッター。右が先へ進む操作、左がそのほか）。

use design_prelude::*;

mod design_prelude {
    pub use crate::components::{Button, Variant, content_button};
    pub use crate::{Icon, Theme, font_size, spacing};
    pub use gpui::{
        Div, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div,
        px, rgba,
    };
}

/// 画面の縁から内容・フッターまでの余白（全画面で共通）。
pub const WIZARD_MARGIN: f32 = spacing::XL;
/// 「step」画面の内容（見出しと中身）の最大の幅。横長のパネルでは、この幅の列を左右中央に置く（行が間延びしない）。
pub const WIZARD_CONTENT: f32 = 704.0;
// アイコンの大きさと行の高さはダイアログと同じ（画面の種類で見た目を揃える）。
use crate::components::{
    BUTTON_HEIGHT, BUTTON_PADDING, DIALOG_ICON as ICON, DIALOG_LEADING as LEADING,
};
/// 「次へ」「戻る」のアイコンの大きさ。
const ARROW: f32 = font_size::LG;
/// 「次へ」「戻る」の最小の幅。文字が短くても、押しやすい幅を保つ。
const ARROW_BUTTON_MIN: f32 = BUTTON_HEIGHT * 2.5;

/// ウィザードの進み具合（0 始まりの現在の手順と全体の数）。UI を持たないので単体でテストできる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wizard {
    step: usize,
    len: usize,
}

impl Wizard {
    /// `len` 個の手順（1 以上）。
    pub fn new(len: usize) -> Self {
        Self {
            step: 0,
            len: len.max(1),
        }
    }
    pub fn step(&self) -> usize {
        self.step
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn is_first(&self) -> bool {
        self.step == 0
    }
    pub fn is_last(&self) -> bool {
        self.step + 1 == self.len
    }
    /// 次へ。最後なら動かず `false`（呼び出し側が完了として扱う）。
    // 手順を進める操作で、イテレータの `next` ではない。
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> bool {
        let ok = !self.is_last();
        if ok {
            self.step += 1;
        }
        ok
    }
    /// 前へ。最初なら動かず `false`。
    pub fn back(&mut self) -> bool {
        let ok = !self.is_first();
        if ok {
            self.step -= 1;
        }
        ok
    }
}

/// ウィザードのパネル: 地（`background`。設定画面の中身の側と同じ）のパネル。画面にはカードや一覧をそのまま並べられる。
/// 中の画面（[`navigation`](crate::components::navigation) の四角い地）は、パネルの角丸で切る（切らないと角丸を覆う）。
pub fn wizard_panel(theme: &Theme) -> Div {
    crate::components::panel(theme)
        .p(px(0.0))
        .overflow_hidden()
        .clip_rounded()
}

/// 「次へ」ボタン（文字 + 右の caret）。ポジティブな意味のボタンは青（`Variant::Primary` = accent）。完了・開始などにも使う。id は呼び出し側が決める。
pub fn wizard_next(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
) -> Button {
    arrow_button(
        theme,
        id,
        label,
        &crate::icons::CARET_RIGHT,
        true,
        Variant::Primary,
    )
}

/// 「戻る」ボタン（左の caret + 文字）。
pub fn wizard_back(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
) -> Button {
    arrow_button(
        theme,
        id,
        label,
        &crate::icons::CARET_LEFT,
        false,
        // 地に直接置くので、背景用の色。
        Variant::OnBackground,
    )
}

fn arrow_button(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    glyph: &'static Icon,
    after: bool,
    variant: Variant,
) -> Button {
    let arrow = crate::icon(glyph)
        .weight(crate::Weight::Bold)
        .size(px(ARROW));
    // 文字の側は文字のボタンと同じ余白、矢印の側は少し詰める（矢印は自分の余白を持つ）。端は正円のカプセル。
    let (text, icon) = (px(BUTTON_PADDING), px(spacing::XL));
    let b = content_button(theme, id, variant)
        .min_w(px(ARROW_BUTTON_MIN))
        .circular_corners();
    if after {
        b.pl(text).pr(icon).child(label.into()).child(arrow)
    } else {
        b.pl(icon).pr(text).child(arrow).child(label.into())
    }
}

fn heading(
    theme: &Theme,
    id: &str,
    hero: impl IntoElement,
    title: SharedString,
    description: SharedString,
    centered: bool,
) -> Div {
    let col = div().flex().flex_col().gap(px(spacing::SM));
    let col = if centered {
        col.items_center().text_center()
    } else {
        col
    };
    let (hero_id, title_id) = (format!("{id}-hero"), format!("{id}-title"));
    let hero = div().debug_selector(move || hero_id).child(hero);
    // タイトルも説明も無い画面（ロゴだけの一枚目）では、下の文との間を空けない。空のタイトル・説明は出さない。
    let hero = if title.is_empty() && description.is_empty() {
        hero
    } else {
        hero.mb(px(spacing::SM))
    };
    col.child(hero)
        .children((!title.is_empty()).then(|| {
            div()
                .id(SharedString::from(format!("{id}-title")))
                .debug_selector(move || title_id)
                .text_size(px(font_size::LG))
                .line_height(px(font_size::LG * LEADING))
                .font_weight(FontWeight::BOLD)
                .child(title)
        }))
        .children((!description.is_empty()).then(|| {
            let description_id = format!("{id}-description");
            div()
                .id(SharedString::from(description_id.clone()))
                .debug_selector(move || description_id)
                .text_color(rgba(theme.neutral_text_low))
                .child(description)
        }))
}

/// 画面の上に出すアイコン（アクセント色。大きさは全画面で共通）。ロゴなど別のものを出すときは `hero` に直接渡す。
pub fn wizard_icon(theme: &Theme, glyph: &'static Icon) -> impl IntoElement {
    crate::icon(glyph)
        .size(px(ICON))
        .color(rgba(theme.accent_solid))
}

/// 下の帯（`{id}-footer`）: 右に先へ進む操作 `next`、左にそのほか（戻る・言語の選択など）`other`。無ければ左は空き。
fn footer(id: &str, other: Option<Button>, next: Button) -> gpui::Stateful<Div> {
    div()
        .id(SharedString::from(format!("{id}-footer")))
        .w_full()
        .flex()
        .justify_between()
        .child(other.map_or_else(|| div().into_any_element(), IntoElement::into_any_element))
        .child(next)
}

/// 「welcome」画面（完了画面にも使える）: アイコン（やロゴ）・タイトル・説明を画面の上下左右中央に置き、ボタンは「step」と
/// 同じ下の帯（右に先へ進む `next`、左にそのほかの `other`）。タイトル・説明は、空なら出さない。
pub fn wizard_welcome(
    theme: &Theme,
    id: &str,
    hero: impl IntoElement,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    other: Option<Button>,
    next: Button,
) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .p(px(WIZARD_MARGIN))
        // 上の空きと下の帯の枠が同じだけ伸びるので、内容は画面の上下中央に来る。
        .child(div().flex_1())
        .child(heading(
            theme,
            id,
            hero,
            title.into(),
            description.into(),
            true,
        ))
        .child(
            div()
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .justify_end()
                .child(footer(id, other, next)),
        )
}

/// 「step」画面: 内容（アイコン・タイトル・説明・`body` のフォームなど）を上下中央寄せ（幅は [`WIZARD_CONTENT`] まで）にし、下に左「戻る」・右「次へ」のフッター。
/// 最初の手順は `back` を `None` にする（フッターの左は空き）。
#[allow(clippy::too_many_arguments)]
pub fn wizard_step(
    theme: &Theme,
    id: &str,
    hero: impl IntoElement,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    body: impl IntoElement,
    back: Option<Button>,
    next: Button,
) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .p(px(WIZARD_MARGIN))
        .gap(px(spacing::XL))
        .child(
            div()
                .id(SharedString::from(format!("{id}-content")))
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(WIZARD_CONTENT))
                .self_center()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(spacing::XL))
                .child(heading(
                    theme,
                    id,
                    hero,
                    title.into(),
                    description.into(),
                    false,
                ))
                .child(body),
        )
        .child(footer(id, back, next).flex_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_forward_and_back_within_bounds() {
        let mut w = Wizard::new(3);
        assert!(w.is_first() && !w.back());
        assert!(w.next() && w.next());
        assert!(w.is_last() && !w.next());
        assert_eq!(w.step(), 2);
        assert!(w.back() && w.step() == 1);
        assert_eq!(Wizard::new(0).len(), 1);
    }
}
