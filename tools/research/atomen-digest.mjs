// 从已转写的原子能视频里抽"学习来源 / 学习方法"的句子（2026-10-20）。
//
// 为什么单独抽：用户要的重点不是他讲的技术细节，而是**他从哪学、怎么学**。
// 全量转写有几十万字，人读不完；这里按关键词把相关句子捞出来，保持"来自哪条视频"。
//
// 输出：docs/research/原子能-学习来源摘录.md（每条带 bvid 链接，标 ASR 有错字）。
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..', '..');
const DIR = path.join(ROOT, 'docs/research/transcripts');
const OUT = path.join(ROOT, 'docs/research/原子能-学习来源摘录.md');

// 关键词分三组：来源 / 方法 / 判断
const SOURCES = ['国外', '社区', 'Reddit', 'Hacker', 'YouTube', '油管', '推特', '论文', '会议', '大会', '演讲', 'SIGMOD', 'OSDI', 'NSDI', '文档', '博客', '播客', 'GitHub', '源码', '一手', '官方', '英文', '英语', '论坛', '教材', '课程', '订阅', '邮件列表', '资料', '书'];
const METHODS = ['学', '读', '练', '上手', '沉淀', '教程', '书', '笔记', '总结', '复盘'];
const KEEP = [...SOURCES, ...METHODS];

const files = fs.existsSync(DIR) ? fs.readdirSync(DIR).filter((f) => f.endsWith('.txt')) : [];
const rows = [];
for (const f of files) {
  const raw = fs.readFileSync(path.join(DIR, f), 'utf8');
  const title = (raw.match(/^# (.+)$/m) || [, f])[1];
  const body = raw
    .replace(/^# .+$/m, '')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\s+/g, '');
  const sentences = body.split(/(?<=[。！？!?])/).filter((s) => s.length >= 12 && s.length <= 160);
  const picked = [];
  for (const s of sentences) {
    if (picked.length >= 10) break;
    if (KEEP.some((k) => s.includes(k)) && SOURCES.some((k) => s.includes(k))) picked.push(s);
  }
  if (picked.length) rows.push({ f, title, bvid: f.replace('.txt', ''), picked });
}

let md = `# 原子能 · 学习来源与学习方法摘录（2026-10-20）

> 自动抽自 \`docs/research/transcripts/\`（本地 whisper-small 转写，**有错字**）。
> 规则：句子里同时出现"来源类词"（国外/社区/Reddit/论文/会议/文档/源码/英语…）与"学习类词"。
> 所以这是**索引**，不是结论；引用前要回视频核对。

共 ${rows.length} 条视频有相关句子。

`;
for (const r of rows) {
  md += `## ${r.title}\n\n来源：https://www.bilibili.com/video/${r.bvid}\n\n`;
  for (const s of r.picked) md += `- ${s}\n`;
  md += '\n';
}
fs.writeFileSync(OUT, md, 'utf8');
console.log(`  抽出 ${rows.length} 条视频、${rows.reduce((n, r) => n + r.picked.length, 0)} 句 → ${path.relative(ROOT, OUT)}`);
