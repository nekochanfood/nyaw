use super::*;
use crate::test_util::*;

/// ローカライズの画面: 言語のボタンが切り替えを要求し、言語が変わると文言が訳されて長さも変わる。
#[gpui::test]
fn locale_screen_switches_language_and_text_follows(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| LocaleTest::new(0));
    cx.simulate_resize(gpui::size(px(1200.0), px(1000.0)));
    cx.run_until_parked();
    let width = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("locale-sample-button").unwrap().size.width
    };
    let english = width(cx);
    let c = cx.debug_bounds("lang-ja").unwrap().center();
    cx.simulate_click(c, Modifiers::none());
    let queued = cx.update(|_, cx| cx.global::<PanelControl>().setting_changes.clone());
    assert_eq!(queued, [Setting::Language(Language::Ja)]);
    // フレームループ（ui::sync_settings）の代わりに、設定とグローバルの言語を反映する。
    cx.update(|window, cx| {
        cx.default_global::<PanelControl>().settings.language = Language::Ja;
        cx.set_global(i18n::Lang::Ja);
        window.refresh();
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| i18n::t(cx, "locale.sample_button")),
        "変更を保存"
    );
    assert_ne!(
        width(cx),
        english,
        "the button is laid out with the translation"
    );
}
