//! ウィザード: ユーザーに順番に操作させる画面。どこでも使えるよう、状態（[`Wizard`]）と見た目（[`wizard_welcome`] / [`wizard_step`]）を分ける。
//! 画面は中身だけ（地は無い）。パネルの地（`card_panel`）と `navigation`（右から入る遷移・スワイプバック）で包んで使う。
//! 見た目は全画面で揃える: 縁の余白は [`WIZARD_MARGIN`]、内容は上下中央寄せ、ナビゲーションのボタンは下の帯（フッター）。

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
// アイコンの大きさと行の高さはダイアログと同じ（画面の種類で見た目を揃える）。
use crate::components::{DIALOG_ICON as ICON, DIALOG_LEADING as LEADING};
/// 「次へ」「戻る」の最小幅（文字とアイコンの間に隙間ができる幅）と、アイコンの大きさ。
const NAV_BUTTON_MIN_WIDTH: f32 = 140.0;
const ARROW: f32 = font_size::LG;

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

/// ウィザードのパネル: カード 1 枚のパネルで、中の画面（[`navigation`](crate::components::navigation) の地）をパネルの角丸で切る。
/// `navigation` の画面は四角い地を持つので、切らないとパネルの角丸を覆ってしまう。
pub fn wizard_panel(theme: &Theme) -> Div {
    crate::components::card_panel(theme)
        .overflow_hidden()
        .clip_rounded()
}

/// 「次へ」ボタン（文字は左、caret は右端）。ポジティブな意味のボタンは青（`Variant::Primary` = accent）。完了・開始などにも使う。id は呼び出し側が決める。
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

/// 「戻る」ボタン（caret は左端、文字は右）。
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
        Variant::Default,
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
    // 文字とアイコンは両端に寄せる（between）。最小幅があるので間に隙間ができる。
    let label = div().child(label.into());
    let b = content_button(theme, id, variant)
        .min_w(px(NAV_BUTTON_MIN_WIDTH))
        .justify_between();
    if after {
        b.child(label).child(arrow)
    } else {
        b.child(arrow).child(label)
    }
}

fn heading(
    theme: &Theme,
    id: &str,
    glyph: &'static Icon,
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
    col.child(
        div().mb(px(spacing::SM)).child(
            crate::icon(glyph)
                .size(px(ICON))
                .color(rgba(theme.accent_solid)),
        ),
    )
    .child(
        div()
            .id(SharedString::from(format!("{id}-title")))
            .text_size(px(font_size::LG))
            .line_height(px(font_size::LG * LEADING))
            .font_weight(FontWeight::BOLD)
            .child(title),
    )
    .child(
        div()
            .id(SharedString::from(format!("{id}-description")))
            .text_color(rgba(theme.neutral_text_low))
            .child(description),
    )
}

/// 「welcome」画面（完了画面にも使える）: アイコン・タイトル・説明と、その下に中央寄せのボタン。全体を上下中央に置く。
pub fn wizard_welcome(
    theme: &Theme,
    id: &str,
    glyph: &'static Icon,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    actions: impl IntoIterator<Item = Button>,
) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p(px(WIZARD_MARGIN))
        .gap(px(spacing::XL))
        .child(heading(
            theme,
            id,
            glyph,
            title.into(),
            description.into(),
            true,
        ))
        .child(div().flex().gap(px(spacing::SM)).children(actions))
}

/// 「step」画面: 内容（アイコン・タイトル・説明・`body` のフォームなど）を上下中央寄せにし、下に左「戻る」・右「次へ」のフッター。
/// 最初の手順は `back` を `None` にする（フッターの左は空き）。
pub fn wizard_step(
    theme: &Theme,
    id: &str,
    glyph: &'static Icon,
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
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(spacing::XL))
                .child(heading(
                    theme,
                    id,
                    glyph,
                    title.into(),
                    description.into(),
                    false,
                ))
                .child(body),
        )
        .child(
            div()
                .id(SharedString::from(format!("{id}-footer")))
                .flex_none()
                .flex()
                .justify_between()
                .child(back.map_or_else(|| div().into_any_element(), IntoElement::into_any_element))
                .child(next),
        )
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
