#!/usr/bin/env bash
# nyaw の AppImage を作成する。`cargo build --release` の後に実行する（CI は .github/workflows/appimage.yml）。
# 必要なコマンドは curl と mksquashfs（squashfs-tools）。同梱の方針は AGENT.md の「Linux の配布（AppImage）」。
# 出力は target/installer/nyaw-<バージョン>-x86_64.AppImage
set -euo pipefail
umask 022

# AppImage の先頭に結合するランタイム（AppImage/type2-runtime）。静的リンクのため、実行する環境に libfuse2 は不要
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

installer=$(cd "$(dirname "$0")" && pwd)
app=$(dirname "$installer")
root=$(cd "$app/../.." && pwd)
target=${CARGO_TARGET_DIR:-$app/target}
# バージョンはリリースワークフローがタグから渡す（NYAW_VERSION）。未設定であれば Cargo.toml の値
version=${NYAW_VERSION:-$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "$app/Cargo.toml")}

runtime=$target/appimage/runtime-$RUNTIME_VERSION-x86_64
if [ ! -f "$runtime" ]; then
  mkdir -p "${runtime%/*}"
  curl -fsSL --retry 3 --connect-timeout 30 -o "$runtime.partial" \
    "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64"
  mv "$runtime.partial" "$runtime"
fi
echo "$RUNTIME_SHA256  $runtime" | sha256sum --check --quiet

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
dir=$work/nyaw.AppDir
icon=$root/assets/logos/Logo_Mini_Light.svg

install -Dm755 "$target/release/nyaw" "$dir/usr/bin/nyaw"
# Steam Audio は audio の build.rs が実行ファイルの隣に配置し、実行時も隣から読み込む。ない場合はここで失敗する
install -Dm644 "$target/release/libphonon.so" "$dir/usr/bin/libphonon.so"
install -Dm644 -t "$dir/usr/share/doc/nyaw" \
  "$target/release/steamaudio-THIRDPARTY.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE"

# ルート直下の .desktop、アイコン、.DirIcon は AppImage の仕様が要求する
install -Dm644 -t "$dir/usr/share/applications" "$installer/nyaw.desktop"
install -Dm644 "$icon" "$dir/usr/share/icons/hicolor/scalable/apps/nyaw.svg"
install -m644 "$installer/nyaw.desktop" "$dir/nyaw.desktop"
install -m644 "$icon" "$dir/nyaw.svg"
ln -s nyaw.svg "$dir/.DirIcon"
# nyaw と子プロセスの環境変数を変えないよう、AppRun はスクリプトではなく実行ファイルへのリンクにする
ln -s usr/bin/nyaw "$dir/AppRun"

mksquashfs "$dir" "$work/nyaw.squashfs" -root-owned -noappend -no-xattrs -comp zstd -quiet -no-progress
cat "$runtime" "$work/nyaw.squashfs" > "$work/nyaw.AppImage"
output=$target/installer/nyaw-$version-x86_64.AppImage
install -Dm755 "$work/nyaw.AppImage" "$output"
echo "$output"
