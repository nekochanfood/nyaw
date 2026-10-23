# crates/subsystems/audio

層: 1 Subsystems

OS の音声出力（cpal）・WAV の読み込み・Steam Audio による立体化（HRTF）。出力・ミックス・立体化は別スレッドで、メインスレッドは命令を送るだけ。音は「スピーカー」（音源）から鳴る

## 主な公開 API

`Engine::{open, offline, render, set_listener, set_speaker, remove_speaker, play, play_sound, stop, stop_speaker, poll, playing, spatializer}`, `Speaker`, `Layout::{Auto, Mono, Stereo}`, `Channels`, `Clip`, `Sound::{File, Wav}`, `Spatializer`

## 音声（パネルのスピーカー）

- どのパネルにもスピーカー（音源）があり、パネルの中で鳴らす音はそこから聞こえる。`PanelCommand::Play(audio::Sound, looping)` で鳴らし（読み込みは別スレッド。同じパネルでも別のパネルでも、いくつでも重ねて鳴る）、`StopAudio` でそのパネルの音を止める。パネルを閉じると止まる。`PanelStatus::audio` に再生中の数・設定・立体化の手段。
- `PanelCommand::PlayAt { sound, at, pitch }` はパネルの中の点（ピクセル）から一度だけ鳴らす。その点に置いた 1 点の音源（左右を混ぜる）で、パネルごとに 8 個を順に使い回し、鳴らした時の置き場所に留まる。`pitch` は速さの倍率（`audio::Clip::pitched`）。
- 仮想キーボード（`ui::keyboard::KeyboardView`）はキーを押すと、押した点から `PlayAt` でキーの音（`crates/ui/assets/keyboard/*.wav`。Enter・Backspace・Space は専用、ほかは tap）を鳴らす。音の高さは押すたびに 1 ± 0.1 で揺らす。
- 立体化は既定で有効（オプトアウト）。`PanelCommand::Spatial(false)` で外すと、置き場所・頭の向きに関係なくそのまま左右に鳴る。
- スピーカーはモノラル（パネルの中央の 1 点）とステレオ（パネルの左右の端の 2 点。間隔はパネルの幅）。既定の `Layout::Auto` は鳴らす音に合わせ、1ch と左右がまったく同じ 2ch はモノラル、それ以外はステレオ。`PanelCommand::SpeakerLayout(Layout::Mono | Layout::Stereo)` で固定できる（モノラルは左右を混ぜ、ステレオはモノラルの音を両端から鳴らす）。
- 聞き手は HMD。カーネルが毎 tick、鳴っているスピーカーだけをパネルの姿勢に合わせて動かす（鳴っていなければ何も送らない）。距離は 1m までは減衰せず、その先は逆距離で減る（Steam Audio の既定の逆距離モデル）。
- 立体化は Steam Audio の HRTF（バイノーラル）。ライブラリ（現在のバックエンドは `phonon.dll`。Steam Audio 4.8 の SDK の `lib/windows-x64`）は実行時に読む。`NYAW_PHONON` でファイルを指すか、実行ファイルの隣（Windows）・ライブラリの探索パスに置く。無ければ左右の等パワーのパンニングに落ちる（`Spatializer::Panning`）。ビルドに clang や Steam Audio は要らない（`audio/src/phonon.rs` が `phonon.h` から使う型と関数だけを写している）。
- ミックスは 256 フレームごと。HRTF の状態は最初に 64 個（32 音 × L / R）作っておき、コールバックの中では作らない。超えた音はパンニングで鳴る。出力のレートへは線形補間で合わせる。
- テスト用のカーネル（`Kernel::with_backend`）はデバイスを開かない `audio::Engine::offline`（パンニング）で、テストが `kernel.audio.render` で回して左右に出た音を測る。本番の `Kernel::boot` は `Engine::open`（デバイスを開く・Steam Audio を読むのは別スレッド）。
- `cargo test -p audio -p kernel --lib`、UI を通すシナリオは `cargo test -p testkit --test audio`。実際の Steam Audio とデバイスは `cargo test -p audio -- --ignored`（音が出る。`NYAW_PHONON` で phonon を指す）。
