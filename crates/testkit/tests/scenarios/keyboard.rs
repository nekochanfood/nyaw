//! 表示されたキーをレーザー / novr のポインタで押して入力欄まで通す。
use gpui::Context;
use kernel::subsystems::keyboard::{Database, KeyboardType, Layout};
use testing::KeyboardTest;
use testkit::{Driver, Vec2, VrKit, on_every_runtime};

fn screen(layout: &str, variant: &str, cx: &mut Context<KeyboardTest>) -> KeyboardTest {
    // 非同期ワーカーの速度に依存せず、同じ初期配列から画面操作を始める。
    let db = Database::load().unwrap();
    let layout = Layout::new(layout, variant);
    let keyboard = db.keyboard(&layout).unwrap();
    let view = KeyboardTest::new(0, cx);
    view.keyboard.update(cx, |view, _| {
        view.automatic = false;
        view.select_layout(layout);
        view.keyboard = Some(keyboard);
        view.loading = false;
        view.kind = KeyboardType::Compact;
    });
    view
}

fn normal_shift_and_standalone_tab(kit: &mut impl Driver<KeyboardTest>) {
    kit.click("keyboard-test-field")
        .scroll("keyboard-test-scroll", -30.);
    kit.click("key-AC01").click("key-LFSH").click("key-AC01");
    assert_eq!(kit.read(|view| view.text.clone()), "aA");
    kit.click("key-TAB");
    assert_eq!(kit.read(|view| view.text.clone()), "aA\t");
    kit.click("key-TAB");
    assert_eq!(kit.read(|view| view.text.clone()), "aA\t\t");
}
on_every_runtime!(normal_shift_and_standalone_tab, |_, cx| screen(
    "us", "", cx
));

#[gpui::test]
fn keyboard_tooltip_disappears_after_leaving_a_key(cx: &mut gpui::TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| screen("jp", "", cx));
    kit.click("keyboard-test-field")
        .scroll("keyboard-test-scroll", -30.);
    kit.hover("key-HENK");
    assert_eq!(
        kit.panel_control(|c| c.tooltip.get(&kernel::Hand::Right).map(|t| t.to_string()))
            .as_deref(),
        Some("Convert")
    );
    kit.hover("key-AC01");
    assert_eq!(
        kit.panel_control(|c| c.tooltip.get(&kernel::Hand::Right).map(|t| t.to_string())),
        None
    );
    kit.hover("key-HENK");
    kit.point_at(Vec2::new(2.0, 2.0));
    assert_eq!(
        kit.panel_control(|c| c.tooltip.get(&kernel::Hand::Right).map(|t| t.to_string())),
        None
    );
    kit.click("key-HENK").hover("key-AC01");
    assert_eq!(
        kit.panel_control(|c| c.tooltip.get(&kernel::Hand::Right).map(|t| t.to_string())),
        None
    );
    kit.hand(kernel::Hand::Right).hover("key-HENK");
    kit.hand(kernel::Hand::Left).hover("key-MUHE");
    kit.hover("key-AC01");
    assert_eq!(
        kit.panel_control(|c| c.tooltip.get(&kernel::Hand::Left).map(|t| t.to_string())),
        None
    );
    assert_eq!(
        kit.panel_control(|c| c.tooltip.get(&kernel::Hand::Right).map(|t| t.to_string()))
            .as_deref(),
        Some("Convert")
    );
}

fn altgr_matches_the_german_key_label(kit: &mut impl Driver<KeyboardTest>) {
    kit.click("keyboard-test-field")
        .scroll("keyboard-test-scroll", -30.);
    kit.click("key-RALT").click("key-AD01").click("key-RALT");
    assert_eq!(kit.read(|view| view.text.clone()), "@");
}
on_every_runtime!(altgr_matches_the_german_key_label, |_, cx| screen(
    "de", "", cx
));

fn dvorak_uses_the_chosen_variant(kit: &mut impl Driver<KeyboardTest>) {
    kit.click("keyboard-test-field")
        .scroll("keyboard-test-scroll", -30.);
    kit.click("key-AC01").click("key-AC02").click("key-AC03");
    assert_eq!(kit.read(|view| view.text.clone()), "aoe");
}
on_every_runtime!(dvorak_uses_the_chosen_variant, |_, cx| screen(
    "us", "dvorak", cx
));

fn japanese_kana_uses_jis_positions(kit: &mut impl Driver<KeyboardTest>) {
    kit.click("keyboard-test-field")
        .scroll("keyboard-test-scroll", -30.);
    kit.click("key-AD01").click("key-AD02").click("key-AD03");
    assert_eq!(kit.read(|view| view.text.clone()), "タテイ");
}
on_every_runtime!(japanese_kana_uses_jis_positions, |_, cx| screen(
    "jp", "kana", cx
));
