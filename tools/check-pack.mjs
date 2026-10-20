// 交付门禁：**读一遍打好的压缩包，看里面装的是不是该装的东西**。
//
// ## 为什么要有这一道（拿两次真事故换的）
//
// 2026-10-16 重新打包时，我顺手读了一遍文件清单，才发现两件事：
//   1. `jingwei-demo\.jingwei`（工具自己的数据目录）被当成**第四个演示项目**打进去了
//      —— `-Directory` 枚举到它，而它是过程产物，`graph.db` 里还带着本机的绝对路径；
//   2. 演示项目里混着 `__pycache__` / `*.pyc`（我跑演示时留下的编译产物）。
//
// **两件都不是任何测试发现的，是"读清单"发现的。** 而"读清单"这件事很容易忘——
// 所以把它变成会失败的东西。
//
// ## 它查什么
//
//   ① 该有的都在：两个 exe、run.cmd、说明书、演示走法、演示项目目录
//   ② 不该有的都没有：`.jingwei/`、`__pycache__/`、`*.pyc`、`target/`、`graph.db`
//   ③ 两个 exe 的时间不早于源码里最新的那个 .rs（否则装的是旧二进制——
//      这条正是"包落后于代码"那一次的教训）
//
// 用法：node tools/check-pack.mjs [zip 路径]
//      不给路径就找 dist/ 下最新的那个。

import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

/**
 * 压缩包里的条目名**统一按正斜杠 `/` 看**。
 *
 * ⚠️ 这句话第一版是错的：它断言"压缩包里就是 `/`，与平台无关"。
 * 而 `pack.ps1` 用 Windows PowerShell 5.1 的 `Compress-Archive`，**存的是反斜杠**
 * （实测：48 个条目含 `/` 的 0 个、含 `\` 的 42 个）。于是这个文件里按 `/` 写的规则
 * **全部失效**——2026-10-16 由外部对照复核抓出，修法是在读取处规范化（见 `rows` 那段）。
 * 规则仍然按 `/` 写：**规则表达的是逻辑路径，分隔符是打包器的细节。**
 */
const FORBIDDEN = [
  { re: /(^|\/)\.jingwei\//, why: '工具自己的数据目录（过程产物，graph.db 里带本机绝对路径）' },
  { re: /__pycache__\//, why: 'Python 编译产物' },
  { re: /\.pyc$/, why: 'Python 编译产物' },
  { re: /(^|\/)target\//, why: 'Rust 构建产物' },
  // 注意：**不禁止 `*.db`。** 演示项目里放一个 graph.db 当样例数据是合法的，
  // 硬禁会误报（第一版就是这么写的，当场报了一条假的）。工具自己的库只在 `.jingwei/` 下，
  // 上面那条已经把它挡住了。
];

/** 顶层必须有这些（名字里带中文，所以按"以什么结尾"匹配）。 */
const REQUIRED = [
  { test: (n) => /^[^/]+\.exe$/.test(n) && n.includes('jingwei-gui'), why: '桌面程序' },
  { test: (n) => n.endsWith('.md') && n.includes('使用说明'), why: '说明书' },
  // **2026-10-20 用户改的交付口径**：包只装「GUI 程序 + 说明书」两样，
  // 命令行版、run.cmd、演示走法、演示项目都不再进包（省体积、少一处会过期的副本）。
  // 这条口径变了就要改这里——门禁守的是"现在说好的那件事"，不是历史。
];

/**
 * 包里那个 exe 是从哪些源码编出来的 —— **只比这些**。
 *
 * 2026-10-20 抓到一次假红：`jingwei-gui` 只依赖 `jingwei-engine → jingwei-model`（见 `src-tauri/Cargo.toml`），
 * 而我改了 `crates/cli`（新命令）之后，交付门禁报「包里的 exe 比源码旧 45.4 分钟」——
 * 可 GUI 二进制根本不含 cli 代码，那次"旧"是**判据把不相干的源码算进来了**。
 * 判据错向"假红"比错向"假绿"安全，但假红会逼人重打包或关掉门禁，最后一样失守。
 * 所以：**比 GUI 的依赖闭包**。哪天包里重新带 CLI，就把 `crates/cli` 加回这个表。
 */
const PACK_SOURCE_ROOTS = [
  join(ROOT, 'crates', 'model'),
  join(ROOT, 'crates', 'engine'),
  join(ROOT, 'src-tauri'),
];

/** 依赖闭包里最新的 .rs 的修改时间——用来判断"包是不是落后于代码"。 */
function newestSourceMtime() {
  let newest = 0;
  const walk = (dir) => {
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return;
    }
    for (const name of names) {
      if (['target', 'node_modules', '.git', 'dist'].includes(name)) continue;
      const p = join(dir, name);
      const st = statSync(p);
      if (st.isDirectory()) walk(p);
      else if (name.endsWith('.rs')) newest = Math.max(newest, st.mtimeMs);
    }
  };
  for (const r of PACK_SOURCE_ROOTS) walk(r);
  return newest;
}

/**
 * 最新的"包"：优先看 `dist/*.zip`，没有就看 `dist/<YYYY-MM-DD>/` 目录。
 *
 * 2026-10-20 用户改的口径：产物**不带版本号**，只按日期放一个文件夹
 * （`dist/2026-10-20/` 里只有 GUI 程序 + 说明书）。门禁跟着这个形状走：
 * 目录也是包，判据一模一样（该有的在、不该有的没有、exe 不比源码旧）。
 */
function newestPack() {
  const dir = join(ROOT, 'dist');
  if (!existsSync(dir)) return null;
  const zips = readdirSync(dir)
    .filter((n) => n.endsWith('.zip') && !n.startsWith('_'))
    .map((n) => join(dir, n));
  const dated = readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isDirectory() && /^\d{4}-\d{2}-\d{2}$/.test(e.name))
    .map((e) => join(dir, e.name));
  const all = [...zips, ...dated];
  if (!all.length) return null;
  return all.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
}

/** 目录形态的包：把文件列成"相对路径 → {len, mtimeMs}"，与 zip 那套同一个形状。 */
function walkPack(root) {
  const out = [];
  const walk = (dir, prefix) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${e.name}` : e.name;
      const full = join(dir, e.name);
      if (e.isDirectory()) walk(full, rel);
      else {
        const st = statSync(full);
        out.push({ name: rel, len: st.size, mtimeMs: st.mtimeMs });
      }
    }
  };
  walk(root, '');
  return out;
}

export function checkPack(packPath) {
  const problems = [];
  const zip = packPath || newestPack();
  if (!zip) {
    return { problems: ['dist/ 下没有任何包——还没打过包'], entries: 0, zip: null };
  }
  const isDir = statSync(zip).isDirectory();
  if (isDir) {
    const rows = walkPack(zip);
    return checkRows(rows, zip, problems);
  }

  // 用 PowerShell 列条目：**不引任何依赖**（这个仓库连打包器都没有）。
  // 列出的是 `路径|字节数|写入时间(ISO 8601)`。
  //
  // **为什么是 ISO 而不是 ticks**：见下面 ③ 那段的注释——ticks 不带时区，
  // 拿到之后无论怎么手工换算都要猜"写入方在哪个时区"，有夏令时的地区必然错一小时。
  // ISO 串自带那一刻的偏移，`Date.parse` 一步到位。
  const script =
    'Add-Type -AssemblyName System.IO.Compression.FileSystem;' +
    `$z=[System.IO.Compression.ZipFile]::OpenRead('${zip.replace(/'/g, "''")}');` +
    'try { $z.Entries | ForEach-Object { $_.FullName + "|" + $_.Length + "|" + $_.LastWriteTime.ToString("o") } }' +
    ' finally { $z.Dispose() }';
  let raw;
  try {
    raw = execFileSync('powershell', ['-NoProfile', '-Command', script], { encoding: 'utf8', maxBuffer: 1 << 26 });
  } catch (e) {
    return { problems: [`读不了压缩包：${String(e.message).slice(0, 120)}`], entries: 0, zip };
  }
  const rows = raw.split(/\r?\n/).filter(Boolean).map((l) => {
    const [name, len, iso] = l.split('|');
    // **把名字里的反斜杠规范成正斜杠。**
    //
    // 为什么必须有这一步（2026-10-16，外部对照复核抓出的 A 档）：
    // 上面那句注释原来写着"压缩包里的路径分隔符是 `/`，与平台无关"——**那是错的**。
    // `pack.ps1` 用的是 Windows PowerShell 5.1 的 `Compress-Archive`，
    // **它把条目名存成反斜杠**。实测这个包：48 个条目里含 `/` 的 **0** 个、含 `\` 的 **42** 个。
    //
    // 后果是 `FORBIDDEN` 那三条（`.jingwei/` / `__pycache__/` / `target/`）
    // **在本仓库实际打出的包上一条都不会命中**——也就是说账本里那两次真事故
    // （工具自己的 `.jingwei` 被当成第四个演示项目打进包、演示项目里混着 `__pycache__`）
    // **今天再发生一次，这道门禁一声不响**。而它恰恰是当初为了那两次事故才加的。
    //
    // 只在**读取处**规范，规则本身仍按 `/` 写——规则是"逻辑路径"，分隔符是打包器的细节。
    const norm = name.replace(/\\/g, '/');
    return { name: norm, len: Number(len), mtimeMs: Date.parse(iso) || 0 };
  });
  return checkRows(rows, zip, problems);
}

/** 判据本体：①该有的 ②不该有的 ③包是不是落后于代码。zip 与目录两种形态共用。 */
function checkRows(rows, pack, problems) {
  if (!rows.length) return { problems: ['包里一个条目都没有——包坏了'], entries: 0, zip: pack };

  // ① 该有的
  for (const want of REQUIRED) {
    if (!rows.some((r) => want.test(r.name) || want.test(r.name + '/'))) {
      problems.push(`包里缺「${want.why}」`);
    }
  }
  // ② 不该有的（按**目录**报一次，不然一个 __pycache__ 会刷出二十行）
  const seenDir = new Set();
  for (const r of rows) {
    for (const f of FORBIDDEN) {
      if (!f.re.test(r.name)) continue;
      const dir = r.name.split('/').slice(0, -1).join('/') || '(顶层)';
      const key = f.why + '|' + dir;
      if (seenDir.has(key)) continue;
      seenDir.add(key);
      problems.push(`包里不该有「${f.why}」：${dir}/`);
    }
  }
  // ③ 包是不是落后于代码
  //
  // ⚠️ **这里踩过一个伪装得很好的时区 bug（2026-10-16 抓到）**：
  // `.NET` 的 `LastWriteTime.Ticks` 换算出来是**本地时间意义上的毫秒**
  // （它从计时原点起算，`.NET` 再按本地偏移解释），而 `statSync().mtimeMs` 是
  // **epoch 毫秒（等价 UTC）**。两者直接相减就差了整整一个时区（这里 8 小时），
  // 于是"源码更新"永远算成负数 —— **判据恒假，这道检查永远通过**。
  //
  // 症状特别隐蔽：门禁**看起来在守**，而且它守的东西（"包比源码旧"）
  // 正是这个仓库最贵的一课。所以这个 bug 的形态就是它自己要去防的那种事故。
  //
  // **第二个修法（同一天，外部对照复核补的）**：第一版修成
  // `ticksMs + new Date().getTimezoneOffset() * 60_000`——用**此刻**的偏移去解释
  // **过去写入的**墙上时间。在 UTC+8 这种全年偏移不变的地方是对的，但在有夏令时的地区
  // 跨切换点会差 60 分钟（> 10 分钟容差）：**正常包报红、落后的包静默通过**。
  //
  // 现在改成**读 ISO 时间**：`.NET` 把 zip 里的 DOS 时间按**本地**解释，
  // 输出的 ISO 串自带那一刻正确的偏移，`Date.parse` 一步得到 epoch——
  // 与 `statSync().mtimeMs` 天然同一把尺子，**不需要手工加偏移，也就没有 DST 问题**。
  // （根因是 ticks 不带时区；只要还在手工换算，就永远要猜"写入方在哪个时区"。）
  const exes = rows.filter((r) => r.name.endsWith('.exe') && r.mtimeMs > 0);
  const newestExe = exes.reduce((a, b) => (a && a.mtimeMs > b.mtimeMs ? a : b), null);
  if (newestExe) {
    const exeMs = newestExe.mtimeMs;
    const src = newestSourceMtime();
    const TOLERANCE_MS = 10 * 60_000;
      // **节奏变了（2026-10-20）**：默认不打包，"包比源码旧"是常态，不该天天红。
      // 只有收束打包时才要求新鲜：JINGWEI_DELIVER=1。
      if (src > 0 && exeMs + TOLERANCE_MS < src) {
        const d = ((src - exeMs) / 60000).toFixed(1);
        const msg = '包里的 exe 比源码旧 ' + d + ' 分钟（exe ' + new Date(exeMs).toLocaleString('zh-CN')
          + '，最新源码 ' + new Date(src).toLocaleString('zh-CN') + '）';
        if (process.env.JINGWEI_DELIVER === '1') {
          problems.push(msg + '——收束打包时不允许旧包，重新构建再打包');
        } else {
          console.log('  · 提示：' + msg + '（未收束打包，允许）');
        }
      }
  }

  return { problems, entries: rows.length, zip: pack };
}

if (process.argv[1] && process.argv[1].endsWith('check-pack.mjs')) {
  const ifPresent = process.argv.includes('--if-present');
  const explicit = process.argv.slice(2).find((a) => !a.startsWith('--'));
  const target = explicit || newestPack();

  // `--if-present`：没打过包就**如实跳过**，不当失败。
  // 为什么：干净克隆不该因为"还没打包"而红——那不是缺陷，是没做那件事。
  // 但**有了包就必须验**：一旦 dist/ 里躺着 zip，它就得是对的。
  if (!target && ifPresent) {
    console.log('交付门禁：dist/ 里还没有包，跳过（想验先跑 pack.ps1）。');
    process.exit(0);
  }

  const r = checkPack(target);
  if (r.problems.length) {
    console.log(`交付门禁未通过（${r.zip ? relative(ROOT, r.zip) : '没有包'}，${r.entries} 个条目）：`);
    for (const p of r.problems) console.log('   × ' + p);
    process.exit(1);
  }
  console.log(`交付门禁通过：${relative(ROOT, r.zip)} 里 ${r.entries} 个条目，该有的都在、不该有的没有、exe 新鲜度见上面的提示。`);
}
