#!/usr/bin/env bash
# Issue と PR に付けるラベルを 1 行 1 件で出力する。
#   triage.sh pr    : 標準入力は変更ファイルの「パス<TAB>変更行数」
#   triage.sh issue : 標準入力は Issue の本文
set -euo pipefail

# 規模に数えないファイル（生成物と、取り込んだ上流のソース）
generated='(^|/)(Cargo\.lock|pnpm-lock\.yaml|routeTree\.gen\.ts)$|^apps/nyaw/vendor/'

case "${1:-}" in
  pr)
    input=$(cat)
    paths=$(cut -f1 <<< "$input")
    has() { grep -qE "$1" <<< "$paths"; }
    nyaw=false web=false
    # assets/ は両方が使う
    if has '^(apps/nyaw|assets)/'; then nyaw=true; echo "app: nyaw"; fi
    if has '^(apps/web|assets)/'; then web=true; echo "app: web"; fi
    if ! $nyaw && ! $web; then echo "repo"; fi
    # アプリのリリースノートから除く（.github/release.yml）
    if $web && ! $nyaw; then echo "skip-changelog"; fi
    # awk の -v はバックスラッシュを解釈するため、正規表現は環境変数で渡す
    lines=$(SKIP=$generated awk -F'\t' '$1 !~ ENVIRON["SKIP"] { sum += $2 } END { print sum + 0 }' <<< "$input")
    if ((lines < 10)); then
      echo "size: XS"
    elif ((lines < 50)); then
      echo "size: S"
    elif ((lines < 250)); then
      echo "size: M"
    elif ((lines < 1000)); then
      echo "size: L"
    else
      echo "size: XL"
    fi
    ;;
  issue)
    # テンプレートの「対象」でチェックした項目
    body=$(cat)
    for app in nyaw web; do
      if grep -qE "^[[:space:]]*[-*] \[[xX]\] $app([^[:alnum:]_-]|$)" <<< "$body"; then
        echo "app: $app"
      fi
    done
    ;;
  *)
    echo "usage: triage.sh pr|issue" >&2
    exit 2
    ;;
esac
