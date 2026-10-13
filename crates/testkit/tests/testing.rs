//! Testing 画面の操作。Testing は一連の機能を1画面で試せるテスト用の画面で、
//! 基本操作はどのランタイムでもネイティブに行えるべきなので、シナリオは1回だけ書いて全ランタイム（novr / VR）で実行する。

use gpui::TestAppContext;
use testing::{
    BUTTON_ACTIVE_BG, BUTTON_BG, BUTTON_HOVER_BG, DROP_TARGET_BG, FIELD_FOCUS_BG, Testing,
};
use testkit::{Driver, NoVrKit, on_every_runtime};

fn click_button(kit: &mut impl Driver<Testing>) {
    kit.click("click-me").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 2);
}
on_every_runtime!(click_button, |_, _| Testing::default());

fn hover_highlights_button(kit: &mut impl Driver<Testing>) {
    assert_eq!(kit.color_at("click-me"), BUTTON_BG);
    kit.hover("click-me");
    assert_eq!(kit.color_at("click-me"), BUTTON_HOVER_BG);
}
on_every_runtime!(hover_highlights_button, |_, _| Testing::default());

/// ロックはボタンで切り替わり、有効な間はボタンの色で分かる。
fn lock_button_toggles(kit: &mut impl Driver<Testing>) {
    kit.click("lock").hover("click-me");
    assert_eq!(kit.color_at("lock"), BUTTON_ACTIVE_BG);
    kit.click("lock").hover("click-me");
    assert_eq!(kit.color_at("lock"), BUTTON_BG);
}
on_every_runtime!(lock_button_toggles, |_, _| Testing::default());

/// 取り付け先はどれか1つだけ光り、外すと消える。
/// （操作している右手に取り付けるとレーザーの先にパネルが付いてきて他を指せないので、右手は attach.rs で左手から操作する）
fn attach_buttons_show_target(kit: &mut impl Driver<Testing>) {
    kit.click("attach-hmd")
        .click("attach-left")
        .hover("click-me");
    assert_eq!(kit.color_at("attach-hmd"), BUTTON_BG);
    assert_eq!(kit.color_at("attach-left"), BUTTON_ACTIVE_BG);
    kit.click("detach").hover("click-me");
    assert_eq!(kit.color_at("attach-left"), BUTTON_BG);
}
on_every_runtime!(attach_buttons_show_target, |_, _| Testing::default());

// ---- GPUI の部品 ----

fn toggle_switches_and_shows_state(kit: &mut impl Driver<Testing>) {
    kit.click("toggle").hover("click-me");
    assert!(kit.read(|h| h.toggled));
    assert_eq!(kit.color_at("toggle"), BUTTON_ACTIVE_BG);
    kit.click("toggle").hover("click-me");
    assert!(!kit.read(|h| h.toggled));
    assert_eq!(kit.color_at("toggle"), BUTTON_BG);
}
on_every_runtime!(toggle_switches_and_shows_state, |_, _| Testing::default());

/// `.active()`: 押している間だけ見た目が変わる。
fn pressed_style_while_held(kit: &mut impl Driver<Testing>) {
    kit.down("hold-me");
    assert_eq!(kit.color_at("hold-me"), BUTTON_ACTIVE_BG);
    kit.up();
    assert_eq!(kit.color_at("hold-me"), BUTTON_HOVER_BG);
}
on_every_runtime!(pressed_style_while_held, |_, _| Testing::default());

/// `.on_drag()` / `.drag_over()` / `.on_drop()`: 運んでいる間はドロップ先が光り、離すと届く。
fn drag_and_drop(kit: &mut impl Driver<Testing>) {
    kit.down("drag-chip").hover("drop-zone");
    assert_eq!(kit.color_at("drop-zone"), DROP_TARGET_BG);
    kit.up();
    assert_eq!(kit.read(|h| h.dropped), 1);
    assert_ne!(kit.color_at("drop-zone"), DROP_TARGET_BG);
}
on_every_runtime!(drag_and_drop, |_, _| Testing::default());

/// `.on_scroll_wheel()` と `.overflow_y_scroll()`: スクロール量が届き、一覧が実際に動く。
fn scroll_moves_list(kit: &mut impl Driver<Testing>) {
    kit.scroll("scroll-list", 1.0).scroll("scroll-list", -3.0);
    assert_eq!(kit.read(|h| h.scrolled), -2.0);
    assert!(
        kit.read(|h| h.list.offset().y) < gpui::px(0.0),
        "下へ動いている"
    );
}
on_every_runtime!(scroll_moves_list, |_, _| Testing::default());

/// `deferred(anchored())`: ポップアップを開いて項目を選ぶと閉じる。外を押しても閉じる。
fn popup_menu_picks_item(kit: &mut impl Driver<Testing>) {
    kit.click("open-menu").click("menu-b");
    assert_eq!(
        kit.read(|h| (h.picked, h.menu_open)),
        (Some("menu-b"), false)
    );
    kit.click("open-menu");
    assert!(kit.read(|h| h.menu_open));
    kit.click("click-me");
    assert!(!kit.read(|h| h.menu_open));
}
on_every_runtime!(popup_menu_picks_item, |_, _| Testing::default());

/// `.track_focus()` / `.focus()`: 押した入力欄にフォーカスが移り、見た目で分かる。
fn click_focuses_field(kit: &mut impl Driver<Testing>) {
    kit.click("text-field").hover("click-me");
    assert_eq!(kit.color_at("text-field"), FIELD_FOCUS_BG);
}
on_every_runtime!(click_focuses_field, |_, _| Testing::default());

/// `.on_key_down()`: キーボードは novr にしかない。
#[gpui::test]
fn typing_goes_to_focused_field(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.type_keys("x"); // フォーカスが無いうちは入らない
    kit.click("text-field").type_keys("h i space o k backspace");
    assert_eq!(kit.read(|h| h.text.clone()), "hi o");
}

/// UI のロジックだけの確認なので novr で十分。
#[gpui::test]
fn hover_alone_does_not_click(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.hover("click-me");
    assert_eq!(kit.read(|h| h.clicks), 0);
}
