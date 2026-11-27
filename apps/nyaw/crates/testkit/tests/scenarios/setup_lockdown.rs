//! 結合テスト: セットアップウィザード + ロックダウン + LazyFollow。
//! 表示中はパネルを閉じられず、ダッシュボードの開閉も抑制される。頭の水平方向の向きと歩行に緩やかに追従し、
//! 完了時にロックダウンが解除されてパネルが閉じる。

use gpui::TestAppContext;
use kernel::PanelCommand;
use kernel::vr::{Pose, Quat, Vec3};
use setup::SetupWizard;
use testkit::{Driver, VrKit};

/// 頭部の姿勢を生成する（`yaw` はラジアン。+X 方向が正）。
fn head(x: f32, y: f32, z: f32, yaw: f32) -> Pose {
    Pose::new(Vec3::new(x, y, z), Quat::from_rotation_y(-yaw))
}

/// 1 回の収束に必要なフレーム数。時定数を短縮しているため（後述）、この回数で十分追従する。
const STEPS: usize = 120;

/// パネルが追従し収束するまでフレームを進める。
fn settle(kit: &mut VrKit<'_, SetupWizard>, head: Pose) {
    kit.kernel.subsystems.backend.frame.hmd = head;
    for _ in 0..STEPS {
        kit.step();
    }
}

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

#[gpui::test]
fn setup_wizard_is_locked_down_and_lazily_follows_the_user(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| SetupWizard::build(0, cx));
    // 起動仕様（`setup::spec`）と同じロックダウンおよび追従を設定する。
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.kernel.apply(
        0,
        // 本番と同一設定で、追従の時定数のみ短縮する（描画を伴うためフレーム数を抑える）。
        PanelCommand::LazyFollow(Some(setup::follow().with_time_constants(0.1, 0.05))),
    );
    settle(&mut kit, head(0.0, 1.6, 0.0, 0.0));

    // ロックダウン: 設定がグローバルに反映され（wrist 等が参照）、閉じる要求は無視される。
    assert!(kit.kernel.settings().lockdown);
    assert!(kit.panel_control(|c| c.settings.lockdown));
    kit.kernel.apply(0, PanelCommand::Close);
    assert!(
        !kit.kernel.panel(0).closed,
        "ロックダウン中はパネルを閉じることができない"
    );

    // 頭部の正面方向に、HMD の高さで配置される。
    let d = setup::follow().distance;
    let p = kit.panel_pose().position;
    assert!(p.abs_diff_eq(Vec3::new(0.0, 1.6, -d), 5e-2), "{p}");

    // 小さな首振りでは追従しない（不感帯）。
    settle(&mut kit, head(0.0, 1.6, 0.0, 10f32.to_radians()));
    assert!(kit.panel_pose().position.abs_diff_eq(p, 5e-2));

    // 見上げや首の傾きには反応せず、上下・ロールは無視する。
    let tilted = Pose::new(
        Vec3::new(0.0, 1.6, 0.0),
        Quat::from_rotation_x(0.8) * Quat::from_rotation_z(0.4),
    );
    settle(&mut kit, tilted);
    assert!(kit.panel_pose().position.abs_diff_eq(p, 5e-2));
    let up = kit.panel_pose().orientation * Vec3::Y;
    assert!(up.abs_diff_eq(Vec3::Y, 1e-3), "パネルは立ったまま: {up}");

    // 大きく向きを変えるとパネルが追従する（右へ 90°）。追従は 2° 手前で停止するため、ずれは距離に比例する。
    settle(&mut kit, head(0.0, 1.6, 0.0, 90f32.to_radians()));
    let p = kit.panel_pose().position;
    assert!(p.abs_diff_eq(Vec3::new(d, 1.6, 0.0), 4e-2 * d), "{p}");

    // 移動してしゃがむと、距離を維持しつつ高さも合わせて追従する。
    let walked = head(4.0, 1.0, -3.0, 90f32.to_radians());
    settle(&mut kit, walked);
    let p = kit.panel_pose().position;
    assert!((flat_distance(p, walked.position) - d).abs() < 5e-2, "{p}");
    assert!((p.y - 1.0).abs() < 5e-2, "高さ {}", p.y);

    // 追従中でも操作可能: 最後まで進める。完了画面ではロックダウンを解き（ショートカットの二回押しでダッシュボードを開けるように）、
    // 二回押しで完了する（novr の通し操作 `setup::setup_decides_each_setting_and_marks_done`）。
    // 非表示のステップ（OpenVR 以外での SteamVR ステップ）はスキップされるため、ステップ数ではなく末尾まで押す。
    for _ in 1..setup::STEPS.len() {
        if kit.read(|v| v.wizard.is_last()) {
            break;
        }
        assert!(
            kit.kernel.settings().lockdown,
            "完了画面まではロックダウンを続ける"
        );
        kit.click("setup-next");
    }
    assert!(kit.read(|v| v.wizard.is_last()));
    kit.step();
    assert!(!kit.kernel.settings().lockdown);
    assert!(!kit.kernel.panel(0).closed, "二回押すまで開いたまま");
}

/// 完了画面は、左手で使用中のコントローラを映し、その機種のショートカットのボタンを示す（Index なら A）。
#[gpui::test]
fn done_page_shows_the_left_controller_in_use(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| {
        let mut v = SetupWizard::build(0, cx);
        for _ in setup::STEPS {
            v.go(true, &kernel::Settings::default());
        }
        v
    });
    kit.hand(kernel::Hand::Left)
        .profile("valve/index_controller")
        .render_model("{indexcontroller}valve_controller_knu_1_0_left");
    kit.step().step();
    assert!(kit.try_bounds("zoom-ring-a").is_some());
    assert!(kit.try_bounds("zoom-ring-x").is_none());
    // 開発ビルドだけ、右上のドロップダウンでほかの機種を映せる。
    kit.click("setup-controller")
        .click("setup-controller-oculus_quest_plus_controller_left");
    assert_eq!(kit.read(|v| v.preview), Some(guide::Model::QuestPlus));
    assert!(kit.try_bounds("zoom-ring-x").is_some());
    assert!(kit.try_bounds("zoom-ring-a").is_none());
}
