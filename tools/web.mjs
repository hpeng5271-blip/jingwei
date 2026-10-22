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
// 取第 n 个"位置参数"（跳过 --flag 与它的值）——否则 `v2ex --limit 2` 会把 "--limit" 当关键词。
const pos = (n = 0) => {
  const out = [];
  for (let i = 0; i < args.length; i += 1) {
    if (args[i].startsWith('--')) { if (args[i + 1] && !args[i + 1].startsWith('--')) i += 1; continue; }
    out.push(args[i]);
  }
  return out[n];
};
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

// ── Lobsters：技术社区，JSON 直接给（无账号） ─────────────
const lobsters = (q, n = limit) => {
  // 实测：hottest/newest/t/<tag>.json 都是 200；`search.json` 返回的是 HTML（不能当 JSON 用）。
  const url = !q ? 'https://lobste.rs/hottest.json'
    : (/^[a-z0-9-]+$/.test(q) ? `https://lobste.rs/t/${encodeURIComponent(q)}.json` : 'https://lobste.rs/hottest.json');
  const j = JSON.parse(curl(url, true));
  return (j || []).slice(0, n).map((it) => ({
    src: 'lobsters',
    title: it.title,
    url: it.url || `https://lobste.rs/s/${it.short_id}`,
    meta: `${it.score || 0} 分 · ${it.comment_count || 0} 评论`,
    extra: (it.tags || []).join(", "),
  }));
};

// ── dev.to：公开文章 API（无账号） ─────────────
const devto = (q, n = limit) => {
  const url = q
    ? `https://dev.to/api/articles?tag=${encodeURIComponent(q)}&per_page=${n}`
    : `https://dev.to/api/articles?per_page=${n}`;
  const j = JSON.parse(curl(url, true));
  return (j || []).slice(0, n).map((it) => ({
    src: 'dev.to',
    title: it.title,
    url: it.url,
    meta: `${it.positive_reactions_count || 0} 赞 · ${it.comments_count || 0} 评论`,
    extra: (it.tag_list || []).join(", "),
  }));
};

// ── V2EX：中文技术社区（无账号） ─────────────
const v2ex = (q, n = limit) => {
  if (q === "hot" || !q) {
    const j = JSON.parse(curl('https://www.v2ex.com/api/topics/hot.json', true));
    return (j || []).slice(0, n).map((it) => ({
      src: 'v2ex',
      title: it.title,
      url: it.url,
      meta: `${it.replies || 0} 回复`,
      extra: (it.node && it.node.title) || "",
    }));
  }
  throw new Error("V2EX 只有热门接口（v2ex hot）；按关键词搜要自己开浏览器");
};

// ── PullPush：Reddit 公开档案（无账号；会 429，慢一点再试） ─────────────
const pullpush = (q, n = limit) => {
  const url = `https://api.pullpush.io/reddit/search/submission/?q=${encodeURIComponent(q)}&size=${n}&sort=desc`;
  const j = JSON.parse(curl(url, true));
  return (j.data || []).slice(0, n).map((it) => ({
    src: 'pullpush',
    title: it.title,
    url: `https://www.reddit.com${it.permalink || ""}`,
    meta: `${it.score || 0} 分 · ${it.num_comments || 0} 评论`,
    extra: it.subreddit || "",
  }));
};
// ── SO 正文：页面被 Cloudflare 保护，但 **API 带 filter=withbody 能拿到正文** ──
const soBody = (qid, site = "stackoverflow", n = 5) => {
  const q = JSON.parse(curl(`https://api.stackexchange.com/2.3/questions/${qid}?site=${site}&filter=withbody`, true));
  const a = JSON.parse(curl(`https://api.stackexchange.com/2.3/questions/${qid}/answers?site=${site}&filter=withbody&sort=votes&order=desc&pagesize=${n}`, true));
  const pill = (it) => ({
    title: decode(it.title || `答案 by ${it.owner && it.owner.display_name}`),
    url: it.link || `https://${site}/q/${qid}`,
    body: stripTags(it.body || ""),
    score: it.score,
  });
  const rows = (q.items || []).map(pill);
  for (const it of (a.items || [])) rows.push(pill(it));
  return rows;
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
      ...safe(() => pullpush(q, 3)),
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
  } else if (cmd === 'lobsters') {
    printRows(safe(() => lobsters(pos(1) || '')), has('json'));
  } else if (cmd === 'devto') {
    printRows(safe(() => devto(pos(1) || '')), has('json'));
  } else if (cmd === 'v2ex') {
    printRows(safe(() => v2ex(pos(1) || 'hot')), has('json'));
  } else if (cmd === 'pullpush') {
    printRows(safe(() => pullpush(pos(1))), has('json'));
  } else if (cmd === 'soq') {
    const qid = pos(1);
    if (!qid) throw new Error("用法：node tools/web.mjs soq <question_id> [--site stackoverflow] [--chars N]");
    const rows = safe(() => soBody(qid, getFlag("site", "stackoverflow")));
    const chars = Number(getFlag('chars', '2500'));
    if (has('json')) { console.log(JSON.stringify(rows, null, 2)); }
    else rows.forEach((r, i) => {
      console.log(`${i === 0 ? "问题" : "答案 " + i}. ${r.title}${typeof r.score === "number" ? `（${r.score} 分）` : ""}`);
      console.log(`   ${r.url}`);
      console.log(`   ${r.body.slice(0, chars)}`);
      console.log("");
    });
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
  soq <question_id>         SO 问题 + 答案正文（走 API，绕开 Cloudflare）
  discord                   为什么读不了
  lobsters [q]              技术社区（JSON）
  devto [tag]               dev.to 文章（JSON）
  v2ex [hot]                V2EX 热门（中文，JSON）
  pullpush "<q>"            Reddit 公开档案（会 429，慢点再试）
公共：--json --no-proxy --proxy <url> --limit N   代理默认 ${proxy || '（不用）'}`);
    process.exit(2);
  }
} catch (e) {
  console.error(`取信失败：${e.message}`);
  process.exit(1);
}
