#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""**角色判定的可信度探针**：拿真项目量"入口 / 公共底层"到底有多准。

## 为什么要有这个脚本

2026-10-18：拿 `fd` 量出来「入口 5 个里只有 1 个是真的」。但**一个项目不能证明规则普遍成立**，
所以把测量做成可复跑的东西：样本换一批，结论就跟着换，**不靠我记**。

## 判据（与 `crates/engine/src/graph/roles.rs` 一致，别处不许再写一份）

- 只算 `calls` 边（`contains` / `defines` 是包含，不是使用）
- 聚到文件层面，**同文件调用不算**
- 测试文件不参与（`/tests/`、`*_test.rs`、`tests.rs`）
- 入口：出 ≥ 3 且 出 ≥ 2×入；公共底层：入 ≥ 3 且 入 ≥ 2×出；其余中间层；出+入=0 叫"还没连上"

## 怎么读输出

`main.rs 是入口?` 这一列是**唯一的基准**：
- 应用：`main.rs` 就是真入口，判对了打"是"，判错了打"否"
- 库：**没有入口这个概念**（库要被别人用）——所以这一列打"—"，
  并且**库不该出现在"入口准不准"的统计里**（2026-10-18 这条差点算错）

用法：
    python tools/roles_probe.py                # 默认样本
    python tools/roles_probe.py <项目目录> ...  # 指定目录
"""
import collections
import os
import sqlite3
import subprocess
import sys

EXE = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                   'target', 'release', 'jingwei.exe')


def is_test(path):
    p = path.replace('\\', '/').lower()
    return '/tests/' in p or p.endswith('_test.rs') or p.endswith('tests.rs')


def roles_of(db):
    con = sqlite3.connect(db)
    cur = con.cursor()
    node = {}
    cur.execute('select id, kind, name, file_path from nodes')
    for i, _k, _n, f in cur.fetchall():
        node[i] = f
    out, inn = collections.Counter(), collections.Counter()
    cross = same = 0
    cur.execute("select src, dst from edges where kind='calls'")
    for s, d in cur.fetchall():
        fa, fb = node.get(s), node.get(d)
        if not fa or not fb:
            continue
        if fa == fb:
            same += 1
            continue
        cross += 1
        out[fa] += 1
        inn[fb] += 1
    rows = []
    for f in set(out) | set(inn):
        if is_test(f):
            continue
        o, i = out.get(f, 0), inn.get(f, 0)
        if o >= 3 and o >= 2 * i:
            r = 'entry'
        elif i >= 3 and i >= 2 * o:
            r = 'foundation'
        else:
            r = 'middle'
        if o + i == 0:
            r = 'isolated'
        rows.append((r, o, i, os.path.basename(f)))
    return collections.Counter(r[0] for r in rows), rows, cross, same


def default_samples():
    """默认样本：3 个真应用（真入口 = src/main.rs）。"""
    home = os.environ['USERPROFILE']
    reg = os.path.join(home, '.cargo', 'registry', 'src')
    out = []
    fd = os.path.join(os.environ.get('TEMP', ''), 'jw-real', 'fd')
    if os.path.isdir(fd):
        out.append(('fd', fd))
    for name in ('cargo-watch-8.5.3', 'embed-resource-3.0.12'):
        if not os.path.isdir(reg):
            break
        for d in os.listdir(reg):
            p = os.path.join(reg, d, name)
            if os.path.isdir(p):
                out.append((name, p))
                break
    return out


def main():
    targets = [(os.path.basename(p.rstrip('\\/')), p) for p in sys.argv[1:]] or default_samples()
    if not targets:
        print('没有样本：给几个项目目录，或先造 D:\\projects\\jingwei-demo')
        return 1
    if not os.path.isfile(EXE):
        print(f'找不到 {EXE}：先 cargo build --release -p jingwei')
        return 1

    print(f"{'项目':<26}{'类型':<5}{'文件':>5}{'跨文件calls':>12}{'入口':>5}{'底层':>5}  main.rs 是入口?")
    apps = entries = correct = 0
    for name, path in targets:
        subprocess.run([EXE, 'index', '--root', path], capture_output=True, text=True,
                       encoding='utf-8', errors='replace')
        db = os.path.join(path, '.jingwei', 'graph.db')
        if not os.path.isfile(db):
            print(f'  {name}: 没建出图谱库（先跑一次 index）')
            continue
        counts, rows, cross, _same = roles_of(db)
        is_app = os.path.isfile(os.path.join(path, 'src', 'main.rs'))
        total = sum(counts.values())
        ents = [r[3] for r in rows if r[0] == 'entry']
        main_is = 'main.rs' in ents
        mark = ('是' if main_is else '否 ⚠') if is_app else '—'
        print(f'{name:<26}{"应用" if is_app else "库":<5}{total:>5}{cross:>12}'
              f'{counts.get("entry", 0):>5}{counts.get("foundation", 0):>5}  {mark}')
        if ents:
            print(f'    入口名单: {", ".join(ents)}')
        if is_app:
            apps += 1
            entries += len(ents)
            correct += 1 if main_is else 0
    if apps:
        print()
        print(f'  应用 {apps} 个 · 判出的"入口"共 {entries} 个 · 其中真的是 main.rs 的 {correct} 个'
              f' → **准确率 {correct * 100 // max(entries, 1)}%**')
        print('  （库不算进这个准确率：库没有"入口"这回事，它要被别人用）')
    return 0


if __name__ == '__main__':
    sys.exit(main())
