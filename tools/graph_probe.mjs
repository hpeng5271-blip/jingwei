// **图谱质量探针**：量"这个图谱能不能回答模块依赖"。
//
// 为什么它是一个常驻工具（而不是一次性脚本）：
// 图谱的质量指标（跨文件依赖密度、入口的出入度）**不是靠读代码能看出来的**，
// 每改一次解析都要重量一次。实测证明它就是靠这个量出"跨文件只有 80 条"那个根因的。
//
// 用法：node tools/graph_probe.mjs [导出 json 路径]
import { readFileSync } from 'node:fs';

const p = process.argv[2] || (process.env.TEMP + '/jw-idx2.json');
const g = JSON.parse(readFileSync(p, 'utf8'));

const fileOf = (id) => String(id).replace(/^symbol:/, '').split('#')[0];
const kinds = {};
for (const e of g.edges) kinds[e.kind] = (kinds[e.kind] || 0) + 1;

const inner = new Map();
const cross = new Map();
for (const e of g.edges) {
  if (e.kind !== 'calls') continue;
  const a = fileOf(e.src), b = fileOf(e.dst);
  if (a === b) inner.set(a, (inner.get(a) || 0) + 1);
  else {
    const k = a + ' -> ' + b;
    cross.set(k, (cross.get(k) || 0) + 1);
  }
}
const innerTotal = [...inner.values()].reduce((x, y) => x + y, 0);
const crossTotal = [...cross.values()].reduce((x, y) => x + y, 0);

const out = new Map();
const inn = new Map();
for (const k of cross.keys()) {
  const [a, b] = k.split(' -> ');
  out.set(a, (out.get(a) || 0) + 1);
  inn.set(b, (inn.get(b) || 0) + 1);
}
const files = new Set(g.nodes.map((n) => fileOf(n.id)).filter(Boolean));
const linked = new Set([...out.keys(), ...inn.keys()]);

console.log(`  节点 ${g.nodes.length} / 边 ${g.edges.length}   边类型 ${JSON.stringify(kinds)}`);
console.log(`  calls：文件内 ${innerTotal}（${inner.size} 个文件）/ **跨文件 ${crossTotal}**（${cross.size} 个方向）`);
console.log(`  有跨文件依赖的文件：${linked.size} / ${files.size}   孤立 ${files.size - linked.size}`);
console.log('');
console.log('  出入度最高的文件（入口与枢纽该在这里）：');
const deg = new Map();
for (const f of linked) deg.set(f, (out.get(f) || 0) + (inn.get(f) || 0));
for (const [f, d] of [...deg.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8)) {
  console.log(`    ${String(d).padStart(3)}  ${f}   出 ${out.get(f) || 0} / 入 ${inn.get(f) || 0}`);
}
