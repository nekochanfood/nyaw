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

# 更新の新旧はビルド番号で判定する。バージョン文字列の順序では古くなる版（-patch.<番号> など）も適用できるようにする。
# apt と dnf は同じバージョンが導入済みだと何もせずに正常終了し、更新できたと誤って判断される。
# そのため、同じバージョンでもファイルを置き換える指定をする
case $format in
  deb)
    DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall --allow-downgrades "$dir/$name"
    ;;
  rpm)
    # Fedora 45 以降の rpm は署名のないパッケージを拒否する。dnf はファイルを指定した導入では署名を検査しないため、
    # dnf があれば再導入にも使う（内容は上の SHA-256 で照合済み）
    if command -v dnf > /dev/null; then
      # 名前、バージョン、リリース、アーキテクチャが同じパッケージが導入済みの場合は、再導入する
      if rpm -q "$(rpm -qp --nosignature "$dir/$name")" > /dev/null 2>&1; then
        dnf reinstall -y "$dir/$name"
      else
        dnf install -y "$dir/$name"
      fi
    elif command -v zypper > /dev/null; then
      zypper --non-interactive install --allow-unsigned-rpm --oldpackage --force "$dir/$name"
    else
      rpm -U --nosignature --replacepkgs --replacefiles --oldpackage "$dir/$name"
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
