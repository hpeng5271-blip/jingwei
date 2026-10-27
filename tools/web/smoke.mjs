// 前端/协议冒烟：M2 先实现 --proto（hello / 未知 feature 降级 / replay 幂等 / ack）。
// 用法：node tools/web/smoke.mjs --proto [repo]
// 退出码 0 = 全 PASS；1 = 有 FAIL。
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, mkdirSync, existsSync, statSync, unlinkSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";


const JW = process.env.JW || "target/debug/jw.exe".replace(/\//g, process.platform === "win32" ? "\\" : "/");
const args = process.argv.slice(2);
const modes = args.filter((a) => a.startsWith("--"));
if (!modes.length) modes.push("--proto");

function makeRepo() {
  const dir = mkdtempSync(join(tmpdir(), "jw-smoke-"));
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "src", "lib.rs"), "pub fn a() {}\nlet x = 1;\n// note\npub fn b() {}\n");
  writeFileSync(join(dir, "diagnostics.json"), JSON.stringify({
    diagnostics: {
      "src/lib.rs": [
        { line: 1, severity: "error", message: "unused function a" },
        { line: 2, severity: "warning", message: "unused variable x" },
        { line: 3, severity: "info", message: "comment style" }
      ]
    }
  }, null, 2));
  const init = spawnSync(JW, ["init", dir], { encoding: "utf8" });
  if (init.status !== 0) throw new Error("init failed: " + init.stderr);
  const parse = spawnSync(JW, ["parse", dir], { encoding: "utf8" });
  if (parse.status !== 0) throw new Error("parse failed: " + parse.stderr);
  return dir;
}

function serveClient(repo, env = {}) {
  const child = spawn(JW, ["serve", repo], { stdio: ["pipe", "pipe", "inherit"], env: { ...process.env, ...env } });
  let buf = "";
  const frames = [];
  let waiters = [];
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (c) => {
    buf += c;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (!line) continue;
      let msg;
      try { msg = JSON.parse(line); } catch { continue; }
      frames.push(msg);
      waiters = waiters.filter((w) => {
        if (w.match(msg)) { w.resolve(msg); return false; }
        return true;
      });
    }
  });
  const next = (match, timeout = 8000) =>
    new Promise((resolve, reject) => {
      const found = frames.find(match);
      if (found) return resolve(found);
      const t = setTimeout(() => reject(new Error("timeout waiting frame")), timeout);
      waiters.push({ match, resolve: (m) => { clearTimeout(t); resolve(m); } });
    });
  let seq = 0;
  const rpc = (method, extra = {}, matchId, timeoutOverride) => {
    const id = matchId ?? ++seq;
    child.stdin.write(JSON.stringify({ id, method, ...extra }) + "\n");
    return next((m) => m.id === id, timeoutOverride);
  };
  return { next, rpc, close: () => child.kill() };
}

// ---- 窗口/跳转自测：起 bridge + headless Chrome，读页面里的 selftest JSON ----
const CHROME = process.env.CHROME || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }
async function waitHealth(port, tries = 40) {
  for (let i = 0; i < tries; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/health`);
      if (r.ok) return true;
    } catch {}
    await sleep(150);
  }
  return false;
}
function runChromeDom(url) {
  return new Promise((resolve, reject) => {
    const c = spawn(CHROME, ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--virtual-time-budget=8000", "--dump-dom", url], { stdio: ["ignore", "pipe", "pipe"] });
    let out = "";
    c.stdout.setEncoding("utf8");
    c.stdout.on("data", (d) => (out += d));
    c.on("exit", (code) => (code === 0 || out ? resolve(out) : reject(new Error("chrome exit " + code))));
    c.on("error", reject);
  });
}
async function selftest(kind, repo, port, env = {}) {
  const bridge = spawn(process.execPath, ["tools/web/server.mjs", repo, String(port)], { stdio: ["ignore", "pipe", "inherit"], env: { ...process.env, JW, ...env } });
  try {
    if (!(await waitHealth(port))) throw new Error("bridge not ready");
    const dom = await runChromeDom(`http://127.0.0.1:${port}/?selftest=${kind}`);
    const m = dom.match(/<pre id="selftest">([\s\S]*?)<\/pre>/);
    if (!m) throw new Error("selftest output missing");
    const json = JSON.parse(m[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&"));
    for (const c of json.checks) check(`${kind}.${c.name}`, c.ok, c.detail ?? "");
    if (json.pass !== json.total) check(`${kind}.all`, false, `${json.pass}/${json.total}`);
    if (kind === "visual") {
      const outDir = ".pro/exp/out";
      for (const [theme, file] of [["dense", "theme-dense.png"], ["mc", "theme-mc.png"]]) {
        mkdirSync(outDir, { recursive: true });
        const png = resolve(process.cwd(), outDir, file);
        let ok = false, detail = "";
        try { if (existsSync(png)) unlinkSync(png); } catch {}
        for (let attempt = 0; attempt < 2 && !ok; attempt++) {
          const profile = mkdtempSync(join(tmpdir(), "jw-chrome-"));
          const r = spawnSync(CHROME, ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
            `--user-data-dir=${profile}`, "--window-size=1400,900",
            "--virtual-time-budget=9000", `--screenshot=${png}`, `http://127.0.0.1:${port}/?path=src/lib.rs&theme=${theme}`], { encoding: "utf8" });
          ok = r.status === 0 && existsSync(png) && statSync(png).size > 10000;
          detail = ok ? `${statSync(png).size}B` : `${r.status} ${String(r.stderr || "").replace(/\s+/g, " ").slice(-160)}`;
          if (!ok) await sleep(600);
        }
        check(`visual.screenshot.${theme}`, ok, detail);
      }
    }
  } finally {
    bridge.kill();
  }
}

// 缺 jw 时 bridge 必须明确报错并退出，不许 health 空等
function checkMissingJw() {
  return new Promise((resolve) => {
    const c = spawn(process.execPath, ["tools/web/server.mjs", process.cwd(), "7899"], {
      stdio: ["ignore", "ignore", "pipe"],
      env: { ...process.env, JW: join(process.cwd(), "target", "debug", "__no_such_jw__.exe") },
    });
    let err = "";
    c.stderr.setEncoding("utf8");
    c.stderr.on("data", (d) => (err += d));
    const t = setTimeout(() => { c.kill(); resolve({ ok: false, err: "timeout" }); }, 4000);
    c.on("exit", (code) => { clearTimeout(t); resolve({ ok: code === 1 && /jw not found/i.test(err), err, code }); });
  });
}

async function lspGate(repo) {
  const appdata = mkdtempSync(join(tmpdir(), "jw-lsp-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const st = await c.rpc("lsp_status");
  check("lsp.trusted_default_false", st.result.trusted === false, String(st.result.trusted));
  const no = await c.rpc("lsp_spawn");
  check("lsp.untrusted_no_spawn", no.ok === false && no.result.reason === "untrusted_workspace" && no.result.spawned === false, JSON.stringify(no.result));
  const gr = await c.rpc("lsp_trust", { action: "grant" });
  check("lsp.grant", gr.result.trusted === true, JSON.stringify(gr.result));
  const sp = await c.rpc("lsp_spawn");
  check("lsp.spawned_after_trust", sp.ok === true && sp.result.spawned === true && sp.result.source === "lsp", JSON.stringify(sp.result));
  const marker = join(repo, ".jingwei", "lsp-marker");
  check("lsp.marker_exists", existsSync(marker), marker);
  const rv = await c.rpc("lsp_trust", { action: "revoke" });
  const no2 = await c.rpc("lsp_spawn");
  check("lsp.revoked_no_spawn", rv.result.trusted === false && no2.ok === false, JSON.stringify(no2.result));
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lsp.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  c.close();
}

async function lspDiag(repo, port) {
  const appdata = mkdtempSync(join(tmpdir(), "jw-lsp-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const no = await c.rpc("lsp_diagnostics", { path: "src/lib.rs" });
  check("lspdiag.untrusted_no_spawn", no.ok === false && no.result.reason === "untrusted_workspace",
    JSON.stringify(no.result.reason));
  await c.rpc("lsp_trust", { action: "grant" });
  const push = await c.rpc("lsp_diagnostics", { path: "src/lib.rs", mode: "push" });
  const pushItems = (push.result && push.result.items) || [];
  check("lspdiag.push_items", pushItems.length === 3 && push.result.source === "lsp",
    `${pushItems.length}/${push.result.source}`);
  const pull = await c.rpc("lsp_diagnostics", { path: "src/lib.rs", mode: "pull" });
  const pullItems = (pull.result && pull.result.items) || [];
  check("lspdiag.pull_items", pullItems.length === 3 && pull.result.source === "lsp",
    `${pullItems.length}/${pull.result.source}`);
  const mapped = pushItems.every((d) => typeof d.line === "number" && d.line >= 1 && d.col >= 1 && d.file && d.severity && d.source === "lsp");
  check("lspdiag.mapped", mapped, JSON.stringify(pushItems[0] || {}));
  const marker = join(repo, ".jingwei", "lsp-marker");
  check("lspdiag.spawn_marker", existsSync(marker), marker);
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lspdiag.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  await selftest("lsp-diag", repo, port, { APPDATA: appdata });
}

function makeCrate() {
  const dir = mkdtempSync(join(tmpdir(), "jw-ra-crate-"));
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "Cargo.toml"), '[package]\nname = "ra_fixture"\nversion = "0.1.0"\nedition = "2021"\n');
  // 故意的语法错误：RA 必报 >=1 条诊断
  writeFileSync(join(dir, "src/main.rs"), "fn main() {\n    let x: i32 = ;\n    println!(\"{x}\");\n}\n");
  const init = spawnSync(JW, ["init", dir], { encoding: "utf8" });
  if (init.status !== 0) throw new Error("init failed: " + init.stderr);
  const parse = spawnSync(JW, ["parse", dir], { encoding: "utf8" });
  if (parse.status !== 0) throw new Error("parse failed: " + parse.stderr);
  return dir;
}

async function lspReal() {
  const repo = makeCrate();
  const appdata = mkdtempSync(join(tmpdir(), "jw-ra-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const info = await c.rpc("lsp_ra_info");
  if (!info.result.found) {
    skip("lsp-real", "no_server: rust-analyzer 未安装（PATH/JW_RA_PATH/lsp.json 均未命中）");
    c.close();
    return;
  }
  check("lspreal.found", info.result.found === true, info.result.path || "");
  check("lspreal.version", !!(info.result.version || "").match(/rust-analyzer/i), info.result.version || "");
  const no = await c.rpc("lsp_spawn");
  check("lspreal.untrusted_no_spawn", no.ok === false && no.result.reason === "untrusted_workspace", JSON.stringify(no.result.reason));
  await c.rpc("lsp_trust", { action: "grant" });
  const r = await c.rpc("lsp_real", { path: "src/main.rs" }, undefined, 60000);
  const res = r.result || {};
  check("lspreal.diagnostics", r.ok === true && (res.items || []).length >= 1 && res.source === "lsp",
    `${(res.items || []).length} ${res.source} ${JSON.stringify((res.items || [])[0] || {})}`);
  check("lspreal.capability_used", ["pull", "push", "push(implicit)"].includes(res.capabilities_used), String(res.capabilities_used));
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lspreal.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  if (res.pid) {
    const tl = spawnSync("tasklist", ["/FI", `PID eq ${res.pid}`], { encoding: "utf8" });
    const gone = !/rust-analyzer/i.test(tl.stdout || "");
    check("lspreal.process_tree_gone", gone, `pid=${res.pid}`);
  }
}

function makeHoverCrate() {
  const dir = mkdtempSync(join(tmpdir(), "jw-ra-hover-"));
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "Cargo.toml"), '[package]\nname = "ra_hover"\nversion = "0.1.0"\nedition = "2021"\n');
  writeFileSync(
    join(dir, "src/main.rs"),
    "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let x = add(1, 2);\n    println!(\"{x}\");\n}\n"
  );
  spawnSync(JW, ["init", dir], { encoding: "utf8" });
  spawnSync(JW, ["parse", dir], { encoding: "utf8" });
  return dir;
}

async function lspHover(port) {
  const repo = makeHoverCrate();
  const appdata = mkdtempSync(join(tmpdir(), "jw-ra-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const info = await c.rpc("lsp_ra_info");
  if (!info.result.found) {
    skip("lsp-hover", "no_server: rust-analyzer 未安装");
    c.close();
    return;
  }
  check("lsp.ra_found", info.result.found === true, info.result.version || "");
  const no = await c.rpc("lsp_hover", { path: "src/main.rs", line: 5, col: 9 });
  check("lsp.hover_untrusted_fallback",
    no.result.type === null && no.result.source === "db" && no.result.skip === "untrusted_workspace",
    JSON.stringify(no.result.skip));
  await c.rpc("lsp_trust", { action: "grant" });
  const h = await c.rpc("lsp_hover", { path: "src/main.rs", line: 5, col: 9 }, undefined, 60000);
  const hv = h.result || {};
  check("lsp.hover_type", typeof hv.type === "string" && /i32/.test(hv.type) && hv.source === "lsp", JSON.stringify(hv.type));
  check("lsp.hover_position", hv.lsp_line === 4 && hv.lsp_char === 8, `lsp=${hv.lsp_line},${hv.lsp_char}`);
  check("lsp.hover_capability", hv.capabilities_used === "hover", String(hv.capabilities_used));
  const down = await c.rpc("lsp_hover", { path: "src/main.rs", line: 5, col: 9, force_down: true });
  check("lsp.hover_forced_down", down.result.type === null && down.result.skip === "server_down" && down.result.source === "db",
    JSON.stringify(down.result.skip));
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lsp.hover_history_zero", m && m[1] === "0", m ? m[1] : "missing");
  await selftest("lsp-hover", repo, port, { APPDATA: appdata });
}

async function lspDef(port) {
  const repo = makeHoverCrate();
  const appdata = mkdtempSync(join(tmpdir(), "jw-ra-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const info = await c.rpc("lsp_ra_info");
  if (!info.result.found) {
    skip("lsp-def", "no_server: rust-analyzer 未安装");
    c.close();
    return;
  }
  check("lspdef.found", info.result.found === true, info.result.version || "");
  const no = await c.rpc("lsp_def", { path: "src/main.rs", line: 5, col: 13 });
  check("lspdef.untrusted_fallback", no.result.source === "db" && no.result.precision === "low",
    JSON.stringify({ source: no.result.source, skip: no.result.skip }));
  await c.rpc("lsp_trust", { action: "grant" });
  const d = await c.rpc("lsp_def", { path: "src/main.rs", line: 5, col: 13 }, undefined, 60000);
  const dr = d.result || {};
  const tg = (dr.targets || [])[0] || {};
  check("lspdef.target", tg.file === "src/main.rs" && tg.line === 2 && dr.source === "lsp" && dr.precision === "high",
    JSON.stringify(tg));
  check("lspdef.position", dr.lsp_line === 4 && dr.lsp_char === 12, `lsp=${dr.lsp_line},${dr.lsp_char}`);
  const down = await c.rpc("lsp_def", { path: "src/main.rs", line: 5, col: 13, force_down: true });
  check("lspdef.forced_down", down.result.source === "db" && down.result.precision === "low", JSON.stringify(down.result.skip));
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lspdef.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  await selftest("lsp-def", repo, port, { APPDATA: appdata });
}

async function lspRef(port) {
  const repo = makeHoverCrate();
  const appdata = mkdtempSync(join(tmpdir(), "jw-ra-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const info = await c.rpc("lsp_ra_info");
  if (!info.result.found) {
    skip("lsp-ref", "no_server: rust-analyzer 未安装");
    c.close();
    return;
  }
  check("lspref.found", info.result.found === true, info.result.version || "");
  const no = await c.rpc("lsp_ref", { path: "src/main.rs", line: 2, col: 8 });
  check("lspref.untrusted_fallback", no.result.source === "db" && no.result.precision === "low" && !!no.result.coverage,
    JSON.stringify(no.result.skip));
  await c.rpc("lsp_trust", { action: "grant" });
  const warm0 = await c.rpc("lsp_ref", { path: "src/main.rs", line: 2, col: 8 }, undefined, 60000);
  const total = (warm0.result.targets || []).length;
  check("lspref.total", total >= 1 && warm0.result.source === "lsp", `${total}/${warm0.result.source}`);
  const files = new Set((warm0.result.targets || []).map((x) => x.file));
  check("lspref.grouped", files.size >= 1, [...files].join(","));
  const keys = (warm0.result.targets || []).map((x) => `${x.file}:${x.line}:${x.col}`);
  const sortedUnique = keys.length === new Set(keys).size && keys.join("|") === [...warm0.result.targets].sort((a, b) =>
    a.file.localeCompare(b.file) || a.line - b.line || a.col - b.col).map((x) => `${x.file}:${x.line}:${x.col}`).join("|");
  check("lspref.dedup_sorted", sortedUnique, keys.join(","));
  const t0 = Date.now();
  const warm = await c.rpc("lsp_ref", { path: "src/main.rs", line: 2, col: 8 }, undefined, 60000);
  const ms = Date.now() - t0;
  check("lspref.warm_latency", ms <= 1500 && warm.ok === true, `${ms}ms`);
  const down = await c.rpc("lsp_ref", { path: "src/main.rs", line: 2, col: 8, force_down: true });
  check("lspref.forced_down", down.result.source === "db" && down.result.precision === "low" && !!down.result.coverage,
    JSON.stringify(down.result.skip));
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lspref.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  await selftest("lsp-ref", repo, port, { APPDATA: appdata });
}

async function lspHint(port) {
  const repo = makeHoverCrate();
  const appdata = mkdtempSync(join(tmpdir(), "jw-ra-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const info = await c.rpc("lsp_ra_info");
  if (!info.result.found) { skip("lsp-hint", "no_server: rust-analyzer 未安装"); c.close(); return; }
  check("lsp_hint.found", info.result.found === true, info.result.version || "");
  const no = await c.rpc("lsp_hints", { path: "src/main.rs", start: 1, end: 80 });
  check("lsp_hint.untrusted_off", no.result.source === "db" && no.result.skip === "untrusted_workspace" && (no.result.hints || []).length === 0,
    JSON.stringify(no.result.skip));
  await c.rpc("lsp_trust", { action: "grant" });
  const r = await c.rpc("lsp_hints", { path: "src/main.rs", start: 1, end: 40 }, undefined, 60000);
  const hints = (r.result && r.result.hints) || [];
  check("lsp_hint.hints", hints.length >= 1 && r.result.source === "lsp", `${hints.length}/${r.result.source}`);
  check("lsp_hint.cap_200", hints.length <= 200 && r.result.truncated === false, `${hints.length}`);
  check("lsp_hint.fields", hints.every((h) => h.source === "lsp" && h.line >= 1 && h.col >= 1 && h.label), JSON.stringify(hints[0] || {}));
  const warm = await c.rpc("lsp_hints", { path: "src/main.rs", start: 1, end: 40 }, undefined, 60000);
  const down = await c.rpc("lsp_hints", { path: "src/main.rs", start: 1, end: 40, force_down: true });
  check("lsp_hint.forced_down", down.result.source === "db" && down.result.badge === "db" && (down.result.hints || []).length === 0,
    JSON.stringify(down.result.skip));
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lsp_hint.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  await selftest("lsp-hint", repo, port, { APPDATA: appdata });
}

function makeActionCrate() {
  const dir = mkdtempSync(join(tmpdir(), "jw-ra-action-"));
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "Cargo.toml"), '[package]\nname = "ra_action"\nversion = "0.1.0"\nedition = "2021"\n');
  writeFileSync(join(dir, "src/main.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let unused = 1;\n    println!(\"hi\");\n}\n");
  spawnSync(JW, ["init", dir], { encoding: "utf8" });
  spawnSync(JW, ["parse", dir], { encoding: "utf8" });
  return dir;
}

async function lspAction(port) {
  const repo = makeActionCrate();
  const appdata = mkdtempSync(join(tmpdir(), "jw-ra-appdata-"));
  const c = serveClient(repo, { APPDATA: appdata });
  const info = await c.rpc("lsp_ra_info");
  if (!info.result.found) { skip("lsp-action", "no_server: rust-analyzer 未安装"); c.close(); return; }
  check("lsp_action.found", info.result.found === true, info.result.version || "");
  const no = await c.rpc("lsp_actions", { path: "src/main.rs", line: 4, col: 9, message: "unused variable", severity: 2 });
  check("lsp_action.untrusted_hidden", no.result.source === "db" && (no.result.actions || []).length === 0,
    JSON.stringify(no.result.skip));
  await c.rpc("lsp_trust", { action: "grant" });
  const r = await c.rpc("lsp_actions", { path: "src/main.rs", line: 4, col: 9, message: "unused variable: `unused`", severity: 2 }, undefined, 60000);
  const acts = (r.result && r.result.actions) || [];
  check("lsp_action.chips", acts.length >= 1 && r.result.source === "lsp", `${acts.length}/${r.result.source}`);
  check("lsp_action.fields", acts.every((a) => a.source === "lsp" && a.title), JSON.stringify(acts[0] || {}));
  check("lsp_action.applied_zero", r.result.applied === 0, String(r.result.applied));
  const down = await c.rpc("lsp_actions", { path: "src/main.rs", line: 4, col: 9, message: "unused", severity: 2, force_down: true });
  check("lsp_action.forced_down", down.result.source === "db" && (down.result.actions || []).length === 0, JSON.stringify(down.result.skip));
  c.close();
  const chk = spawnSync(JW, ["check", repo], { encoding: "utf8" });
  const m = /lsp_in_history=(\d+)/.exec(chk.stdout || "");
  check("lsp_action.history_zero", m && m[1] === "0", m ? m[1] : "missing");
  await selftest("lsp-action", repo, port, { APPDATA: appdata });
}

function procCount() {
  try {
    const out = spawnSync("C:\\Windows\\System32\\tasklist.exe", [], { encoding: "utf8" }).stdout || "";
    const low = out.toLowerCase();
    const jw = ["jw.exe", "jingwei-gui.exe"].reduce((n, x) => n + (low.split(x).length - 1), 0);
    const all = ["jw.exe", "jingwei-gui.exe", "cmd.exe", "powershell.exe", "explorer.exe"].reduce((n, x) => n + (low.split(x).length - 1), 0);
    return { jw, all };
  } catch { return { jw: -1, all: -1 }; }
}

function commandClosure() {
  const ui = "D:/projects/jingwei-shell/ui";
  const invoked = new Set();
  for (const f of readdirSync(ui)) {
    if (!f.endsWith(".js")) continue;
    const txt = readFileSync(join(ui, f), "utf8");
    for (const m of txt.matchAll(/invoke\('([A-Za-z0-9_:]+)'/g)) invoked.add(m[1]);
  }
  const main = readFileSync("D:/projects/jingwei-shell/src-tauri/src/main.rs", "utf8");
  const i = main.indexOf("tauri::generate_handler![");
  const j = main.indexOf("])", i);
  const block = main.slice(i, j);
  const registered = new Set();
  for (const m of block.matchAll(/^\s*([\w:]+),\s*$/gm)) registered.add(m[1].split("::").pop());
  const missing = [...invoked].filter((c) => !registered.has(c));
  check("ui.command_closure", missing.length === 0, missing.join(","));
}

function uiSyntax() {
  const roots = ["D:/projects/jingwei-shell/ui", "D:/projects/jingwei/tools/web/ui"];
  const bad = [];
  for (const r of roots) {
    for (const f of readdirSync(r)) {
      if (!f.endsWith(".js")) continue;
      const r2 = spawnSync(process.execPath, ["--check", join(r, f)], { encoding: "utf8" });
      if (r2.status !== 0) bad.push(f);
    }
  }
  check("ui.syntax", bad.length === 0, bad.join(","));
}

function uiCopySync() {
  const base = "D:/projects/jingwei-shell/ui";
  const web = "D:/projects/jingwei/tools/web/ui";
  const files = ["core.js","datacore.js","datacore-panel.js","editor.js","notes.js","projects.js","scan.js","scan-extra.js","engine-editor.js"];
  const bad = [];
  for (const f of files) {
    try {
      const a = readFileSync(join(base, f), "utf8");
      const b = readFileSync(join(web, f), "utf8");
      if (a !== b) bad.push(f);
    } catch { bad.push(f + "(missing)"); }
  }
  check("ui.copy_sync", bad.length === 0, bad.join(","));
}

async function shellSmoke(port) {
  commandClosure();
  uiSyntax();
  uiCopySync();
  const bridge = spawn(process.execPath, ["tools/web/server.mjs", ".", String(port)], { stdio: ["ignore", "pipe", "inherit"], env: { ...process.env, JW } });
  try {
    if (!(await waitHealth(port))) throw new Error("bridge not ready");
    const procBefore = procCount();
    const dom = await runChromeDom(`http://127.0.0.1:${port}/ui/index.html?selftest=shell`);
    const procAfter = procCount();
    const m = dom.match(/<pre id="selftest"[^>]*>([\s\S]*?)<\/pre>/);
    let data = null;
    try { data = m ? JSON.parse(m[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">")) : null; } catch {}
    check("shell.selftest", !!data, m ? "found" : "missing");
    if (!data) return;
    check("shell.tabs", data.tabs.length === 6 && data.tabs.join(",") === "home,run,editor,note,machine,clean", data.tabs.join(","));
    check("shell.no_graph", !(data.scripts || []).some((x) => /graph\.js/.test(x)), (data.scripts || []).join(","));
    const pg = data.pages || {};
    check("shell.home", !!(pg.home && pg.home.nodes > 5 && pg.home.textLen > 10), JSON.stringify(pg.home));
    check("shell.projects", !!(pg.home && pg.home.textLen > 10), "");
    check("shell.editor", !!(pg.editor && pg.editor.nodes > 5), JSON.stringify(pg.editor));
    check("shell.note", !!(pg.note && pg.note.textLen > 0), JSON.stringify(pg.note));
    check("shell.machine", !!(pg.machine && pg.machine.nodes > 5), JSON.stringify(pg.machine));
    check("shell.clean", !!(pg.clean && pg.clean.nodes > 5), JSON.stringify(pg.clean));
    const ct = (pg.clean && pg.clean.text) || "";
    const CLEAN_NAMES = ["false_coverage","refuted_without_full","orphan_evidence","orphan_alternative","unregistered_kind","dangling_source","write_log_bad","edge_empty","rules_version_bad","capability_violation","edge_capability_violation","lsp_in_history"];
    const missing = CLEAN_NAMES.filter((n) => !ct.includes(n));
    check("shell.clean_12names", missing.length === 0, missing.join(","));
    const nz = (ct.match(/[1-9]\d*/g) || []).length;
    check("shell.clean_nonzero", nz >= 3, String(nz));
    const mt = (pg.machine && pg.machine.text) || "";
    const hasOs = /(windows|linux|macos)/i.test(mt);
    const hasArch = mt.includes("架构");
    const hasCpu = /逻辑处理器[\s\S]{0,40}(\d+|取不到)/.test(mt);
    check("shell.machine_engine", hasOs && hasArch && hasCpu, JSON.stringify({ hasOs, hasArch, hasCpu }));
    const ht = (pg.home && pg.home.text) || "";
    const hasNum = /文件[\s\S]{0,20}\d+/.test(ht) && /符号[\s\S]{0,20}\d+/.test(ht) && /边[\s\S]{0,20}\d+/.test(ht);
    const hasRepo = ht.includes("jingwei");
    check("shell.home_engine", hasNum && hasRepo, JSON.stringify({ hasNum, hasRepo }));
    const et = (pg.editor && pg.editor.text) || "";
    const hasEng = /状态：(held|refuted|uncertain)/.test(et) && /边 \d+ 条/.test(et) && /仅结构边/.test(et);
    const hasMeta = /未由内核提供（T6）/.test(et);
    check("shell.editor_engine", hasEng && hasMeta, JSON.stringify({ hasEng, hasMeta }));
    const escText = (pg.editor && pg.editor.escape) || "";
    check("shell.editor_outside_root", escText.includes("outside root"), escText.slice(0, 100));
    check("shell.editor_cite", /(commit:|skip:)/.test(et), (et.match(/(commit:|skip:\w+)/) || [""])[0]);
    check("shell.editor_refs", /引用|references/i.test(et) && /symbol:[^#]+#/.test(et), (et.match(/symbol:[^#]+#[\w]+/g) || []).slice(0, 2).join(","));
    const p1 = (pg.editor && pg.editor.preview1) || "";
    const p2 = (pg.editor && pg.editor.preview2) || "";
    const pEsc = (pg.editor && pg.editor.previewEsc) || "";
    check("shell.preview_open", /symbol:/.test(p1), p1.slice(0, 80));
    const firstSym = (p1.match(/symbol: ([^ \n]+)/) || [])[1] || "___";
    check("shell.preview_replace", p2.length > 0 && !p2.includes(firstSym), JSON.stringify({ p2: p2.slice(0, 60), firstSym }));
    check("shell.preview_esc", pEsc.trim() === "点一条引用看预览" || pEsc.length > 0, pEsc.slice(0, 60));
    check("shell.root_loaded", typeof data.rootAt === "string" && data.rootAt.length > 0, String(data.rootAt));
    const nt = (pg.note && pg.note.text) || "";
    check("shell.note_t6", nt.includes("文档索引未由内核提供（T6）"), nt.slice(0, 80));
    const rt = (pg.run && pg.run.text) || "";
    check("shell.run_t6", rt.includes("终端未接"), rt.slice(0, 80));
    check("shell.run_deny", /外部操作未实现|已禁用/.test(rt), rt.slice(0, 120));
    check("shell.run_no_spawn", procBefore.jw === procAfter.jw && procBefore.jw >= 0, `jw ${procBefore.jw}->${procAfter.jw} · all ${procBefore.all}->${procAfter.all}`);
    check("shell.console_errors", (data.consoleErrors || []).length === 0, JSON.stringify((data.consoleErrors || []).slice(0, 3)));
    check("shell.unmapped_visible", Array.isArray(data.unmapped), String((data.unmapped || []).length));
  } finally {
    bridge.kill();
  }
}

const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? "  " + detail : ""}`);
}
function skip(name, detail = "") {
  results.push({ name, skip: true, detail });
  console.log(`SKIP ${name}${detail ? "  " + detail : ""}`);
}

for (const mode of modes) {
if (mode === "--lsp-action") {
  const port = 7800 + Math.floor(Math.random() * 90);
  await lspAction(port);
} else if (mode === "--shell") {
  const port = 7800 + Math.floor(Math.random() * 90);
  await shellSmoke(port);
} else if (mode === "--lsp-hint") {
  const port = 7800 + Math.floor(Math.random() * 90);
  await lspHint(port);
} else if (mode === "--lsp-ref") {
  const port = 7800 + Math.floor(Math.random() * 90);
  await lspRef(port);
} else if (mode === "--lsp-def") {
  const port = 7800 + Math.floor(Math.random() * 90);
  await lspDef(port);
} else if (mode === "--lsp-hover") {
  const port = 7800 + Math.floor(Math.random() * 90);
  await lspHover(port);
} else if (mode === "--lsp-real") {
  await lspReal();
} else if (mode === "--lsp-diag") {
  const repo = makeRepo();
  const port = 7800 + Math.floor(Math.random() * 90);
  await lspDiag(repo, port);
} else if (mode === "--lsp-gate") {
  const repo = makeRepo();
  await lspGate(repo);
} else if (["--windows", "--jump", "--code", "--problems", "--theme", "--visual"].includes(mode)) {
  const repo = makeRepo();
  const port = 7800 + Math.floor(Math.random() * 90);
  await selftest(mode.slice(2), repo, port);
} else if (mode === "--proto") {
  const repo = makeRepo();
  const c = serveClient(repo);
  // case 1: 主动 hello + 握手
  const hello = await c.next((m) => m.type === "hello");
  check("hello.proactive", hello.proto === 1 && Array.isArray(hello.features) && String(hello.rules).length > 0,
    `proto=${hello.proto} rules=${hello.rules} features=${hello.features?.length}`);
  const hs = await c.rpc("hello", { features: ["hello", "nope"] });
  check("hello.degraded", hs.ok === true && (hs.result.degraded || []).includes("nope"),
    `degraded=${JSON.stringify(hs.result.degraded)}`);
  // case 2: 写一次 → 有事件；同 since 重放幂等
  await c.rpc("refresh");
  const ev1 = await c.rpc("events_since", { since: 0 });
  const ev2 = await c.rpc("events_since", { since: 0 });
  check("replay.idempotent",
    JSON.stringify(ev1.result.events) === JSON.stringify(ev2.result.events) && ev1.result.events.length >= 1,
    `events=${ev1.result.events.length} gap=${ev1.result.gap} truncated=${ev1.result.truncated}`);
  // case 3: ack 裁剪
  const last = ev1.result.events.at(-1)?.seq ?? 0;
  const ack = await c.rpc("ack", { seq: last });
  check("ack.trims", ack.ok === true && ack.result.dropped >= 1, `dropped=${ack.result.dropped}`);
  c.close();
  // 缺 jw：bridge 必须明确报错并 exit 1
  const mj = await checkMissingJw();
  check("bridge.missing_jw", mj.ok, mj.err ? mj.err.trim().slice(0, 90) : "");
} else {
  console.log(`SKIP ${mode}`);
}
}

const failed = results.filter((r) => !r.ok && !r.skip).length;
const skipped = results.filter((r) => r.skip).length;
console.log(`\n${results.length - failed - skipped}/${results.length - skipped} PASS` + (skipped ? `  SKIP=${skipped}` : ""));
process.exit(failed === 0 ? 0 : 1);
