#!/usr/bin/env node
// 外网取信（2026-10-22）：走本机代理，只用**不需要账号**的公开入口。
//
// 实测结论（同一台机器、同一代理，18:30）：
//   StackOverflow/SE API  ✓   GitHub issues API ✓   Hacker News(Algolia) ✓
//   Discourse 论坛 search.json ✓   old.reddit HTML ✓
//   www.reddit.com/*.json ✗403   DuckDuckGo HTML ✗202 挑战页   Bing RSS ✗返回垃圾结果
//   Discord ✗ 要 bot token（匿名读不了，不假装能做）
//
// 用法：
//   node tools/web.mjs search "webview2 white screen"      # 多源：GitHub issues + HN + StackOverflow
//   node tools/web.mjs so "…" [--site stackoverflow]
//   node tools/web.mjs gh "…" [--repo owner/name]
//   node tools/web.mjs hn "…"
//   node tools/web.mjs reddit "…" | reddit r/<sub> [--limit N]
//   node tools/web.mjs discourse <host> "…"                 # 例：users.rust-lang.org
//   node tools/web.mjs fetch <url> [--chars N] [--save 主题]
//   公共：--json --no-proxy --proxy <url> --limit N

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const cmd = args[0];
const getFlag = (k, d) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 && args[i + 1] && !args[i + 1].startsWith('--') ? args[i + 1] : d;
};
const has = (k) => args.includes(`--${k}`);
const limit = Number(getFlag('limit', '5'));
const proxy = has('no-proxy') ? null : getFlag('proxy', process.env.JINGWEI_PROXY || 'http://127.0.0.1:7897');
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36';

const decode = (s) => String(s || '')
  .replace(/<!\[CDATA\[|\]\]>/g, '')
  .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"')
  .replace(/&#39;|&apos;/g, "'").replace(/&amp;/g, '&').replace(/&nbsp;/g, ' ')
  .replace(/&#(\d+);/g, (_, d) => String.fromCharCode(Number(d)));
const stripTags = (s) => decode(String(s || '')
  .replace(/<script[\s\S]*?<\/script>/gi, ' ').replace(/<style[\s\S]*?<\/style>/gi, ' ')
  .replace(/<[^>]+>/g, ' ')).replace(/\s+/g, ' ').trim();

function curl(url, json = false) {
  const a = ['-sS', '-L', '--max-time', '40', '-A', UA];
  if (json) a.push('-H', 'Accept: application/json');
  if (proxy) a.push('-x', proxy);
  a.push(url);
  const r = spawnSync('curl.exe', a, { encoding: 'utf8', timeout: 60000, maxBuffer: 32 * 1024 * 1024 });
  if (r.error) throw new Error(`curl 起不来：${r.error.message}`);
  if (r.status !== 0) throw new Error(`curl 退出码 ${r.status}：${(r.stderr || '').slice(0, 200)}`);
  return r.stdout || '';
}

const se = (q, site = 'stackoverflow', n = limit) => {
  const j = JSON.parse(curl(`https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance`
    + `&q=${encodeURIComponent(q)}&site=${site}&pagesize=${n}`, true));
  return (j.items || []).map((it) => ({
    src: site === 'stackoverflow' ? 'SO' : site,
    title: decode(it.title),
    url: it.link,
    meta: `${it.score} 分 / ${it.answer_count} 答${it.is_answered ? '·已解决' : ''}`,
    extra: (it.tags || []).join(', '),
  }));
};

const gh = (q, repo = '', n = limit) => {
  const qq = repo ? `${q} repo:${repo}` : q;
  const j = JSON.parse(curl(`https://api.github.com/search/issues?q=${encodeURIComponent(qq)}&per_page=${n}&sort=reactions`, true));
  return (j.items || []).map((it) => ({
    src: 'GH',
    title: it.title,
    url: it.html_url,
    meta: `${it.state} · ${it.comments} 评论`,
    extra: it.repository_url ? it.repository_url.replace('https://api.github.com/repos/', '') : '',
  }));
};

const hn = (q, n = limit) => {
  const j = JSON.parse(curl(`https://hn.algolia.com/api/v1/search?query=${encodeURIComponent(q)}&tags=story&hitsPerPage=${n}`, true));
  return (j.hits || []).map((it) => ({
    src: 'HN',
    title: it.title || it.story_title || '',
    url: it.url || `https://news.ycombinator.com/item?id=${it.objectID}`,
    meta: `${it.points || 0} 分 · ${it.num_comments || 0} 评论`,
    extra: it.created_at ? it.created_at.slice(0, 10) : '',
  }));
};

const discourse = (host, q, n = limit) => {
  const j = JSON.parse(curl(`https://${host}/search.json?q=${encodeURIComponent(q)}`, true));
  return (j.topics || []).slice(0, n).map((t) => ({
    src: host.split('.')[0],
    title: t.title,
    url: `https://${host}/t/${t.slug}/${t.id}`,
    meta: `${t.posts_count || 0} 帖 · ${(t.created_at || '').slice(0, 10)}`,
    extra: (t.tags || []).join(', '),
  }));
};

const oldReddit = (q, n = limit) => {
  const url = q.startsWith('r/')
    ? `https://old.reddit.com/${q}/new`
    : `https://old.reddit.com/search?q=${encodeURIComponent(q)}&sort=relevance&t=all`;
  const html = curl(url);
  // Reddit 从 2026 起对匿名访问给"欢迎页/登录墙"：能拿到 300KB HTML，但里面没有一条帖子。
  // 这条**必须如实说查不了**，不能返回空数组让人以为"没有讨论"。
  if (/Welcome to Reddit|log in to Reddit|enable JavaScript/i.test(html) && !/\/comments\//.test(html)) {
    throw new Error('Reddit 匿名读不了：返回的是登录墙/欢迎页（要登录态或官方 API key）。SO / GitHub / HN / Discourse 都可用');
  }
  const rows = [];
  const seen = new Set();
  const re = /<a[^>]+href="((?:https:\/\/old\.reddit\.com)?\/r\/[^"]*\/comments\/[^"]*)"[^>]*>([\s\S]*?)<\/a>/g;
  let m;
  while ((m = re.exec(html)) && rows.length < 80) {
    const raw = decode(m[1]);
    const link = raw.startsWith('http') ? raw : `https://old.reddit.com${raw}`;
    const title = stripTags(m[2]);
    if (!title || title.length < 8 || seen.has(link)) continue;
    seen.add(link);
    rows.push({ src: 'reddit', title, url: link });
  }
  const words = q.toLowerCase().split(/\s+/).filter((w) => w.length > 2);
  const kept = q.startsWith('r/') ? rows : rows.filter((r) => words.some((w) => r.title.toLowerCase().includes(w)));
  return kept.slice(0, n);
};

const printRows = (rows, asJson) => {
  if (asJson) {
    console.log(JSON.stringify(rows, null, 2));
    return;
  }
  if (!rows.length) {
    console.log('（没取到——如实说"查不了"，别编）');
    return;
  }
  rows.forEach((r, i) => {
    console.log(`${i + 1}. [${r.src}] ${r.title}`);
    console.log(`   ${r.url}`);
    if (r.meta) console.log(`   ${r.meta}${r.extra ? ' · ' + r.extra : ''}`);
  });
};

const safe = (f) => {
  try {
    return f();
  } catch (e) {
    return [{ src: 'error', title: `取信失败：${e.message}`, url: '', meta: '' }];
  }
};

try {
  if (cmd === 'search') {
    const q = args[1];
    if (!q) throw new Error('用法：node tools/web.mjs search "<关键词>"');
    const rows = [
      ...safe(() => gh(q, '', 3)),
      ...safe(() => se(q, 'stackoverflow', 3)),
      ...safe(() => hn(q, 3)),
    ];
    printRows(rows, has('json'));
  } else if (cmd === 'so') {
    printRows(safe(() => se(args[1], getFlag('site', 'stackoverflow'))), has('json'));
  } else if (cmd === 'gh') {
    printRows(safe(() => gh(args[1], getFlag('repo', ''))), has('json'));
  } else if (cmd === 'hn') {
    printRows(safe(() => hn(args[1])), has('json'));
  } else if (cmd === 'reddit') {
    printRows(safe(() => oldReddit(args[1])), has('json'));
  } else if (cmd === 'discourse') {
    const host = args[1];
    const q = args[2];
    if (!host || !q) throw new Error('用法：node tools/web.mjs discourse <host> "<关键词>"');
    printRows(safe(() => discourse(host, q)), has('json'));
  } else if (cmd === 'fetch') {
    const url = args[1];
    if (!/^https?:\/\//.test(url || '')) throw new Error('用法：node tools/web.mjs fetch <http(s)://…> [--chars N] [--save 主题]');
    const chars = Number(getFlag('chars', '4000'));
    const text = stripTags(curl(url)).slice(0, chars);
    const save = getFlag('save', '');
    if (save) {
      const dir = path.join(process.cwd(), 'docs', 'research');
      fs.mkdirSync(dir, { recursive: true });
      const day = new Date().toISOString().slice(0, 10);
      const file = path.join(dir, `${day}-${save.replace(/[^\w\u4e00-\u9fa5-]+/g, '-')}.md`);
      const body = [`# ${save}（${day} 抓取）`, '', `> 来源：${url}`, '> 抓取方式：`node tools/web.mjs fetch`（curl 走代理），**去标签原文**，未做摘要。', '', text, ''].join('\n');
      fs.writeFileSync(file, body, 'utf8');
      console.log(text);
      console.log(`\n已存：${path.relative(process.cwd(), file)}`);
    } else {
      console.log(text);
    }
  } else if (cmd === 'discord') {
    console.log('Discord：**匿名读不了**（三态：查不了）。');
    console.log('官方 API 要 bot token；社区内容不在公开搜索索引里。');
    console.log('能用的只有：有邀请链接时人工看，或自建 bot 并授权——后者要装东西，先由 pro 裁定。');
  } else {
    console.log(`用法：
  search "<q>"              多源（GitHub issues + StackOverflow + HN）
  so "<q>" [--site …]       StackExchange API
  gh "<q>" [--repo o/n]     GitHub issues
  hn "<q>"                  Hacker News（Algolia）
  reddit "<q>" | r/<sub>    old.reddit
  discourse <host> "<q>"    Discourse 论坛（如 users.rust-lang.org）
  fetch <url> [--chars N] [--save 主题]
  discord                   为什么读不了
公共：--json --no-proxy --proxy <url> --limit N   代理默认 ${proxy || '（不用）'}`);
    process.exit(2);
  }
} catch (e) {
  console.error(`取信失败：${e.message}`);
  process.exit(1);
}
