#!/usr/bin/env bash
# 作成した配布物を導入して起動を確認する（.github/workflows/linux.yml）。システムを変更するため、使い捨ての環境で実行する。
# 引数: <形式（appimage / tar / deb / rpm / pacman）> <配布物のあるディレクトリ>
set -euo pipefail
format=$1
dist=$(cd "$2" && pwd)
scripts=$(cd "$(dirname "$0")/../crates/subsystems/update/src/linux" && pwd)
sudo=$(command -v sudo || true)
work=$(mktemp -d)

# 確認に使う道具（GPU のない環境で描画する Vulkan の実装と、仮想ディスプレイ）を導入する。
# パッケージは依存関係の宣言も確認するため、nyaw が使うライブラリは AppImage と tar.gz の場合にだけ指定する
# shellcheck disable=SC1091
. /etc/os-release
libraries=()
case $ID in
  debian | ubuntu)
    $sudo apt-get update
    add_packages() { $sudo apt-get install -y --no-install-recommends "$@"; }
    tools=(mesa-vulkan-drivers xvfb xauth)
    alsa=libasound2 pipewire=libpipewire-0.3-0
    if apt-cache show libasound2t64 > /dev/null 2>&1; then
      alsa=libasound2t64 pipewire=libpipewire-0.3-0t64
    fi
    libraries=("$alsa" "$pipewire" libxkbcommon0 libvulkan1 libx11-6 libxcursor1 libwayland-client0 libwayland-cursor0)
    ;;
  fedora)
    add_packages() { $sudo dnf install -y --setopt=install_weak_deps=False "$@"; }
    tools=(mesa-vulkan-drivers xorg-x11-server-Xvfb xorg-x11-xauth findutils diffutils)
    ;;
  arch)
    $sudo pacman -Syu --noconfirm
    add_packages() { $sudo pacman -S --noconfirm --needed "$@"; }
    tools=(vulkan-swrast xorg-server-xvfb xorg-xauth)
    ;;
  *)
    echo "unsupported distribution: $ID" >&2
    exit 2
    ;;
esac
add_packages "${tools[@]}"

one() { # one <ファイル名のパターン>: 一致するファイルが 1 つであることを確認する
  local found=("$dist"/$1)
  [ "${#found[@]}" = 1 ] && [ -f "${found[0]}" ] || {
    echo "expected exactly one $1 in $dist" >&2
    return 1
  }
  echo "${found[0]}"
}

# check_info <update-info のファイル> <アセット名のパターンの末尾>: 同梱の更新情報の内容を確認する
check_info() {
  echo "update information: $(cat "$1")"
  grep -x -E "gh-releases\|[^|]+\|nyaw\|latest(-all)?\|nyaw-\*-$2" "$1"
}

args=()
case $format in
  appimage)
    add_packages "${libraries[@]}"
    nyaw=$(one 'nyaw-*-x86_64.AppImage')
    chmod +x "$nyaw"
    echo "update information: $("$nyaw" --appimage-updateinformation)"
    "$nyaw" --appimage-updateinformation | grep -E '^gh-releases-zsync\|[^|]+\|nyaw\|latest(-all)?\|nyaw-\*-x86_64\.AppImage\.zsync$'
    zsync=$(one 'nyaw-*-x86_64.AppImage.zsync')
    grep -a -x "URL: $(basename "$nyaw")" "$zsync"
    grep -a -x "SHA-1: $(sha1sum "$nyaw" | cut -d' ' -f1)" "$zsync"
    # コンテナでは FUSE を使えないため、展開して実行する
    [ -e /dev/fuse ] || args=(--appimage-extract-and-run)
    ;;
  tar)
    add_packages "${libraries[@]}"
    tar -xzf "$(one 'nyaw-*-x86_64.tar.gz')" -C "$work"
    check_info "$work/nyaw/update-info" 'x86_64\.tar\.gz'
    nyaw=$work/nyaw/nyaw
    ;;
  deb | rpm | pacman)
    case $format in
      deb) suffix=amd64.deb ;;
      rpm) suffix=x86_64.rpm ;;
      pacman) suffix=x86_64.pkg.tar.zst ;;
    esac
    package=$(one "nyaw-*-$suffix")
    # アプリの更新が root で実行するスクリプトで導入し、依存関係の解決とあわせて確認する
    name=$(basename "$package")
    sha=$(sha256sum "$package" | cut -d' ' -f1)
    # 内容が SHA-256 と一致しないパッケージは導入しない
    if $sudo sh "$scripts/install-package.sh" "$package" "$name" "${sha//[0-9a-f]/0}" "$format"; then
      echo "a package with a wrong checksum was installed" >&2
      exit 1
    fi
    [ ! -e /usr/bin/nyaw ]
    $sudo sh "$scripts/install-package.sh" "$package" "$name" "$sha" "$format"
    # 同じパッケージの再導入と、上書きの導入が成功すること
    $sudo sh "$scripts/install-package.sh" "$package" "$name" "$sha" "$format"
    check_info /usr/lib/nyaw/update-info "${suffix//./\\.}"
    [ "$(readlink -f /usr/bin/nyaw)" = /usr/lib/nyaw/nyaw ]
    nyaw=/usr/bin/nyaw
    ;;
  *)
    echo "unknown format: $format" >&2
    exit 2
    ;;
esac

# VR ランタイムのない環境では、エラーをログに記録して終了コード 1 で終了する。起動と動的リンクを確認する
echo "== without a VR runtime"
status=0
timeout 120 "$nyaw" "${args[@]}" > "$work/no-runtime.log" 2>&1 || status=$?
cat "$work/no-runtime.log"
[ "$status" = 1 ]
grep -q 'cannot show an error dialog' "$work/no-runtime.log"

# エミュレーターは自分では終了しないため、時間切れ（終了コード 124）まで動き続ければ成功とする。
# 音声は出力を破棄する ALSA のデバイスにして、同梱の Steam Audio を読み込めることも確認する
echo "== emulator"
export HOME=$work/home XDG_RUNTIME_DIR=$work/runtime
install -d -m 700 "$HOME" "$XDG_RUNTIME_DIR"
printf 'pcm.!default { type null }\nctl.!default { type null }\n' > "$HOME/.asoundrc"
status=0
xvfb-run -a timeout 30 "$nyaw" "${args[@]}" --novr > "$work/novr.log" 2>&1 || status=$?
cat "$work/novr.log"
[ "$status" = 124 ]
! grep -qi 'panic' "$work/novr.log"
grep -q 'spatializer=SteamAudio' "$work/novr.log"
