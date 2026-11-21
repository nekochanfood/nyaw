//! テスト共通ユーティリティ。各画面のテストは対応するファイルの子モジュール（`bar/tests.rs` 等）に配置する。

use super::*;
pub(crate) use gpui::{Modifiers, TestAppContext, VisualTestContext};
pub(crate) use kernel::desktop::DEFAULT_FRAME_RATE;

/// テスト用通知（ID: `id`。タイトル・本文・アクションボタン 1 つ）。
pub(crate) fn note(id: u32) -> kernel::subsystems::notifications::Notification {
    use kernel::subsystems::notifications as n;
    n::Notification {
        id,
        app: n::App {
            id: "Chat".into(),
            name: "Chat".into(),
            icon: None,
        },
        title: format!("Title {id}"),
        body: "Body".into(),
        arrived: std::time::SystemTime::now(),
        url: None,
        actions: vec![n::Action {
            label: "Reply".into(),
            url: None,
        }],
    }
}

pub(crate) fn summon(from: PanelId, opened: Opened) -> PanelSpec {
    PanelSpec::spawn(from, move |id, _, cx| {
        opened(id, cx);
        Mirror::new(id, None, None, None, cx)
    })
}

pub(crate) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, Modifiers::none());
}

/// 最新のスポーン要求をパネル `id` で開く。
pub(crate) fn open_spawned(cx: &mut VisualTestContext, id: PanelId) {
    cx.update(|window, cx| {
        let spec = cx.default_global::<PanelControl>().spawns.pop().unwrap();
        let _ = spec.build(id, window, cx);
    });
}

pub(crate) fn commands(cx: &mut VisualTestContext, id: PanelId) -> Vec<PanelCommand> {
    cx.update(|_, cx| {
        cx.global::<PanelControl>()
            .panels
            .get(&id)
            .map_or(Vec::new(), |p| p.commands.clone())
    })
}

/// `id` に蓄積されたコマンドを取り出す（[`commands`] はコピーのみ）。
pub(crate) fn take_commands(cx: &mut VisualTestContext, id: PanelId) -> Vec<PanelCommand> {
    cx.update(|_, cx| {
        std::mem::take(
            &mut cx
                .default_global::<PanelControl>()
                .panels
                .entry(id)
                .or_default()
                .commands,
        )
    })
}

pub(crate) fn docked_to_bar(c: &PanelCommand) -> bool {
    matches!(
        c,
        PanelCommand::Parent(Some(kernel::Parent {
            panel: 1,
            docked: true,
            slot: Some(_),
            ..
        }))
    )
}

pub(crate) fn playing() -> std::sync::Arc<media::NowPlaying> {
    std::sync::Arc::new(media::NowPlaying {
        title: "Song".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        playing: false,
        position: Duration::from_secs(65),
        duration: Some(Duration::from_secs(200)),
        at: std::time::Instant::now(),
        app: media::App {
            id: "Spotify.exe".into(),
            name: "Spotify".into(),
            icon: None,
            exe: None,
        },
        art: None,
        backdrop: None,
        light: Some(true),
        controls: media::Controls {
            play_pause: true,
            next: true,
            previous: false,
            seek: true,
            raise: false,
        },
        sessions: 1,
    })
}

pub(crate) fn redraw_bar(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// テストでバーを別ウィンドウに描画するためのラッパー。
pub(crate) struct BarView(pub(crate) gpui::Entity<Dashboard>);
impl Render for BarView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

/// テストで [`Mirror`] をウィンドウに載せるためのコンテナ。
pub(crate) struct Waiter(pub(crate) gpui::Entity<Mirror>);

impl Render for Waiter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}
