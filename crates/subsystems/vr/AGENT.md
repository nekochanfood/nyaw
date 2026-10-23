# crates/subsystems/vr

層: カーネル / Subsystems / ランタイム

OpenVR / OpenXR / novr の差を吸収。`Subsystems` が `VrBackend` を所有し、上位は共通フレームを受け取る。novr はデスクトップのウィンドウで VR を模すエミュレーター

## 主な公開 API

`connect`, `VrBackend`, `Runtime`, `novr::{Emulator, Button}`, `Pose`, `Frame`（`pose(&Device)`。`hands` に骨格、`batteries` に電池の残量 `Battery`（OpenVR は 5 秒ごとに読み直す。OpenXR は標準の手段が無く空、novr は固定の模擬値））, `Device`, `RawController`, `HandSkeleton`（スケルタル入力。関節 `JOINT_COUNT` = 26 は OpenXR の並び、`joint_parent`, `TIPS`, `pinch_distance`。OpenXR は XR_EXT_hand_tracking、OpenVR は骨格のアクション。novr には無い）, `mock::MockBackend`（`test-support` フィーチャー時のみ）

## novr（エミュレーター）

- **novr は VR のエミュレーター**（`vr::novr::Emulator`）。最初の poll でウィンドウ（`nyaw (novr)`）を開き、受け取ったテクスチャを
  固定した頭（原点・前方 -Z、縦の視野 70°）から見た絵として合成する。重なりは VR と同じく描画順。背景は透過がわかるよう無彩色の市松模様。
  ウィンドウを閉じると終了を求める（`Frame::quit`）。poll は約 90Hz に待って周期を作る（VR の vsync 待ちに当たる）。
  - 合成は GPU（wgpu。`vr::compositor`）でウィンドウに直接描く。使えなければ（または `gpu.emulator = false` か `NYAW_EMULATOR_CPU=1`）CPU で合成して minifb に渡す。
    GPU のときも、見えたか（`VrBackend::overlay_seen`。隠れたパネルの描き直しを止めるカリングに使う）は縦横 1/8 の CPU の合成で求める。
    スクリーンショットはいつも CPU で合成し直す。`Emulator::headless` はウィンドウを出さず CPU で合成し、`Emulator::scripted_window` は
    ウィンドウと合成は本番どおりで入力だけ注入する（ベンチマーク用）。
  - カーソルの周りの輪は長押しの進み（`PanelControl::hold`）。満ちるとマウスの押下になり、その場でドラッグが始まる。
  - 片手操作: 右手のレイは目からマウスの方向（レーザーは視線と重なるので見えず、カーソルだけ見える）。左クリック = トリガー、
    右クリック = グリップ（掴む）、X キー = 左手の X ボタン（`novr::Button::X`）、矢印キー = 右手のスティック、掴んでいる間の PageUp / PageDown = パネルの拡大 / 縮小（テスト用。`vr::RESIZE_INPUT` → `ControllerState::resize`）。左手は手元で下を向いていてパネルを指さず、WASD = 左手のスティック。
  - デバイスは実在しないので取り付けたパネルは追従しない（Subsystems が返す共通フレームの `pose` が `None`。取り付け先を見失ったのと同じ扱い）。
  - キーボードはスティックなのでパネルには届かない（VR と同じ）。
- ビューはランタイムを意識しない（どのランタイムでも同じビューが動く）。

## 未実装（土台のみ）

- 実ランタイムでの確認の残り。OpenVR（`openvr_bindings` の C API の関数表。openvr_api は jsoncpp が OpenXR ローダーと衝突するのでリンクせず、`openvr/client.rs` が vrclient を直接読む）は null ドライバで接続・オーバーレイへの送信（Vulkan テクスチャ、毎フレーム更新）まで確認済みで、コントローラ入力は未確認。OpenXR（XR_EXTX_overlay + XR_KHR_vulkan_enable2）は Monado（Windows、simulated）でパネルが映るところまで確認済みで、コントローラ入力は未確認。
- 描画は毎フレーム CPU に読み戻し、変化があれば `vr` が Vulkan のイメージへ上げ直してランタイムへ渡す（`crates/subsystems/vr/src/gpu.rs`。1024x768 で約 1.6ms）。GPUI のテクスチャを読み戻さずに直接共有する経路は未実装。
