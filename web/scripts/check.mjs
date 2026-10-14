// lint / fmt:check / typecheck / build を並列に流す（直列だと各 npm 起動と tsc で 14 秒前後かかる）。
// 1 つでも失敗したら、その出力をまとめて出して非 0 で終わる。
import { spawn } from "node:child_process";
import path from "node:path";

// node_modules/.bin を PATH に足して直接呼ぶ（npm run を 4 回起動するより速い）。
const env = {
  ...process.env,
  PATH: path.resolve("node_modules/.bin") + path.delimiter + process.env.PATH,
};
const tasks = {
  lint: "oxlint -c web/.oxlintrc.json --deny-warnings web crates/ui/src/web",
  "fmt:check": "oxfmt -c web/.oxfmtrc.json --check web crates/ui/src/web",
  typecheck: "tsc --noEmit --incremental --tsBuildInfoFile node_modules/.cache/tsc.tsbuildinfo",
  build: "vite build",
};

const run = (name, cmd) =>
  new Promise((resolve) => {
    // lint と fmt はリポジトリのルートから流す（パスが package.json の lint と同じ）。
    const cwd = name === "lint" || name === "fmt:check" ? ".." : ".";
    const p = spawn(cmd, { cwd, env, shell: true });
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (out += d));
    p.on("close", (code) => resolve({ name, code, out }));
  });

const results = await Promise.all(Object.entries(tasks).map(([n, c]) => run(n, c)));
let failed = false;
for (const { name, code, out } of results) {
  console.log(`${code === 0 ? "ok  " : "FAIL"} ${name}`);
  if (code !== 0) {
    failed = true;
    console.log(out);
  }
}
process.exit(failed ? 1 : 0);
