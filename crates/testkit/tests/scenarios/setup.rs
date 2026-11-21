use gpui::{TestAppContext, px};
use kernel::{DominantHand, Language, PanelCommand, ThemeMode, WristDesign};
use setup::{SetupWizard, Step};
use testing::{BACKGROUND, PANEL_BG, SetupWizardSpawn, Testing};
use testkit::{Action, Driver, NoVrKit, Vec2, VrKit};

/// The wizard walks through every decision. Each choice applies at once, and only Finish marks the setup done.
#[gpui::test]
fn setup_decides_each_setting_and_marks_done(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| SetupWizard::new(0));
    assert!(!kit.kernel.settings().setup_done);
    // 起動仕様（`setup::spec`）と同様に、表示時点からロックダウンを有効にする。表示中は閉じることができない。
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.step();
    assert!(kit.kernel.settings().lockdown);
    kit.kernel.apply(0, PanelCommand::Close);
    assert!(!kit.kernel.panel(0).closed);

    // 1 ページ目: 言語は左下のドロップダウンで選択し、即座に反映される（「次へ」の押下を待たない）。
    assert_eq!(kit.read(|v| v.step()), Step::Welcome);
    kit.click("setup-language");
    assert!(kit.read(|v| v.language_open));
    kit.click("setup-lang-ja");
    assert_eq!(kit.kernel.settings().language, Language::Ja);
    assert!(!kit.read(|v| v.language_open), "選択後にドロップダウンが閉じる");
    kit.click("setup-next");

    // 利き手の選択。前のページに戻っても選択状態は保持される。
    assert_eq!(kit.read(|v| v.step()), Step::Hand);
    kit.click("setup-hand-left");
    assert_eq!(kit.kernel.settings().dominant_hand, DominantHand::Left);
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::Welcome);
    kit.click("setup-next").click("setup-next");
    assert_eq!(kit.kernel.settings().dominant_hand, DominantHand::Left);

    // アクセシビリティ: 外観の前に視認性を設定する（設定画面と共通の部品。上段にプレビュー、下段に同等の 3 項目）。
    assert_eq!(kit.read(|v| v.step()), Step::Accessibility);
    let [sample, text, ui, bold] = [
        "a11y-preview",
        "setting-text-size",
        "setting-ui-scale",
        "setting-bold-text",
    ]
    .map(|s| kit.bounds(s));
    assert!(
        sample.bottom() <= text.top() && text.bottom() <= ui.top() && ui.bottom() <= bold.top()
    );
    assert_eq!((text.size, ui.size), (ui.size, bold.size));
    kit.click("setting-bold-text");
    assert!(kit.kernel.settings().bold_text, "選択が即座に反映される");
    kit.click("setting-bold-text");
    assert!(!kit.kernel.settings().bold_text);
    kit.click("setup-next");

    // 外観: テーマとパーソナルカラーを 1 ページで設定する（設定画面と共通の部品。上からプレビュー・テーマ・色）。
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    let [preview, theme, colors] =
        ["theme-preview", "setting-theme", "accent-custom"].map(|s| kit.bounds(s));
    assert!(preview.bottom() <= theme.top() && theme.bottom() <= colors.top());
    kit.click("setting-theme").click("setting-theme-light");
    assert_eq!(kit.kernel.settings().theme, ThemeMode::Light);
    assert_eq!(kit.kernel.settings().personal_color, None);
    kit.click("accent-pink");
    let pink = kit.kernel.settings().personal_color.expect("picked");
    assert_eq!(pink.hue, 350.0);
    kit.click("setup-next");

    // 文字盤: 送りボタンだけでは変更されず、「設定する」で確定する。種類での絞り込みも可能。
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
    kit.click("watch-next");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Dots);
    kit.click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Ring);
    kit.click("watch-kind")
        .click("watch-kind-analog")
        .click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Minimal);
    // novr モードでは SteamVR のステップは表示されず、文字盤の次が完了画面になる（戻る際も省略される）。
    kit.click("setup-next");

    // 完了画面からも戻ることができる（左下の「戻る」。「完了」は右下）。「完了」を押すまではセットアップ未完了のまま。
    assert!(kit.read(|v| v.wizard.is_last()));
    let (finish, back) = (kit.bounds("setup-finish"), kit.bounds("setup-back"));
    assert!(back.right() < finish.left() && back.bottom() == finish.bottom());
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
    kit.click("setup-next");
    assert!(kit.read(|v| v.wizard.is_last()));
    kit.step();
    assert!(!kit.kernel.settings().setup_done);
    kit.click("setup-finish").step();
    assert!(kit.kernel.settings().setup_done);
    assert!(!kit.kernel.settings().lockdown, "完了後にロックダウンが解除される");
    assert!(kit.kernel.panel(0).closed);
}

/// On OpenVR, a step after the watch face asks whether nyaw starts with SteamVR. It stays off until chosen.
#[gpui::test]
fn openvr_asks_whether_to_launch_with_steamvr(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    // OpenVR バックエンドのみが自動起動の切り替えに対応する（モックで OpenVR を模倣）。
    kit.kernel.subsystems.backend.auto_launch = Some(false);
    kit.step();
    for _ in 0..5 {
        kit.click("setup-next");
    }
    assert_eq!(kit.read(|v| v.step()), Step::AutoLaunch);
    kit.click("setup-auto-launch-on").step();
    assert_eq!(kit.kernel.settings().auto_launch, Some(true));
    kit.click("setup-auto-launch-off").step();
    assert_eq!(kit.kernel.settings().auto_launch, Some(false));
    kit.click("setup-next");
    assert!(kit.read(|v| v.wizard.is_last()));
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::AutoLaunch);
}

/// The first screen: the logo, the title and one line sit in the middle of the screen. The buttons are in the
/// bottom corners like on a step: Next at the right; at the left, the power button and then the language dropdown,
/// opening upward.
#[gpui::test]
fn welcome_centers_the_logo_over_next_and_the_language_in_the_corners(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    let panel = kit.bounds("setup-nav");
    let [hero, title, about, next, quit, language] = [
        "setup-hero",
        "setup-title",
        "setup-description",
        "setup-next",
        "setup-quit",
        "setup-language",
    ]
    .map(|s| kit.bounds(s));
    let near = |a: gpui::Pixels, b: gpui::Pixels| (a - b).abs() < px(2.0);
    // ロゴ・タイトル・説明のグループが画面の上下左右中央に配置される。ロゴは大きいサイズで表示。
    for part in [hero, title, about] {
        assert!(near(part.center().x, panel.center().x), "{part:?}");
    }
    assert!(near((hero.top() + about.bottom()) / 2.0, panel.center().y));
    assert!(hero.size.height >= px(160.0), "{:?}", hero.size);
    assert!(hero.bottom() <= title.top() && title.bottom() <= about.top());
    // 「次へ」は右下に配置。左下に終了ボタンとその右に言語ドロップダウン（同一高さ）。
    assert!(about.bottom() < next.top() && language.right() < next.left());
    assert!(next.right() > panel.center().x && language.left() < panel.center().x);
    assert!(quit.right() < language.left());
    assert_eq!(next.bottom(), language.bottom());
    assert_eq!(quit.bottom(), language.bottom());
    assert!(near(
        panel.right() - next.right(),
        quit.left() - panel.left()
    ));
    // ドロップダウンは上方向に展開する（下端が画面の縁に近いため）。左端はボタンに揃い、画面外にはみ出さない。
    kit.click("setup-language").step();
    let options = ["setup-lang-en", "setup-lang-ja"].map(|s| kit.bounds(s));
    let language = kit.bounds("setup-language");
    assert!(options.iter().all(|o| o.bottom() <= language.top()));
    assert!(options.iter().all(|o| o.left() >= language.left()));
    // 外部クリックで、未選択のまま閉じる。
    kit.click("setup-title");
    assert!(!kit.read(|v| v.language_open));
    assert_eq!(kit.read(|v| v.step()), Step::Welcome);
}

/// The power button on the first screen asks in a dialog, the wizard's child (even in lockdown), before it quits the app.
/// Cancel only closes the dialog.
#[gpui::test]
fn setup_asks_before_it_quits_the_app(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.click("setup-quit").step().step();
    let dialog = 1;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));
    let tint = kit.kernel.subsystems.backend.overlays[0].tint;
    assert!(tint < 1.0, "the wizard behind is dimmed");
    kit.panel(dialog).click("quit-cancel").step().step();
    assert!(kit.kernel.panel(dialog).closed);
    assert!(!kit.panel_control(|c| c.quit));
    assert!(!kit.kernel.panel(0).closed && kit.kernel.settings().lockdown);
    kit.panel(0).click("setup-quit").step().step();
    let dialog = 2;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));
    kit.panel(dialog).click("quit-confirm");
    assert!(kit.panel_control(|c| c.quit));
    assert!(
        !kit.kernel.settings().setup_done,
        "the wizard comes back next time"
    );
}

/// On the appearance step, the dialog that makes a custom color opens as the wizard's child, even in lockdown.
#[gpui::test]
fn setup_makes_a_custom_color_in_a_child_dialog(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.click("setup-next")
        .click("setup-next")
        .click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    kit.click("accent-custom").step().step();
    let dialog = 1;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));
    let tint = kit.kernel.subsystems.backend.overlays[0].tint;
    assert!(tint < 1.0, "the wizard behind is dimmed");
    kit.panel(dialog).click("color-ok").step().step();
    assert!(kit.kernel.panel(dialog).closed);
    let made = kit.kernel.settings().personal_color;
    assert!(made.is_some());
    assert_eq!(kit.kernel.settings().color_history[0], made);
    assert!(
        kit.kernel.settings().lockdown,
        "the wizard keeps the lockdown"
    );
    kit.panel(0).click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
}

/// The wizard's panel has the backdrop color of the Settings content side, so the shared parts sit on it in their
/// own colors (cards on the backdrop) with no box around them.
#[gpui::test]
fn wizard_panel_is_the_backdrop_with_the_parts_in_their_own_colors(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    kit.click("setup-next")
        .click("setup-next")
        .click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    let row = kit.bounds("setting-theme");
    let x = f32::from(row.left()) + 4.0;
    let inside = kit.vr_color_at_px(Vec2::new(x, f32::from(row.center().y)));
    let between = kit.vr_color_at_px(Vec2::new(x, f32::from(row.bottom()) + 8.0));
    assert_eq!(
        inside, PANEL_BG,
        "the row is a card, as on the Settings screen"
    );
    assert_eq!(between, BACKGROUND, "the panel itself is the backdrop");
    // The watch faces too: around them is the panel's backdrop.
    kit.click("setup-next");
    let faces = kit.bounds("watch-faces");
    let corner = Vec2::new(f32::from(faces.left()) + 2.0, f32::from(faces.top()) + 2.0);
    assert_eq!(kit.vr_color_at_px(corner), BACKGROUND);
}

/// Grabs the handle of slider `id` and moves it to `at` along its travel (0 = start, 1 = end), still holding.
fn grab_to(kit: &mut VrKit<'_, SetupWizard>, id: &'static str, handle: &'static str, at: f32) {
    let (h, s) = (kit.bounds(handle), kit.bounds(id));
    let from = Vec2::new(f32::from(h.center().x), f32::from(h.center().y));
    let travel = f32::from(s.size.width - h.size.width);
    let to = f32::from(s.left() + h.size.width / 2.0) + at * travel;
    kit.point_at(from).press(Action::Select);
    for i in 1..=5 {
        kit.point_at(from + Vec2::new((to - from.x) * i as f32 / 5.0, 0.0));
    }
    kit.step();
}

/// The accessibility step comes right before the appearance. Text size applies while dragging; the UI scale would
/// move the wizard itself under the laser, so it only zooms the sample while dragging and applies on release.
/// Whatever the two are set to, the wizard still fits its panel and goes on.
#[gpui::test]
fn setup_adjusts_text_size_and_ui_scale_before_the_appearance(cx: &mut TestAppContext) {
    use kernel::{TEXT_SCALE_MAX, UI_SCALE_MAX};
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.click("setup-next").click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Accessibility);
    let (next, back) = (kit.bounds("setup-next"), kit.bounds("setup-back"));
    let preview = kit.bounds("a11y-preview");
    let sample = kit.bounds("a11y-sample");

    // 文字サイズ: ドラッグ中も即座に反映される。プレビューは固定サイズの枠内で拡大される。見出しと説明も拡大されるため枠の
    // 上下位置は変化するが、スライダーの水平位置とフッター（「戻る」「次へ」）は動かない。
    let text = (
        "setting-text-size-slider",
        "setting-text-size-slider-handle",
    );
    let slider = kit.bounds(text.0);
    grab_to(&mut kit, text.0, text.1, 1.0);
    assert_eq!(kit.kernel.settings().text_scale, TEXT_SCALE_MAX);
    let held = kit.bounds(text.0);
    assert_eq!((held.left(), held.size), (slider.left(), slider.size));
    kit.release(Action::Select).step();
    let (grown, around) = (kit.bounds("a11y-sample"), kit.bounds("a11y-preview"));
    assert!(grown.size.height > sample.size.height);
    assert!(grown.top() >= around.top() && grown.bottom() <= around.bottom());
    assert_eq!(around.size, preview.size);
    // フッターのボタンは文字サイズに応じて横方向に拡大されるが、上下の位置と高さは維持される（左右端に寄せたまま）。
    let footer = (kit.bounds("setup-next"), kit.bounds("setup-back"));
    assert_eq!(
        (footer.0.top(), footer.0.size.height),
        (next.top(), next.size.height)
    );
    assert_eq!(
        (footer.0.right(), footer.1.left()),
        (next.right(), back.left())
    );
    assert_eq!(footer.1.top(), back.top());

    // UI スケール: リリースするまでウィザード自体のサイズは変更せず、リリース時に拡大する（内部レイアウトは同一）。
    let (size, resolution) = (kit.kernel.panel(0).size, kit.kernel.panel(0).resolution);
    let ui = ("setting-ui-scale-slider", "setting-ui-scale-slider-handle");
    grab_to(&mut kit, ui.0, ui.1, 1.0);
    assert_eq!(kit.kernel.settings().ui_scale, 1.0);
    assert_eq!(kit.kernel.panel(0).size, size);
    kit.release(Action::Select).step().step();
    assert_eq!(kit.kernel.settings().ui_scale, UI_SCALE_MAX);
    assert!(
        kit.kernel
            .panel(0)
            .size
            .abs_diff_eq(size * UI_SCALE_MAX, 1e-5)
    );
    assert_eq!(kit.kernel.panel(0).resolution, resolution);
    // パネルサイズが拡大されても、内部レイアウトは 1px も変化しない。
    assert_eq!((kit.bounds("setup-next"), kit.bounds("setup-back")), footer);
    assert!(
        kit.kernel.settings().lockdown,
        "the wizard keeps the lockdown"
    );

    // 文字サイズ・UI スケールの両方を最大にしても、項目はパネル内に収まり次へ進める（次のステップは外観）。
    let panel = kit.bounds("setup-nav");
    for row in ["setting-text-size", "setting-ui-scale", "setting-bold-text"] {
        let b = kit.bounds(row);
        assert!(
            b.left() >= panel.left() && b.right() <= panel.right(),
            "{row}"
        );
        assert!(b.bottom() <= next.top(), "{row} stays above the footer");
    }
    kit.click("setting-bold-text");
    assert!(kit.kernel.settings().bold_text);
    kit.click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::Accessibility);
    let s = kit.kernel.settings();
    assert_eq!(
        (s.text_scale, s.ui_scale, s.bold_text),
        (TEXT_SCALE_MAX, UI_SCALE_MAX, true),
        "前のページに戻っても選択状態は保持される"
    );
}

#[gpui::test]
fn testing_can_retry_the_setup_wizard(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| {
        cx.set_global(SetupWizardSpawn(setup::spawn_spec));
        Testing::default()
    });
    kit.click("setup-wizard");
    assert!(!kit.kernel.panel(1).closed);
}
