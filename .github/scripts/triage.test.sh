#!/usr/bin/env bash
# triage.sh の判定の確認。`bash .github/scripts/triage.test.sh`
cd "$(dirname "$0")" || exit 1
failed=0
check() { # <pr|issue> <期待するラベル（, 区切り）> <入力>
  local got
  got=$(printf '%b' "$3" | bash triage.sh "$1" | paste -sd ',' -)
  if [ "$got" != "$2" ]; then
    echo "NG: $1 $3 -> $got（期待: $2）"
    failed=1
  fi
}
check pr "app: nyaw,size: XS" 'apps/nyaw/src/main.rs\t9'
check pr "app: web,skip-changelog,size: S" 'apps/web/src/router.tsx\t10\npackage.json\t2'
check pr "app: nyaw,app: web,size: M" 'apps/nyaw/src/main.rs\t40\napps/web/src/router.tsx\t10'
check pr "app: nyaw,app: web,size: XS" 'assets/logos/Logo_Full_Dark.svg\t2'
check pr "repo,size: L" '.github/workflows/ci.yml\t250'
check pr "app: nyaw,size: XS" 'apps/nyaw/Cargo.lock\t5000\napps/nyaw/vendor/gpui/src/app.rs\t900\napps/nyaw/Cargo.toml\t1'
check pr "app: nyaw,size: XL" 'apps/nyaw/src/main.rs\t1000'
# 除外の指定の「.」は、任意の 1 文字ではなく「.」だけに一致する
check pr "app: nyaw,size: M" 'apps/nyaw/CargoXlock	100'
check issue "app: nyaw" '## 対象\r\n- [x] nyaw（アプリ）\r\n- [ ] web（ウェブサイト）\r\n'
check issue "app: nyaw,app: web" '- [X] nyaw\n* [x] web（ウェブサイト）\n'
check issue "" '- [ ] nyaw（アプリ）\n本文に web と書いただけ\n- [x] webhook の追加\n'
check issue "" ''
[ "$failed" = 0 ] && echo "ok"
exit "$failed"
