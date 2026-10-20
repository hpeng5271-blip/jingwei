// 真源门禁的一步：把「知识条目严筛 + 待重编回收」接进来（2026-10-20，pro 任务卡）。
//
// 为什么放在真源那一步而不是单开第 5 道：
//   两者语义同源——真源门禁守「文档里的数字 = 代码事实」，这里守「文档里的主张 = 有据可查的事实」。
//   单开一道就是门禁膨胀（pro 的原话），所以 gates.json 的**道数仍是 4**。
//
// 为什么用 `cargo run` 而不是调 `jingwei`：
//   PATH 上那个可能是旧二进制（实测踩过：装完之后新命令"不认识"）。
//   判据只有一份，就在 `crates/cli/src/cmd_knowledge.rs`；这里只负责把它跑起来，不复制判断逻辑。
//
// 跑两条：
//   `knowledge check` —— 条目缺来源/等级、候选来源失效 → 红
//   `knowledge stale` —— 条目引用了图谱里查不到的符号（该重编了）→ N>0 红
//   没有图谱时 stale 会打印「查不了」并 exit 0（干净克隆不该因为还没 index 而红）。
import { execFileSync } from 'node:child_process';

const root = process.cwd();

function run(args) {
  try {
    return {
      code: 0,
      out: execFileSync('cargo', ['run', '-q', '-p', 'jingwei', '--', ...args], {
        cwd: root,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      }),
    };
  } catch (e) {
    return {
      code: typeof e.status === 'number' ? e.status : 1,
      out: `${e.stdout || ''}${e.stderr || ''}`,
    };
  }
}

const check = run(['knowledge', 'check']);
if (check.out.trim()) console.log(check.out.trim());
if (check.code !== 0) {
  console.error(`知识条目严筛未通过（exit=${check.code}）`);
  process.exit(check.code === 1 ? 1 : check.code);
}

const stale = run(['knowledge', 'stale']);
if (stale.out.trim()) console.log(stale.out.trim());
if (stale.code !== 0) {
  console.error(`有待重编的条目（exit=${stale.code}）——改挂：jingwei knowledge fix <条目> --symbol <新符号>`);
  process.exit(1);
}
