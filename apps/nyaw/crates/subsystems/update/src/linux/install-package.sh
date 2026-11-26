#!/bin/sh
# ダウンロードしたパッケージをインストールする。更新時に pkexec が root で実行する（src/linux.rs）。
# 引数: <ファイル> <アセット名> <SHA-256> <形式（deb / rpm / pacman）>
set -eu
file=$1 name=$2 sha256=$3 format=$4

# 認証を待つ間に利用者の権限でファイルを差し替えられないよう、root だけが書き込める場所に複製してから照合する
dir=$(mktemp -d /tmp/nyaw-install.XXXXXX)
trap 'rm -rf "$dir"' EXIT
cp -- "$file" "$dir/$name"
echo "$sha256  $dir/$name" | sha256sum -c - > /dev/null
# apt は権限を下げたユーザー（_apt）でパッケージを読み込む
chmod 755 "$dir"
chmod 644 "$dir/$name"

# 更新の新旧はビルド番号で判定する。バージョン文字列の順序では古くなる版（-patch.<番号> など）も適用できるようにする
case $format in
  deb)
    DEBIAN_FRONTEND=noninteractive apt-get install -y --allow-downgrades "$dir/$name"
    ;;
  rpm)
    if command -v dnf > /dev/null; then
      dnf install -y "$dir/$name"
    elif command -v zypper > /dev/null; then
      zypper --non-interactive install --allow-unsigned-rpm --oldpackage "$dir/$name"
    else
      rpm -U --oldpackage "$dir/$name"
    fi
    ;;
  pacman)
    pacman -U --noconfirm "$dir/$name"
    ;;
  *)
    echo "unknown package format: $format" >&2
    exit 2
    ;;
esac
