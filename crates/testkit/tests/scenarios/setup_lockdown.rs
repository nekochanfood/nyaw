//! 結合: セットアップウィザード + ロックダウン + LazyFollow。
//! 開いている間は閉じられず・ダッシュボードの開閉が止められ、頭の向き（左右だけ）と歩きにゆるく付いてきて、
//! 完了するとロックダウンが外れてパネルが閉じる。

use gpui::TestAppContext;
use kernel::PanelCommand;
use kernel::vr::{Pose, Quat, Vec3};
use setup::SetupWizard;
use testkit::{Driver, VrKit};

/// 頭の姿勢（`yaw` はラジアン。+X 側へ向く向きが正）。
fn head(x: f32, y: f32, z: f32, yaw: f32) -> Pose {
    Pose::new(Vec3::new(x, y, z), Quat::from_rotation_y(-yaw))
}

/// 1 回に進めるフレーム数。時定数を短くしてあるので（下）、これで追いつく。
const STEPS: usize = 120;

/// 追いつくまで進める。
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
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    // 起動仕様（`setup::spec`）と同じロックダウンと追従を付ける。
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.kernel.apply(
        0,
        // 本番と同じ設定で、追いつきだけ速くする（描画を伴うのでフレーム数を抑えたい）。
        PanelCommand::LazyFollow(Some(setup::follow().with_time_constants(0.1, 0.05))),
    );
    settle(&mut kit, head(0.0, 1.6, 0.0, 0.0));

    // ロックダウン: 設定が全体に反映され（wrist などが見る）、閉じる要求は無視される。
    assert!(kit.kernel.settings().lockdown);
    assert!(kit.panel_control(|c| c.settings.lockdown));
    kit.kernel.apply(0, PanelCommand::Close);
    assert!(!kit.kernel.panel(0).closed, "ロックダウン中は閉じられない");

    // 頭の正面に、HMD の高さで置かれる。
    let p = kit.panel_pose().position;
    assert!(p.abs_diff_eq(Vec3::new(0.0, 1.6, -1.0), 5e-2), "{p}");

    // 小さく首を振っても逃げない（不感帯）。
    settle(&mut kit, head(0.0, 1.6, 0.0, 10f32.to_radians()));
    assert!(kit.panel_pose().position.abs_diff_eq(p, 5e-2));

    // 見上げて首を傾けても、上下・傾きは見ない。
    let tilted = Pose::new(
        Vec3::new(0.0, 1.6, 0.0),
        Quat::from_rotation_x(0.8) * Quat::from_rotation_z(0.4),
    );
    settle(&mut kit, tilted);
    assert!(kit.panel_pose().position.abs_diff_eq(p, 5e-2));
    let up = kit.panel_pose().orientation * Vec3::Y;
    assert!(up.abs_diff_eq(Vec3::Y, 1e-3), "パネルは立ったまま: {up}");

    // 大きく向きを変えると付いてくる（右へ 90°）。
    settle(&mut kit, head(0.0, 1.6, 0.0, 90f32.to_radians()));
    let p = kit.panel_pose().position;
    assert!(p.abs_diff_eq(Vec3::new(1.0, 1.6, 0.0), 8e-2), "{p}");

    // 歩いてしゃがむと、距離を保って高さも合わせて付いてくる。
    let walked = head(4.0, 1.0, -3.0, 90f32.to_radians());
    settle(&mut kit, walked);
    let p = kit.panel_pose().position;
    assert!(
        (flat_distance(p, walked.position) - 1.0).abs() < 5e-2,
        "{p}"
    );
    assert!((p.y - 1.0).abs() < 5e-2, "高さ {}", p.y);

    // 追従しても操作できる: 最後まで進めて完了すると、ロックダウンが外れて閉じる。
    for _ in 1..setup::STEPS.len() {
        kit.click("setup-next");
    }
    assert!(kit.read(|v| v.wizard.is_last()));
    assert!(kit.kernel.settings().lockdown, "完了を押すまでは続く");
    kit.click("setup-finish");
    kit.step();
    assert!(kit.kernel.settings().setup_done);
    assert!(!kit.kernel.settings().lockdown);
    assert!(kit.kernel.panel(0).closed);
}
