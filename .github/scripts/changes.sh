#!/usr/bin/env bash
# 標準入力の変更ファイル（1 行 1 件）から、検査が必要な対象を nyaw=<true|false>、web=<true|false> で出力する。
# どちらの対象にもないファイル（ドキュメントなど）だけの変更では、どちらも検査しない。
set -euo pipefail
files=$(cat)
match() { if grep -qE "$1" <<< "$files"; then echo true; else echo false; fi; }
# 呼び出し側の仕組みと改行コードの設定は、両方に影響する。
# .github/scripts/ は含めない（それぞれのテストで確かめる。このスクリプトは changes ジョブが毎回テストする）
shared='\.gitattributes$|\.github/workflows/changes\.yml$'
echo "nyaw=$(match "^(apps/nyaw/|assets/|\.github/workflows/(ci|codeql)\.yml$|$shared)")"
# ウェブサイトはアプリのデザイントークンも読む
echo "web=$(match "^(apps/web/|assets/|apps/nyaw/crates/design/tokens\.json$|(package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml)$|\.github/workflows/web\.yml$|$shared)")"
