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


def is_library_surface(path):
    """**crate 根的 `src/lib.rs` 是公共面**，不是公共底层（与 roles.rs 同一判据）。

    ⚠ 这是**第二份**判据（唯一真源在 roles.rs）。放在这里的理由：探针要能独立跑，
    不能依赖 Rust 侧的输出格式。**两处不一致时以 roles.rs 为准**——
    它有自己的单测盯着（`库的根文件是公共面不是公共底层`）。
    """
    p = path.replace("\\", "/")
    return p == "src/lib.rs" or p.endswith("/src/lib.rs")


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
    # **必须含"没有任何边"的文件**：`src/main.rs` 常常一条跨文件调用都没有，
    # 但它是结构入口。只遍历 out|inn 会把它整条漏掉（第一版就这样漏了 1 个）。
    all_files = set(out) | set(inn)
    # **file 节点的路径存在 `id` 里**（`file:src/main.rs`），`file_path` 是 NULL——
    # 这条与 roles.rs 的注释是同一件事，我又踩了一次。
    cur.execute("select id, file_path from nodes where kind='file'")
    for nid, fp in cur.fetchall():
        path = fp or (nid[5:] if nid.startswith("file:") else None)
        if path:
            all_files.add(path)
    rows = []
    for f in all_files:
        if is_test(f):
            continue
        o, i = out.get(f, 0), inn.get(f, 0)
        if is_library_surface(f):
            r = 'surface'
        elif is_bin_target(f):
            r = 'entry'
        elif o >= 3 and o >= 2 * i:
            r = 'entry_candidate'
        elif i >= 3 and i >= 2 * o:
            r = 'foundation'
        else:
            r = 'middle'
        # **只覆盖"按度数算出来的中间层"**：结构入口/公共面是事实，不许被覆盖。
        # （第一版这里无条件覆盖，于是 embed-resource 的 `src/main.rs` 从入口变成"还没连上"。）
        if o + i == 0 and r == 'middle':
            r = 'isolated'
        rows.append((r, o, i, os.path.basename(f)))
    return collections.Counter(r[0] for r in rows), rows, cross, same


def suspicious_foundation(names):
    """**可疑的"公共底层"**：crate 根文件（main.rs / lib.rs / 根上的 mod.rs）不该是底层。

    为什么：它们是被别人*引用*的入口，不是被别人*调用*的工具；
    如果它们进了底层，说明"入度"把"被 use"也算进来了，或者根本没接上。
    """
    bad = []
    for n in names:
        if n in ('main.rs', 'lib.rs', 'mod.rs'):
            bad.append(n)
    return bad


def is_bin_target(path):
    """**Cargo 的 bin 目标**：`main.rs`（任何深度）或 `src/bin/` 下的文件。

    这是**事实**，所以由它判出来的才叫「入口」；按度数判出来的叫「候选」。
    （同 roles.rs；两处不一致以 roles.rs 为准，它有单测。）
    """
    p = path.replace("\\", "/")
    return p == "main.rs" or p.endswith("/main.rs") or "/src/bin/" in p or p.startswith("src/bin/")


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

    print(f"{'项目':<26}{'类型':<5}{'文件':>5}{'跨文件calls':>12}{'入口':>5}{'候选':>5}{'公共面':>6}{'底层':>5}  main.rs 是入口?")
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
        cands = [r[3] for r in rows if r[0] == 'entry_candidate']
        main_is = 'main.rs' in ents
        mark = ('是' if main_is else '否 ⚠') if is_app else '—'
        print(f'{name:<26}{"应用" if is_app else "库":<5}{total:>5}{cross:>12}'
              f'{counts.get("entry", 0):>5}{counts.get("entry_candidate", 0):>5}{counts.get("surface", 0):>6}{counts.get("foundation", 0):>5}  {mark}')
        if ents:
            print(f'    入口名单: {", ".join(ents)}')
        if cands:
            print(f'    候选名单: {", ".join(cands)}')
        founds = [r[3] for r in rows if r[0] == 'foundation']
        if founds:
            print(f'    底层名单: {", ".join(founds)}')
            bad = suspicious_foundation(founds)
            if bad:
                print(f'    ⚠ 可疑底层（crate 根文件不该是底层）: {", ".join(bad)}')
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
