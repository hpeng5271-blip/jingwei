// 工具链下载器（**只下载 + 校验 + 落盘，不安装、不提权、不动系统**）。
//
// 见 `框架.md` §4.10 的三级校验：这一份是**「下载时」那一级**。
//   1. 只跟随 **≤1 次** 重定向，且重定向目标**必须仍在 `_hosts` 白名单里**
//   2. 实算 SHA-256 必须等于表里的值
//   3. 字节数必须等于表里的 `size`（下载被截断/被替换立刻露馅）
//   4. 表里有 `publisher` 时，用 Authenticode 验签名主体；`publisher: null` 时**明说"未签名"**
//   任何一项不过：**删掉半成品**，报清是哪一项、期望值/实际值各是什么。
//
// 用法：
//   node tools/fetch-toolchain.mjs <id> [--out <目录>]
//   node tools/fetch-toolchain.mjs --selftest      （验校验器自己会红，不联网）
//
// 诚实边界：本工具**不执行**下载到的东西。安装/装配是另一件事（见 §4.10 第四节）。

import { createHash } from 'node:crypto';
import { createWriteStream, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join, basename } from 'node:path';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..');

export function loadTable(root = ROOT) {
  return JSON.parse(readFileSync(join(root, 'tools', 'toolchains.json'), 'utf8'));
}

export function sha256Of(path) {
  const h = createHash('sha256');
  h.update(readFileSync(path));
  return h.digest('hex');
}

/// Windows Authenticode 主体（读不出就 null）。**只读，不执行那个文件。**
export function publisherOf(path) {
  if (process.platform !== 'win32') return null;
  try {
    const out = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `(Get-AuthenticodeSignature -LiteralPath '${path.replace(/'/g, "''")}').SignerCertificate.Subject`,
      ],
      { encoding: 'utf8', timeout: 30000 },
    );
    const s = String(out).trim();
    return s || null;
  } catch {
    return null;
  }
}

/// 校验一个已经落盘的文件。
///
/// **返回两类，别混**（2026-10-18 自测抓出来的）：
///   - `problems`：硬失败，文件必须删掉（哈希/体积/签名主体不符）
///   - `notes`：**必须说出来但不是失败**的事（例如"这个文件本来就没签名"）
///
/// 第一版把"未签名"写进了 `problems`，照那样跑 `rustup-init`（官方就是未签名）会被当场删掉——
/// **拿"没签名"当"校验失败"，是把一个事实判成了一次失败。**
export function verifyFile(entry, path) {
  const problems = [];
  const notes = [];
  if (!existsSync(path)) return { problems: [`文件不存在：${path}`], notes };

  const size = statSync(path).size;
  if (size !== entry.size) {
    problems.push(`字节数不对：期望 ${entry.size}，实际 ${size}（下载被截断或被换过）`);
  }
  const got = sha256Of(path);
  if (got !== entry.sha256) {
    problems.push(`sha256 不对：期望 ${entry.sha256}，实际 ${got}`);
  }
  if (entry.publisher === null) {
    // 没有期望主体 = 我们接受它未签名，但**必须说出来**，不能当成"验过了"。
    notes.push("这个文件未签名（表里 publisher 就是 null）——没有被 Authenticode 验证过");
  } else {
    const who = publisherOf(path);
    if (!who) {
      problems.push(`读不出签名主体（期望「${entry.publisher}」）`);
    } else if (!who.includes(entry.publisher)) {
      problems.push(`签名主体不符：期望「${entry.publisher}」，实际「${who}」`);
    }
  }
  return { problems, notes };
}

/// 跟随重定向，但**最多 1 次**，且每一次的目标主机都必须在白名单里。
async function getFollowingSafe(url, allow) {
  const hops = [];
  let cur = url;
  for (let i = 0; i < 2; i++) {
    const r = await fetch(cur, { redirect: 'manual' });
    if (r.status >= 300 && r.status < 400) {
      const loc = r.headers.get('location');
      if (!loc) throw new Error(`重定向没有 Location：${cur}`);
      const abs = new URL(loc, cur).toString();
      const host = new URL(abs).host;
      hops.push(host);
      if (hops.length > 1) throw new Error(`重定向超过 1 次（到 ${abs}）——不允许`);
      if (!allow.has(host)) throw new Error(`重定向目标主机「${host}」不在白名单里`);
      cur = abs;
      continue;
    }
    if (!r.ok) throw new Error(`HTTP ${r.status}：${cur}`);
    return { res: r, finalUrl: cur, hops };
  }
  throw new Error('重定向处理循环异常');
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--selftest')) return selftest();

  const id = args.find((a) => !a.startsWith('--'));
  if (!id) {
    console.error('用法：node tools/fetch-toolchain.mjs <id> [--out <目录>]');
    process.exit(2);
  }
  const outIdx = args.indexOf('--out');
  const outDir = outIdx >= 0 && args[outIdx + 1] ? args[outIdx + 1] : process.cwd();

  const table = loadTable();
  const entry = (table.entries || []).find((e) => e.id === id);
  if (!entry) {
    console.error(`表里没有 id=${id}`);
    process.exit(2);
  }
  const allow = new Set((table._hosts || []).map((h) => h.host));

  mkdirSync(outDir, { recursive: true });
  const target = join(outDir, basename(new URL(entry.url).pathname));
  const part = `${target}.part`;

  console.log(`下载 ${entry.id} ${entry.version} → ${target}`);
  try {
    const { res, finalUrl, hops } = await getFollowingSafe(entry.url, allow);
    if (hops.length) console.log(`  重定向 1 次 → ${finalUrl}`);
    await pipeline(Readable.fromWeb(res.body), createWriteStream(part));
  } catch (e) {
    if (existsSync(part)) rmSync(part, { force: true });
    console.error(`  ✗ 下载失败：${e.message}`);
    process.exit(1);
  }

  const { problems, notes } = verifyFile(entry, part);
  if (problems.length) {
    rmSync(part, { force: true }); // **不留半成品**
    console.error('  ✗ 校验没过（半成品已删除）：');
    for (const p of problems) console.error('     × ' + p);
    process.exit(1);
  }

  // 过 = 改名成正式文件；**并把这四样实测值打出来**，让人能自己复核。
  rmSync(target, { force: true });
  execFileSync('powershell', ['-NoProfile', '-Command', `Move-Item -LiteralPath '${part}' -Destination '${target}' -Force`]);
  console.log('  ✓ 通过：');
  console.log(`     sha256 = ${entry.sha256}`);
  console.log(`     size   = ${entry.size}`);
  console.log(`     kind   = ${entry.publisher === null ? '未签名（表里写的就是 null）' : entry.publisher}`);
  for (const n of notes) console.log(`     ! ${n}`);
  console.log(`     落在   = ${target}`);
  console.log('  本工具**没有执行**它，也没有改动系统。');
}

// ---------------------------------------------------------------------------
// 自测：**校验器必须先被证明会红**，而且不联网。
// ---------------------------------------------------------------------------
export function selftest() {
  const cases = [];
  const tmp = join(process.env.TEMP || '.', `jw-fetch-selftest-${process.pid}`);
  mkdirSync(tmp, { recursive: true });
  const f = join(tmp, 'blob.bin');
  const content = Buffer.from('hello-jingwei');
  writeFileSync(f, content);
  const real = createHash('sha256').update(content).digest('hex');

  const base = { id: 'x', version: '1', size: content.length, sha256: real, publisher: null };

  // 正确的字节数 + 正确的哈希 + publisher=null → 只该剩一条"未签名"的说明
  const ok = verifyFile(base, f);
  cases.push([
    'publisher=null → problems 为空，notes 里明说未签名',
    ok.problems.length === 0 && ok.notes.some((n) => n.includes('未签名')),
    [...ok.problems, ...ok.notes],
  ]);

  const wrongSha = verifyFile({ ...base, sha256: "a".repeat(64) }, f);
  cases.push([
    'sha256 不符 → 必须红',
    wrongSha.problems.some((p) => p.includes('sha256')),
    wrongSha.problems,
  ]);

  const wrongSize = verifyFile({ ...base, size: 99999 }, f);
  cases.push([
    '体积不符 → 必须红',
    wrongSize.problems.some((p) => p.includes('字节数')),
    wrongSize.problems,
  ]);

  const missing = verifyFile(base, join(tmp, "nope.bin"));
  cases.push([
    '文件不存在 → 必须红',
    missing.problems.some((p) => p.includes('不存在')),
    missing.problems,
  ]);

  // 签名读取器：拿一个**系统自带且有签名**的文件验它真能读出主体（不是空实现）
  if (process.platform === "win32") {
    const signed = join(process.env.WINDIR || "C:/Windows", "System32", "notepad.exe");
    const who = publisherOf(signed);
    cases.push(["签名读取器能读出系统文件的主体", !!who && who.length > 10, [String(who)]]);
  }
  rmSync(tmp, { recursive: true, force: true });
  const failed = cases.filter(([, pass]) => !pass);
  for (const [name, pass, problems] of cases) {
    console.log(`  ${pass ? 'ok  ' : 'FAIL'} ${name}${pass ? '' : ' ← ' + problems[0]}`);
  }
  process.exit(failed.length ? 1 : 0);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) await main();
