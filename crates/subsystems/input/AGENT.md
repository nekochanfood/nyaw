# crates/subsystems/input

層: 1 Subsystems

コントローラ機種の差を吸収（ドライバ的役割）。プロファイル表で生入力 → 抽象アクション。コントローラを持たない手は、骨格から作った仮想コントローラ（`hand_controller`、プロファイル `ext/hand_interaction_ext`）で操作する: 親指と人差し指のつまみ = クリック、中指・薬指・小指を握る = 掴む、左手の手のひらを顔に向けて親指と小指を `kernel::HAND_LONG_PRESS`（0.5 秒）付け続ける = ショートカット（X ボタンの二回押しと同じく `PanelStatus::double_press` に数える）。握りは指の曲がりの角度。レーザーの元は親指と人差し指の付け根の間、向きは手の向き（手首から中指の付け根へ。`HandSkeleton::grip`。手首を曲げれば動き、指では動かない）。コントローラに取り付けたパネル（wrist）は、骨格の手でもランタイムがその手のデバイスを追っていれば（`vr::Frame::hand_devices`。OpenVR は入力のアクションか手の役割 `GetTrackedDeviceIndexForControllerRole` のデバイス。Virtual Desktop のハンドトラッキングは `vd_hand_controller`）コントローラと同じく相対配置で追従させ、無ければ `grip`（揺れを `input::PoseFilter` で抑えたもの。元は手首の時計が腕時計の位置に来る所）に付く。骨格とコントローラの両方がある手は、入力の元の切り替えを 18 フレーム続いてから行う（持ち替え中の申告の揺れでクリックが効かなくならないように）。ただしコントローラが一瞬見えずに骨格へ落ちた手は、戻ればすぐコントローラに戻す。OpenVR は相対配置を任せたオーバーレイの取り付け先の裏のデバイスが入れ替わると付け直す。レーザーは `HandFilter`（One Euro 風。止めると強く抑え、速く振ると付いてくる。つまみかけ〜押している間はさらに鈍くし、クリックしている間は `click_hold_deg` 以内の動きでは押した瞬間のレーザーのまま止める）で揺れを抑える。指を全部追っている手（`vr::FingerTracking::Full`。OpenVR の `GetSkeletalTrackingLevel`。光学式のハンドトラッキングをランタイムがコントローラとして見せていても）も骨格で操作する。しきい値と抑えは `HandTuning`（`config.toml` の `[hand]`、`Setting::Hand`。Subsystems の `hand`）。判定に使った値は `ControllerState::metrics` と `NYAW_LOG=debug` の `controller buttons` に出る。`ControllerState::facing` に手のひら / 手の甲（骨格の手だけ）。Subsystems が `poll` で足す

## 主な公開 API

`map(&RawController, prev) -> ControllerState`, `source(profile, Action)` / `scroll_source(profile)`（逆引き）
