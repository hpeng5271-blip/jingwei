// 把 B 站 UP「原子能」的短视频全部转写成文字（2026-10-20）。
//
// **为什么要这个脚本**：用户要求"全部读他的作品"。视频不能直接看/听，
// 唯一诚实做法是：取音轨 → 本地 whisper 转写 → 读文字（ASR 有错字，引用要标）。
//
// **前置（本机一次性）**：
//   1. ffmpeg（任何安装都行；脚本用 JINGWEI_FFMPEG 指定，默认从 PATH 找）
//   2. whisper.cpp 的 whisper-cli.exe + 模型 ggml-small.bin（脚本用 JINGWEI_WHISPER / JINGWEI_WHISPER_MODEL）
//   3. 目录清单：docs/research/atomen-catalog.json（由 tmp 脚本生成，字段 bvid/title/duration）
//
// **可重入**：已经转写过的（目标 txt 存在且非空）直接跳过，所以中断了再跑即可。
// **不懂就跳过**：单条失败只记日志，不中断整批（最后汇总 failed）。
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';

const ROOT = path.resolve(import.meta.dirname, '..', '..');
const OUT = path.join(ROOT, 'docs/research/transcripts');
const CACHE = path.join(process.env.TEMP || '/tmp', 'jw-asr');
const CATALOG = path.join(ROOT, 'docs/research/atomen-catalog.json');
const FFMPEG = process.env.JINGWEI_FFMPEG || 'ffmpeg';
const WHISPER = process.env.JINGWEI_WHISPER || path.join(CACHE, 'wh', 'Release', 'whisper-cli.exe');
const MODEL = process.env.JINGWEI_WHISPER_MODEL || path.join(CACHE, 'ggml-small.bin');
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122 Safari/537.36';
const cookie = 'buvid3=' + crypto.randomUUID().toUpperCase() + 'infoc; b_nut=' + Math.floor(Date.now() / 1000);
const H = { 'User-Agent': UA, Referer: 'https://www.bilibili.com/', Cookie: cookie };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const getJson = async (url, headers = H) => {
  for (let i = 0; i < 4; i++) {
    try {
      const r = await fetch(url, { headers });
      return await r.json();
    } catch {
      await sleep(800 * (i + 1));
    }
  }
  return { code: -1 };
};

const nav = await getJson('https://api.bilibili.com/x/web-interface/nav');
const keyOf = (u) => u.split('/').pop().split('.')[0];
const rawKey = keyOf(nav?.data?.wbi_img?.img_url || '') + keyOf(nav?.data?.wbi_img?.sub_url || '');
const TAB = [46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52];
const mixin = TAB.map((i) => rawKey[i] || '').join('');
const sign = (p) => {
  const wts = Math.floor(Date.now() / 1000);
  const all = { ...p, wts };
  const q = Object.keys(all)
    .sort()
    .map((k) => `${encodeURIComponent(k)}=${encodeURIComponent(String(all[k]).replace(/[!'()*]/g, ''))}`)
    .join('&');
  return `${q}&w_rid=${crypto.createHash('md5').update(q + mixin).digest('hex')}`;
};

fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(CACHE, { recursive: true });
const catalog = JSON.parse(fs.readFileSync(CATALOG, 'utf8').replace(/^\uFEFF/, ''));
const short = catalog.filter((v) => v.duration <= 2700);
console.log(`  目录 ${catalog.length} 条；要转写 ${short.length} 条（>45min 的长直播/专访不在本批）`);

let done = 0;
let skipped = 0;
const failed = [];
for (const v of short) {
  const txt = path.join(OUT, `${v.bvid}.txt`);
  if (fs.existsSync(txt) && fs.statSync(txt).size > 200) {
    skipped += 1;
    continue;
  }
  try {
    const view = await getJson('https://api.bilibili.com/x/web-interface/view?bvid=' + v.bvid);
    if (view.code !== 0) throw new Error('view ' + view.code);
    const cid = view.data.pages[0].cid;
    const play = await getJson('https://api.bilibili.com/x/player/wbi/playurl?' + sign({ bvid: v.bvid, cid, fnval: 16, fnver: 0, fourk: 1 }));
    if (play.code !== 0) throw new Error('playurl ' + play.code);
    const audio = play.data.dash.audio.sort((a, b) => b.bandwidth - a.bandwidth)[0];
    const m4a = path.join(CACHE, `${v.bvid}.m4a`);
    if (!fs.existsSync(m4a)) {
      let ok = false;
      for (let i = 0; i < 4 && !ok; i++) {
        try {
          const r = await fetch(audio.baseUrl, { headers: H });
          if (!r.ok) throw new Error('HTTP ' + r.status);
          fs.writeFileSync(m4a, Buffer.from(await r.arrayBuffer()));
          ok = true;
        } catch {
          await sleep(1000 * (i + 1));
        }
      }
      if (!ok) throw new Error('audio download');
    }
    const wav = path.join(CACHE, `${v.bvid}.wav`);
    if (!fs.existsSync(wav)) {
      execFileSync(FFMPEG, ['-y', '-loglevel', 'error', '-i', m4a, '-ar', '16000', '-ac', '1', '-c:a', 'pcm_s16le', wav]);
    }
    const of = path.join(CACHE, `S-${v.bvid}`);
    execFileSync(WHISPER, ['-m', MODEL, '-f', wav, '-l', 'zh', '-otxt', '-of', of, '-np'], { stdio: 'ignore' });
    const text = fs.readFileSync(`${of}.txt`, 'utf8').trim();
    if (text.length < 200) throw new Error('转写太短 ' + text.length);
    const header = `# ${v.title}\n\n<!-- 来源：https://www.bilibili.com/video/${v.bvid} · 本地 whisper-small 转写 · 有错字，引用需回视频核对 -->\n\n`;
    fs.writeFileSync(txt, header + text, 'utf8');
    done += 1;
    console.log(`  [${done + skipped}/${short.length}] ${v.bvid} ${v.title.slice(0, 30)} (${text.length} 字)`);
  } catch (e) {
    failed.push(`${v.bvid} ${v.title}: ${e.message}`);
    console.log(`  ! ${v.bvid} 失败：${e.message}`);
  }
}
console.log(`  完成。新转写 ${done}，跳过 ${skipped}，失败 ${failed.length}`);
if (failed.length) console.log('  失败清单：\n  ' + failed.join('\n  '));
