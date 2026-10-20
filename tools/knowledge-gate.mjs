// 真源门禁的一步：把「知识条目严筛」接进来（2026-10-20，pro 任务卡）。
//
// 为什么放在真源那一步而不是单开第 5 道：
//   两者语义同源——真源门禁守「文档里的数字 = 代码事实」，这里守「文档里的主张 = 有据可查的事实」。
//   单开一道就是门禁膨胀（pro 的原话），所以 gates.json 的**道数仍是 4**。
//
// 为什么用 `cargo run` 而不是调 `jingwei`：
//   PATH 上那个可能是旧二进制（实测踩过：装完之后新命令"不认识"）。
//   判据只有一份，就在 `crates/cli/src/cmd_knowledge.rs`；这里只负责把它跑起来，不复制判断逻辑。
import { execFileSync } from 'node:child_process';

const root = process.cwd();
let out;
try {
  out = execFileSync('cargo', ['run', '-q', '-p', 'jingwei', '--', 'knowledge', 'check'], {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
} catch (e) {
  // 非 0 退出：把它的输出原样转出来（红的时候最要紧的就是那句"哪条不过"）
  const text = `${e.stdout || ''}${e.stderr || ''}`.trim();
  if (text) console.log(text);
  const code = typeof e.status === 'number' ? e.status : 1;
  if (code === 0) process.exit(0);
  console.error(`知识条目严筛未通过（exit=${code}）`);
  process.exit(code === 1 ? 1 : code);
}
console.log(out.trim());
