// 工具链清单门禁：**这张表是受检数据，不是代码里的一段常量**（见 `框架.md` §4.10）。
//
// 为什么要有这一道（2026-10-18，用户原话："下载时链接表也需要得到严格检验"）：
//   链接表是**唯一的下载来源**。它写错一个字符，用户下到的东西就换了；
//   而"写错了却没人发现"这件事，在这个仓库里已经发生过很多次（门禁数量、七道/八道、C/C++ 少空格）。
//   所以表本身必须**能被打分**：结构缺字段 → 红；主机不在白名单 → 红；哈希格式不对 → 红。
//
// 三道校验里这一道是**静态**的那一道（另两道：下载时校验重定向与哈希、装后跑冒烟）。
// **静态能查的只有形状，查不出"这个 URL 现在还对不对"**——那一半要靠 `verified_at` 和实测，
// 所以这里**不假装**它能证明链接有效：它只保证"长得对、来源在允许范围内"。
//
// 判据（任何一条不成立就红）：
//   1. 表里必须有 `_hosts` 白名单，且每个 host 都写了 `why`（**说不出为什么允许它，就等于没允许**）
//   2. 每条 entry 必须字段齐全：id / version / url / host / sha256 / publisher / homepage / verified_at / size
//   3. `url` 必须 `https://`；URL 里的主机必须**等于** `host` 字段（不靠正则解析，两边对得上才算数）
//   4. `host` 必须在白名单里
//   5. `sha256` 必须是 64 位小写十六进制
//   6. `verified_at` 必须形如 YYYY-MM-DD，且**不许是将来**
//   7. `size` 必须是正整数（下载中断/被替换会立刻露馅）
//
// 诚实边界：`entries` 允许为空（还没填）。这时门禁**明说"0 条"**，
// 不许说"清单没问题"——**空表和好表不是一回事**。

import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..');

const REQUIRED = [
  'id',
  'version',
  'url',
  'host',
  'sha256',
  'publisher',
  'homepage',
  'verified_at',
  'size',
];

/// 从 URL 里抠主机。**故意不用正则**——URL 解析写错一次就是安全边界。
export function hostOf(u) {
  try {
    return new URL(u).host;
  } catch {
    return null;
  }
}

/// 校验一份表。`table` 是已经 parse 好的对象（便于用夹具测）。
export function checkTable(table) {
  const problems = [];

  const hosts = Array.isArray(table?._hosts) ? table._hosts : null;
  if (!hosts || !hosts.length) {
    return {
      problems: ['缺少 _hosts 白名单——没有白名单就不能有下载来源'],
      checked: 0,
      hosts: 0,
    };
  }
  const allow = new Set();
  for (const h of hosts) {
    if (!h || typeof h.host !== 'string' || !h.host.trim()) {
      problems.push('_hosts 里有一条没有 host');
      continue;
    }
    if (typeof h.why !== 'string' || !h.why.trim()) {
      problems.push(`_hosts 里的 ${h.host} 没写 why——**说不出为什么允许它，就等于没允许**`);
    }
    allow.add(h.host);
  }

  const entries = Array.isArray(table?.entries) ? table.entries : null;
  if (!entries) {
    problems.push('缺少 entries（哪怕空数组也要写）');
    return { problems, checked: 0, hosts: allow.size };
  }

  const today = new Date().toISOString().slice(0, 10);
  entries.forEach((e, i) => {
    const at = `entries[${i}]${e && e.id ? `(${e.id})` : ''}`;
    for (const k of REQUIRED) {
      if (!(k in (e ?? {}))) problems.push(`${at} 缺字段 ${k}`);
    }
    if (!e || typeof e !== 'object') return;

    if (typeof e.url !== 'string' || !e.url.startsWith('https://')) {
      problems.push(`${at} 的 url 不是 https:// 开头（明文下载不许出现在表里）`);
    } else {
      const hu = hostOf(e.url);
      if (!hu) problems.push(`${at} 的 url 解析不出主机`);
      else if (e.host !== hu) {
        problems.push(`${at} 的 host 字段是「${e.host}」而 url 里是「${hu}」——**两边必须相等**`);
      }
    }

    if (typeof e.host !== 'string' || !allow.has(e.host)) {
      problems.push(`${at} 的主机「${e.host}」不在 _hosts 白名单里`);
    }

    if (typeof e.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(e.sha256)) {
      problems.push(`${at} 的 sha256 必须是 64 位小写十六进制（当前：${JSON.stringify(e.sha256)}）`);
    }

    if (typeof e.version !== 'string' || !e.version.trim()) {
      problems.push(`${at} 的 version 是空的——版本管理的落点不能空`);
    }

    if (!(e.publisher === null || (typeof e.publisher === 'string' && e.publisher.trim()))) {
      problems.push(`${at} 的 publisher 要么写签名主体、要么写 null（表示没签名，界面必须明说）`);
    }

    if (typeof e.verified_at !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(e.verified_at)) {
      problems.push(`${at} 的 verified_at 必须形如 YYYY-MM-DD`);
    } else if (e.verified_at > today) {
      problems.push(`${at} 的 verified_at 是将来（${e.verified_at}）——实测日期不可能是将来`);
    }

    if (!Number.isInteger(e.size) || e.size <= 0) {
      problems.push(`${at} 的 size 必须是正整数（字节数；少了它，下载被截断也不会发现）`);
    }

    if (typeof e.homepage !== 'string' || !e.homepage.startsWith('https://')) {
      problems.push(`${at} 的 homepage 必须是 https://（给人核对用的官方页）`);
    }
  });

  return { problems, checked: entries.length, hosts: allow.size };
}

export function checkToolchains(root = ROOT) {
  const f = join(root, 'tools', 'toolchains.json');
  if (!existsSync(f)) {
    return { problems: ['tools/toolchains.json 不见了——下载来源没有真源'], checked: 0, hosts: 0 };
  }
  let table;
  try {
    table = JSON.parse(readFileSync(f, 'utf8'));
  } catch (e) {
    return { problems: [`tools/toolchains.json 读不了（${e.message}）`], checked: 0, hosts: 0 };
  }
  return checkTable(table);
}

// ---------------------------------------------------------------------------
// 自测：**校验器本身必须先被证明会红**。八种坏输入各一例，缺一种都算没测。
// ---------------------------------------------------------------------------
const GOOD = {
  id: 'rustup-init',
  version: '1.28.2',
  url: 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe',
  host: 'static.rust-lang.org',
  sha256: 'a'.repeat(64),
  publisher: 'Rust Project Developers',
  homepage: 'https://rustup.rs',
  verified_at: '2026-10-18',
  size: 12345678,
};
const HOSTS = [{ host: 'static.rust-lang.org', why: 'rustup 官方分发域' }];
const wrap = (e) => ({ _hosts: HOSTS, entries: e ? [e] : [] });

export function selftest() {
  const cases = [];
  const ok = checkTable(wrap(GOOD));
  cases.push(['正确的一条 → 过', ok.problems.length === 0, ok.problems]);

  const bad = [
    ['http 明文', { ...GOOD, url: GOOD.url.replace('https://', 'http://') }],
    ['主机不在白名单', { ...GOOD, host: 'evil.example.com' }],
    ['sha256 太短', { ...GOOD, sha256: 'abc' }],
    ['version 空', { ...GOOD, version: '  ' }],
    ['publisher 缺字段', (() => { const x = { ...GOOD }; delete x.publisher; return x; })()],
    ['size 为 0', { ...GOOD, size: 0 }],
    ['verified_at 是将来', { ...GOOD, verified_at: '2099-01-01' }],
    ['host 与 url 不一致', { ...GOOD, host: 'nodejs.org' }],
  ];
  for (const [name, e] of bad) {
    const r = checkTable(wrap(e));
    cases.push([`坏输入：${name} → 必须红`, r.problems.length > 0, r.problems]);
  }

  // 白名单里没写 why 也要红（说不出为什么允许它 = 没允许）
  const noWhy = checkTable({ _hosts: [{ host: 'static.rust-lang.org' }], entries: [GOOD] });
  cases.push(['白名单没写 why → 必须红', noWhy.problems.length > 0, noWhy.problems]);

  const failed = cases.filter(([, pass]) => !pass);
  return { cases, failed };
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const { cases, failed } = selftest();
  for (const [name, pass, problems] of cases) {
    console.log(`  ${pass ? 'ok  ' : 'FAIL'} ${name}${pass ? '' : ' ← ' + problems[0]}`);
  }
  const r = checkToolchains();
  if (r.problems.length) {
    console.log('工具链清单没过：');
    for (const p of r.problems) console.log('   × ' + p);
    process.exit(1);
  }
  if (!r.checked) {
    // **空表不许说"没问题"**：它只是"还没填"。
    console.log(
      `工具链清单：白名单 ${r.hosts} 个主机，**entries 是空的（还没填）**——结构对，但还没有任何可下载项。`,
    );
  } else {
    console.log(`工具链清单通过：白名单 ${r.hosts} 个主机，${r.checked} 条都字段齐全。`);
  }
  process.exit(failed.length ? 1 : 0);
}
