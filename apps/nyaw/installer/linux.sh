#!/usr/bin/env bash
# Linux の配布物を作成する。`cargo build --release` の後に実行する（CI は .github/workflows/linux.yml）。
# 必要なコマンドは curl、mksquashfs（squashfs-tools）、zsyncmake（zsync）、readelf（binutils）、tar。
# 同梱の方針と更新情報は AGENT.md の「Linux の配布」。
# 出力は target/installer/ の次のファイル:
#   nyaw-<バージョン>-x86_64.AppImage、同 .AppImage.zsync、同 .tar.gz、同 .rpm、同 .pkg.tar.zst、nyaw-<バージョン>-amd64.deb
# ファイル名は、アプリの更新が配布形式の判別に使う（crates/subsystems/update の Package）
set -euo pipefail
umask 022

# AppImage の先頭に結合するランタイム（AppImage/type2-runtime）。静的リンクのため、実行する環境に libfuse2 は不要
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
# deb、rpm、pkg.tar.zst を作成する（goreleaser/nfpm）
NFPM_VERSION=2.49.0
NFPM_SHA256=2afaa5ffb5e3493402e31e7c67a5a1fb42133e3f66fc315ca0051934125561c7

installer=$(cd "$(dirname "$0")" && pwd)
app=$(dirname "$installer")
root=$(cd "$app/../.." && pwd)
target=${CARGO_TARGET_DIR:-$app/target}
output=$target/installer
tools=$target/installer-tools
# バージョンはリリースワークフローがタグから渡す（NYAW_VERSION）。未設定であれば Cargo.toml の値
version=${NYAW_VERSION:-$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "$app/Cargo.toml")}
repository=$(sed -n 's|^repository = "https://github.com/\([^"]*\)".*|\1|p' "$app/Cargo.toml")

# fetch <保存先> <URL> <SHA-256>
fetch() {
  if [ ! -f "$1" ]; then
    mkdir -p "${1%/*}"
    curl -fsSL --retry 3 --connect-timeout 30 -o "$1.partial" "$2"
    mv "$1.partial" "$1"
  fi
  echo "$3  $1" | sha256sum --check --quiet
}

runtime=$tools/runtime-$RUNTIME_VERSION-x86_64
fetch "$runtime" \
  "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64" "$RUNTIME_SHA256"
nfpm_archive=$tools/nfpm-$NFPM_VERSION.tar.gz
fetch "$nfpm_archive" \
  "https://github.com/goreleaser/nfpm/releases/download/v$NFPM_VERSION/nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz" \
  "$NFPM_SHA256"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$output" "$work/nfpm"
tar -xzf "$nfpm_archive" -C "$work/nfpm" nfpm

# すべての形式に含めるファイル
files=$work/files
install -Dm755 "$target/release/nyaw" "$files/nyaw"
# Steam Audio は audio の build.rs が実行ファイルの隣に配置し、実行時も隣から読み込む。ない場合はここで失敗する
install -m644 -t "$files" \
  "$target/release/libphonon.so" "$target/release/steamaudio-THIRDPARTY.md" \
  "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$installer/nyaw.desktop"
install -m644 "$root/assets/logos/Logo_Mini_Light.svg" "$files/nyaw.svg"
docs=(steamaudio-THIRDPARTY.md LICENSE-MIT LICENSE-APACHE)

# AppImage
dir=$work/nyaw.AppDir
install -Dm755 -t "$dir/usr/bin" "$files/nyaw"
install -m644 -t "$dir/usr/bin" "$files/libphonon.so"
for doc in "${docs[@]}"; do
  install -Dm644 -t "$dir/usr/share/doc/nyaw" "$files/$doc"
done
# ルート直下の .desktop、アイコン、.DirIcon は AppImage の仕様が要求する
install -Dm644 -t "$dir/usr/share/applications" "$files/nyaw.desktop"
install -Dm644 -t "$dir/usr/share/icons/hicolor/scalable/apps" "$files/nyaw.svg"
install -m644 -t "$dir" "$files/nyaw.desktop" "$files/nyaw.svg"
ln -s nyaw.svg "$dir/.DirIcon"
# nyaw と子プロセスの環境変数を変えないよう、AppRun はスクリプトではなく実行ファイルへのリンクにする
ln -s usr/bin/nyaw "$dir/AppRun"
mksquashfs "$dir" "$work/nyaw.squashfs" -root-owned -noappend -no-xattrs -comp zstd -quiet -no-progress

# 更新情報（AppImage の仕様）。AppImageUpdate などのツールは、ランタイムの .upd_info セクションから更新の取得先を読む。
# `latest` は正式版だけを対象とするため、プレリリース版は正式版とプレリリースの新しい方を対象とする `latest-all` にする
channel=latest
if [[ $version == *-* ]]; then
  channel=latest-all
fi
update_info="gh-releases-zsync|${repository%/*}|${repository#*/}|$channel|nyaw-*-x86_64.AppImage.zsync"
install -m755 "$runtime" "$work/runtime"
read -r offset size < <(readelf -SW "$work/runtime" |
  awk '{ for (i = 1; i < NF; i++) if ($i == ".upd_info") print $(i + 3), $(i + 4) }')
if [ "${#update_info}" -ge "$((16#$size))" ]; then
  echo "the update information does not fit in .upd_info: $update_info" >&2
  exit 1
fi
printf '%s' "$update_info" | dd of="$work/runtime" bs=1 seek="$((16#$offset))" conv=notrunc status=none

appimage=nyaw-$version-x86_64.AppImage
cat "$work/runtime" "$work/nyaw.squashfs" > "$work/$appimage"
install -m755 "$work/$appimage" "$output/$appimage"
# 差分更新の索引。取得先の URL は索引と同じ場所からの相対パスとする
(cd "$output" && zsyncmake -u "$appimage" -o "$appimage.zsync" "$appimage" > /dev/null)

# tar.gz。展開した nyaw ディレクトリから起動する。package-format はアプリの更新が参照する
mkdir -p "$work/tar/nyaw"
cp -p "$files"/* "$work/tar/nyaw/"
echo tar > "$work/tar/nyaw/package-format"
tar -czf "$output/nyaw-$version-x86_64.tar.gz" --owner=0 --group=0 -C "$work/tar" nyaw

# deb、rpm、pkg.tar.zst
for format in deb rpm pacman; do
  echo "$format" > "$files/package-format-$format"
done
cp "$installer/nfpm.yaml" "$files/"
package() { # package <nfpm の形式> <出力ファイル名>
  (cd "$files" && NYAW_PACKAGE_VERSION=$version "$work/nfpm/nfpm" package -f nfpm.yaml -p "$1" -t "$output/$2")
}
package deb "nyaw-$version-amd64.deb"
package rpm "nyaw-$version-x86_64.rpm"
package archlinux "nyaw-$version-x86_64.pkg.tar.zst"

ls -l "$output"
echo "update information: $update_info"
