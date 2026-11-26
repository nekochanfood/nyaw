#!/usr/bin/env bash
# changes.sh の判定の確認。`bash .github/scripts/changes.test.sh`
cd "$(dirname "$0")" || exit 1
failed=0
check() { # <期待する nyaw> <期待する web> <変更ファイル>...
  local want="nyaw=$1 web=$2" got
  shift 2
  got=$(printf '%s\n' "$@" | bash changes.sh | paste -sd ' ')
  if [ "$got" != "$want" ]; then
    echo "NG: $* -> $got（期待: $want）"
    failed=1
  fi
}
check false true apps/web/src/routes/index.tsx
check false true apps/web/package.json pnpm-lock.yaml pnpm-workspace.yaml package.json
check true false apps/nyaw/crates/ui/src/lib.rs
check true false apps/nyaw/Cargo.lock .github/workflows/ci.yml
check true true apps/nyaw/crates/design/tokens.json
check true true assets/logos/Logo_Full_Dark.svg
check true true .github/scripts/changes.sh
check true true apps/web/AGENT.md apps/nyaw/src/main.rs
check false false README.md AGENT.md .github/workflows/release.yml
check false false ""
[ "$failed" = 0 ] && echo "ok"
exit "$failed"
