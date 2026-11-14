//! Testing パネル本体（[`Testing`]）の描画: VR・GPUI の操作と、ほかのテスト用の画面を開くボタン。

use super::*;

/// ピクセル単位のスクロール（トラックパッド等）を行に換算するときの1行の高さ。
const SCROLL_LINE_PX: f32 = 20.0;

impl Render for Testing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map(|c| c.status.clone())
            .unwrap_or_default();
        let settings = settings(cx);
        let on = |d: &Device| st.attached.as_ref() == Some(d);
        let profiling = cx.try_global::<PanelControl>().is_some_and(|c| c.profiling);
        let tracker = st.trackers.first().map(|id| Device::Tracker(id.clone()));
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let theme = Theme::current(cx);
        // この画面のボタンは現在のテーマで描く（上のモジュール関数 `button` を隠す）。
        macro_rules! button {
            ($($a:expr),+ $(,)?) => {
                themed_button(&theme, $($a),+)
            };
        }

        let theme_label = i18n::tf(
            cx,
            "testing.panel.theme",
            &[(
                "theme",
                &i18n::t(
                    cx,
                    match settings.theme {
                        kernel::ThemeMode::Dark => "theme.dark",
                        kernel::ThemeMode::Light => "theme.light",
                    },
                ),
            )],
        );
        let language_label = i18n::tf(
            cx,
            "testing.panel.language",
            &[("language", &ui::lang(settings.language).native_name())],
        );
        // このパネル自身の操作（表示・テーマ・言語・ロック・計測）。
        let panel_row = row()
            .child(
                button!(
                    "animate-panel",
                    i18n::t(cx, "testing.panel.animation"),
                    st.animated
                )
                .on_click(command(self.panel, PanelCommand::Animated(!st.animated))),
            )
            .child(
                button!("toggle-theme", theme_label, false)
                    .on_click(change(Setting::Theme(settings.theme.toggled()))),
            )
            .child(
                // 言語の名前はその言語自身で出す（どの言語で表示していても読める）。
                button!("toggle-language", language_label, false)
                    .on_click(change(Setting::Language(next_language(settings.language)))),
            )
            .child(
                button!("lock", i18n::t(cx, "testing.panel.lock"), st.locked)
                    .on_click(command(self.panel, PanelCommand::Lock(!st.locked))),
            )
            .child(
                button!(
                    "toggle-profiler",
                    i18n::t(cx, "testing.panel.profiler"),
                    profiling
                )
                .on_click(|_, _, cx| {
                    let c = cx.default_global::<PanelControl>();
                    c.profiling = !c.profiling;
                }),
            );
        // 取り付け先（今どこに付いているかで光る）。
        let attach_row = row()
            .child(
                button!(
                    "attach-hmd",
                    i18n::t(cx, "testing.attach.hmd"),
                    on(&Device::Hmd)
                )
                .on_click(command(
                    self.panel,
                    PanelCommand::Attach(Device::Hmd, attach_offset(self.panel, HMD_OFFSET)),
                )),
            )
            .child(
                button!(
                    "attach-left",
                    i18n::t(cx, "testing.attach.left_hand"),
                    on(&Device::Controller(Hand::Left)),
                )
                .on_click(command(
                    self.panel,
                    PanelCommand::Attach(
                        Device::Controller(Hand::Left),
                        attach_offset(self.panel, HAND_OFFSET),
                    ),
                )),
            )
            .child(
                button!(
                    "attach-right",
                    i18n::t(cx, "testing.attach.right_hand"),
                    on(&Device::Controller(Hand::Right)),
                )
                .on_click(command(
                    self.panel,
                    PanelCommand::Attach(
                        Device::Controller(Hand::Right),
                        attach_offset(self.panel, HAND_OFFSET),
                    ),
                )),
            )
            .child({
                // 見えている最初のトラッカーに取り付ける。見えていなければ何もしない。
                let b = button!(
                    "attach-tracker",
                    match &tracker {
                        Some(Device::Tracker(id)) => {
                            i18n::tf(cx, "testing.attach.tracker", &[("id", id)])
                        }
                        _ => i18n::t(cx, "testing.attach.tracker_none"),
                    },
                    tracker.as_ref().is_some_and(on),
                );
                match tracker {
                    Some(d) => b.on_click(command(
                        self.panel,
                        PanelCommand::Attach(d, attach_offset(self.panel, HAND_OFFSET)),
                    )),
                    None => b,
                }
            })
            .child(
                button!("detach", i18n::t(cx, "testing.attach.detach"), false)
                    .on_click(command(self.panel, PanelCommand::Detach)),
            );
        // コントローラの入力の表示（押せない）。見出しの行に置く。
        let input_row = row()
            .child(indicator(
                &theme,
                "menu-held",
                i18n::t(cx, "testing.menu_button"),
                st.menu,
            ))
            .child(indicator(
                &theme,
                "grabbed",
                i18n::t(cx, "testing.grabbing"),
                st.grabbed,
            ));

        // 隣に開く画面。
        let open_row =
            row()
                .child(
                    button!("spawn-panel", i18n::t(cx, "testing.open.testing"), false)
                        .on_click(spawn(self.panel)),
                )
                .child(
                    button!(
                        "spawn-transparent",
                        i18n::t(cx, "testing.open.transparent"),
                        false
                    )
                    .on_click(spawn_transparent(self.panel)),
                )
                .child(
                    button!("spawn-anim", i18n::t(cx, "testing.open.moving"), false).on_click(
                        spawn_view(self.panel, "testing.open.moving", TestingAnim::new),
                    ),
                )
                .child(
                    button!("spawn-lod", i18n::t(cx, "testing.open.lod"), false).on_click(
                        spawn_view(self.panel, "testing.open.lod", |id, _| TestingLod::new(id)),
                    ),
                )
                .child(
                    button!("spawn-font", i18n::t(cx, "testing.open.font"), false).on_click(
                        spawn_view(self.panel, "testing.open.font", |id, _| {
                            TestingFont::new(id)
                        }),
                    ),
                )
                .child(
                    button!("emoji-test", i18n::t(cx, "testing.open.emoji"), false).on_click(
                        spawn_view(self.panel, "testing.open.emoji", |id, _| EmojiTest::new(id)),
                    ),
                )
                .children(cx.try_global::<SetupWizardSpawn>().map(|s| {
                    let open = s.0;
                    let panel = self.panel;
                    button!("setup-wizard", i18n::t(cx, "testing.open.setup"), false).on_click(
                        move |_, _, cx| {
                            cx.default_global::<PanelControl>().spawns.push(open(panel))
                        },
                    )
                }))
                .child(
                    button!("wizard-test", i18n::t(cx, "testing.open.wizard"), false)
                        .on_click(wizard::spawn(self.panel)),
                )
                .child(
                    button!("logo-test", i18n::t(cx, "testing.open.logo"), false).on_click(
                        spawn_view(self.panel, "testing.open.logo", |id, _| LogoTest::new(id)),
                    ),
                )
                .child(
                    button!(
                        "superellipse-test",
                        i18n::t(cx, "testing.open.superellipse"),
                        false
                    )
                    .on_click(spawn_view(
                        self.panel,
                        "testing.open.superellipse",
                        |id, _| SuperellipseTest::new(id),
                    )),
                )
                .child(
                    button!("keyboard-test", i18n::t(cx, "keyboard.title"), false).on_click({
                        let panel = self.panel;
                        move |_, _, cx| {
                            cx.default_global::<PanelControl>()
                                .spawns
                                .push(KeyboardTest::spec(panel));
                        }
                    }),
                )
                .child(
                    button!(
                        "spawn-scrollbar",
                        i18n::t(cx, "testing.open.scrollbar"),
                        false
                    )
                    .on_click(spawn_view(
                        self.panel,
                        "testing.open.scrollbar",
                        |id, _| TestingScrollbar::new(id),
                    )),
                )
                .child(
                    button!("pointer-test", i18n::t(cx, "testing.open.pointer"), false)
                        .on_click(spawn_pointer_test(self.panel)),
                )
                .child(
                    button!("desktop-test", i18n::t(cx, "desktop.title"), false).on_click(
                        spawn_view(self.panel, "desktop.title", |id, _| DesktopTest::new(id)),
                    ),
                )
                .child(
                    button!("audio-test", i18n::t(cx, "audio.title"), false).on_click(spawn_view(
                        self.panel,
                        "audio.title",
                        |id, _| AudioTest::new(id),
                    )),
                )
                .child(
                    button!("skeleton-test", i18n::t(cx, "skeleton.title"), false).on_click({
                        let panel = self.panel;
                        move |_, _, cx| {
                            cx.default_global::<PanelControl>().spawns.push(
                                PanelSpec::spawn(panel, |id, _, cx| SkeletonTest::new(id, cx))
                                    .sized(SKELETON_SIZE, SKELETON_PX)
                                    .titled("skeleton.title"),
                            )
                        }
                    }),
                )
                .child(
                    button!("smoothing-test", i18n::t(cx, "testing.open.laser"), false).on_click(
                        spawn_view(self.panel, "testing.open.laser", |id, _| {
                            SmoothingTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("tooltip-test", i18n::t(cx, "testing.open.tooltip"), false).on_click(
                        spawn_view(self.panel, "testing.open.tooltip", |id, _| {
                            TooltipTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("dialog-test", i18n::t(cx, "testing.open.dialog"), false)
                        .on_click(spawn_dialog_test(self.panel)),
                )
                .child(
                    button!("settings-test", i18n::t(cx, "testing.open.settings"), false).on_click(
                        {
                            let panel = self.panel;
                            move |_, _, cx| {
                                cx.default_global::<PanelControl>()
                                    .spawns
                                    .push(::settings::spec(panel, |_, _| {}))
                            }
                        },
                    ),
                )
                .child(
                    button!("nyapp-test", i18n::t(cx, "testing.open.nyapps"), false).on_click({
                        let panel = self.panel;
                        move |_, _, cx| {
                            // 一覧と詳細を左右に並べるので、設定画面と同じ大きさ。
                            cx.default_global::<PanelControl>().spawns.push(
                                PanelSpec::spawn(panel, |id, _, cx| NyappTest::new(id, cx))
                                    .sized(::settings::SIZE, ::settings::PX)
                                    .titled("testing.open.nyapps"),
                            )
                        }
                    }),
                )
                .child(
                    button!("nav-test", i18n::t(cx, "testing.open.navigation"), false).on_click(
                        spawn_view(self.panel, "testing.open.navigation", |id, _| {
                            NavTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("mirror-test", i18n::t(cx, "testing.open.mirror"), false)
                        .on_click(spawn_mirror_test(self.panel)),
                )
                .child(
                    button!("slide-test", i18n::t(cx, "testing.open.slide"), false).on_click(
                        spawn_view(self.panel, "testing.open.slide", |id, _| SlideTest::new(id)),
                    ),
                )
                .child(
                    button!("stress-test", i18n::t(cx, "testing.open.stress"), false).on_click(
                        spawn_view(self.panel, "testing.open.stress", |id, _| {
                            StressTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("spawn-catalog", i18n::t(cx, "testing.open.catalog"), false)
                        .on_click(spawn_catalog(self.panel)),
                )
                .child(
                    button!(
                        "locale-test",
                        i18n::t(cx, "testing.open.localization"),
                        false
                    )
                    .on_click({
                        let panel = self.panel;
                        move |_, _, cx| {
                            // 全キーの表を並べるので Testing と同じ大きさ。
                            cx.default_global::<PanelControl>().spawns.push(
                                PanelSpec::spawn(panel, |id, _, _| LocaleTest::new(id))
                                    .sized(TESTING_SIZE, TESTING_PX)
                                    .titled("locale.title"),
                            )
                        }
                    }),
                );

        // ボタン・切り替え・長押し・ポップアップ。
        let press_row = row()
            .child(
                button!(
                    "click-me",
                    i18n::tf(cx, "testing.clicked", &[("count", &self.clicks)]),
                    false
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clicks += 1;
                    cx.notify();
                })),
            )
            .child(
                button!("toggle", i18n::t(cx, "testing.toggle"), self.toggled).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.toggled = !this.toggled;
                        cx.notify();
                    }),
                ),
            )
            .child(
                button!("hold-me", i18n::t(cx, "testing.hold"), false)
                    .active(|s| s.bg(rgba(theme.control_active))),
            )
            .child(
                div()
                    .child(
                        button!(
                            "open-menu",
                            i18n::tf(cx, "testing.menu", &[("item", &self.picked.unwrap_or("-"))]),
                            self.menu_open,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu_open = !this.menu_open;
                            cx.notify();
                        })),
                    )
                    .when(self.menu_open, |d| {
                        d.child(deferred(
                            anchored().child(
                                div()
                                    .mt(px(spacing::XS))
                                    .p(px(spacing::XS))
                                    .flex()
                                    .flex_col()
                                    .gap(px(spacing::XS))
                                    .rounded(px(radius::LG))
                                    .bg(rgba(theme.card))
                                    .shadow_md()
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.menu_open = false;
                                        cx.notify();
                                    }))
                                    .children(["menu-a", "menu-b"].map(|id| {
                                        button!(id, id, self.picked == Some(id)).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.picked = Some(id);
                                                this.menu_open = false;
                                                cx.notify();
                                            }),
                                        )
                                    })),
                            ),
                        ))
                    }),
            );
        // ドラッグ＆ドロップ: 左を右へ運ぶ。
        let drag_row = row()
            .child(
                button!("drag-chip", i18n::t(cx, "testing.drag_me"), false)
                    .on_drag(Chip, |_, _, _, cx| ui::drag_preview(cx, ChipPreview)),
            )
            .child(
                div()
                    .id("drop-zone")
                    .debug_selector(|| "drop-zone".into())
                    .flex_1()
                    .h(px(54.0))
                    .px(px(spacing::LG))
                    .flex()
                    .items_center()
                    .rounded(px(radius::LG))
                    .border_2()
                    .border_dashed()
                    .border_color(rgba(theme.neutral_text_low))
                    .bg(rgba(theme.control))
                    .drag_over::<Chip>(|s, _, _, _| s.bg(rgb(DROP_TARGET_BG)))
                    .on_drop(cx.listener(|this, _: &Chip, _, cx| {
                        this.dropped += 1;
                        cx.notify();
                    }))
                    .child(i18n::tf(
                        cx,
                        "testing.drop_here",
                        &[("count", &self.dropped)],
                    )),
            );

        let field = div()
            .id("text-field")
            .debug_selector(|| "text-field".into())
            .track_focus(&focus)
            .h(px(54.0))
            .px(px(spacing::LG))
            .flex()
            .items_center()
            .rounded(px(radius::LG))
            .bg(rgba(theme.control))
            .focus(|s| s.bg(rgba(theme.accent_bg_active)))
            .on_click(cx.listener(move |this, _, window, cx| {
                ui::keyboard::focus_input(this.panel, &focus, window, cx);
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let k = &e.keystroke;
                match (k.key.as_str(), &k.key_char) {
                    ("backspace", _) => {
                        this.text.pop();
                    }
                    (_, Some(c)) => this.text.push_str(c),
                    ("space", None) => this.text.push(' '),
                    (key, None) if key.chars().count() == 1 => this.text.push_str(key),
                    _ => return,
                }
                cx.notify();
            }))
            .child(if self.text.is_empty() {
                div()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "testing.type_hint"))
            } else {
                div().child(self.text.clone())
            });

        let scrolled = format!("{:.1}", self.scrolled);
        let rows: Vec<SharedString> = (0..LIST_ROWS)
            .map(|i| i18n::tf(cx, "testing.row", &[("index", &i), ("scrolled", &scrolled)]))
            .collect();
        let list =
            div()
                .id("scroll-list")
                .debug_selector(|| "scroll-list".into())
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.list)
                .rounded(px(radius::LG))
                .bg(rgba(theme.control))
                .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                    this.scrolled += match e.delta {
                        ScrollDelta::Lines(p) => p.y,
                        ScrollDelta::Pixels(p) => f32::from(p.y) / SCROLL_LINE_PX,
                    };
                    cx.notify();
                }))
                .child(components::elastic("pull-list", &self.list).children(
                    rows.into_iter().map(|row| {
                        div()
                            .h(design::font::scaled(ROW_PX))
                            .px(px(spacing::LG))
                            .flex()
                            .items_center()
                            .child(row)
                    }),
                ));

        // 見出し: 何の画面か、このパネルの今の状態、閉じる。
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(spacing::SM))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .child(
                        div()
                            .text_size(px(font_size::LG))
                            .font_weight(FontWeight::BOLD)
                            .child(i18n::t(cx, "testing.title")),
                    )
                    .child(
                        div()
                            .debug_selector(|| "panel-status".into())
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::tf(
                                cx,
                                "testing.status",
                                &[
                                    ("runtime", &format!("{:?}", st.runtime)),
                                    (
                                        "attached",
                                        &st.attached
                                            .as_ref()
                                            .map_or(i18n::t(cx, "common.none"), |d| {
                                                format!("{d:?}").into()
                                            }),
                                    ),
                                    (
                                        "lock",
                                        &i18n::t(
                                            cx,
                                            if st.locked {
                                                "testing.locked"
                                            } else {
                                                "testing.unlocked"
                                            },
                                        ),
                                    ),
                                ],
                            )),
                    )
                    .child(input_row),
            );

        // カードが複数なので地を敷く。左 = このパネルと開く画面、右 = 部品の操作。
        components::panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(spacing::MD))
                    .child(
                        div()
                            .id("testing-left")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(spacing::MD))
                            .child(
                                section(&theme, i18n::t(cx, "testing.section.this_panel"))
                                    .child(labeled(
                                        &theme,
                                        i18n::t(cx, "testing.label.view"),
                                        panel_row,
                                    ))
                                    .child(labeled(
                                        &theme,
                                        i18n::t(cx, "testing.label.attach_to"),
                                        attach_row,
                                    )),
                            )
                            .child(
                                section(&theme, i18n::t(cx, "testing.section.open"))
                                    .child(open_row),
                            ),
                    )
                    .child(
                        section(&theme, i18n::t(cx, "testing.section.interact"))
                            .flex_1()
                            .min_w_0()
                            .child(labeled(
                                &theme,
                                i18n::t(cx, "testing.label.press"),
                                press_row,
                            ))
                            .child(labeled(&theme, i18n::t(cx, "testing.label.drag"), drag_row))
                            .child(labeled(&theme, i18n::t(cx, "testing.label.type"), field))
                            .child(
                                labeled(
                                    &theme,
                                    i18n::t(cx, "testing.label.scroll"),
                                    div().relative().flex_1().min_h_0().child(list).child(
                                        components::scrollbars(&theme, "scroll-bars", &self.list)
                                            .pulled_by("pull-list"),
                                    ),
                                )
                                .flex_1()
                                .min_h_0(),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
