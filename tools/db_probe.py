#!/usr/bin/env python3
"""数据层探针：把《数据层设计》里的每个数字变成可复跑的命令。

为什么要有这个：设计里写"1.21M 行 12s、77MB、点查 0.00ms"——
如果没人能重跑，那些数字就是传说。这个脚本用**系统自带的 Python + 标准库 sqlite3**
（不需要装任何东西）建一个同样结构的库，灌数据、量体积、看查询计划、跑不变量检查。

用法：
    python tools/db_probe.py                     # 默认：10k 文件 / 20 符号每文件
    python tools/db_probe.py --files 25000 --spf 20 --eps 4   # 更大规模，找拐点

输出可直接粘进设计文档；凡是文档里的数字，都应该能用这个脚本复现。
"""

import argparse
import os
import sqlite3
import sys
import tempfile
import time

# 建库语句：与《数据层设计》§3 一字不差（改设计就必须改这里，反之亦然）
# 建库语句：**唯一真源是 tools/schema.sql**，本文件不再自己写一份。
#
# 这里原来内嵌过一份，而且已经和 schema.sql 对不上了（少了 files / index_runs /
# query_cache 三张支持表，触发器名也不一样）。两份 DDL = "表结构到底是什么"有两个答案，
# 而压测数字又是拿其中一份跑出来的——那数字就没意义了。
SCHEMA_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema.sql")


def load_schema():
    with open(SCHEMA_PATH, encoding="utf-8") as f:
        return f.read()


def connect(path):
    c = sqlite3.connect(path)
    c.isolation_level = None
    # 用 executescript 而不是逐条 execute：schema.sql 里触发器体内部有分号，
    # 按 ";" 切会把 BEGIN…END 切坏。
    c.executescript(load_schema())
    return c


def sym_id(i, spf):
    f = i // spf
    return f"symbol:src/m{f // 100}/f{f}.rs#s{i}"


def fill(c, files, spf, eps):
    t0 = time.time()
    c.execute("BEGIN")
    c.executemany(
        "INSERT INTO nodes(id,kind,name,file_path,line,language,source,created_at,updated_at)"
        " VALUES(?,?,?,?,?,?,?,1,1)",
        [
            (f"file:src/m{i // 100}/f{i}.rs", "file", f"f{i}.rs",
             f"src/m{i // 100}/f{i}.rs", None, "rust", "ts")
            for i in range(files)
        ],
    )
    n_sym = files * spf
    c.executemany(
        "INSERT INTO nodes(id,kind,name,file_path,line,language,source,confidence,created_at,updated_at)"
        " VALUES(?,?,?,?,?,?,?,1.0,1,1)",
        [
            (sym_id(i, spf), "symbol", f"s{i}", f"src/m{(i // spf) // 100}/f{i // spf}.rs",
             (i % 400) + 1, "rust", "ts")   # 行号从 1 起：schema.sql 的 CHECK 不许 0
            for i in range(n_sym)
        ],
    )
    upsert = ("INSERT INTO edges(src,dst,kind,source,confidence,created_at)"
              " VALUES(?,?,?,?,?,1)"
              " ON CONFLICT(src,dst,kind) DO UPDATE SET"
              " confidence=excluded.confidence, source=excluded.source, expires_at=NULL")
    for i in range(n_sym * eps):
        a, b = i % n_sym, (i * 7919 + 13) % n_sym
        c.execute(upsert, (sym_id(a, spf), sym_id(b, spf), "calls", "ts", 0.85))
    c.execute("COMMIT")
    rows = c.execute("SELECT (SELECT count(*) FROM nodes)+(SELECT count(*) FROM edges)").fetchone()[0]
    return rows, time.time() - t0


def size_mb(path):
    return os.path.getsize(path) / 1048576


def breakdown(c, path, label):
    """量各部分的体积：先用 VACUUM INTO 得到"只含当前对象"的干净副本。"""
    out = []
    for name, drops in [
        ("全部（表+索引+FTS）", []),
        ("去掉 FTS", ["DROP TABLE nodes_fts"]),
        ("再去掉索引", ["DROP INDEX idx_nodes_file_kind", "DROP INDEX idx_nodes_kind",
                        "DROP INDEX idx_edges_dst_kind"]),
        ("只剩 nodes 表", None),
    ]:
        tmp = path + ".probe"
        if os.path.exists(tmp):
            os.remove(tmp)
        # 在副本上做 drop，避免破坏主库
        v = sqlite3.connect(tmp)
        v.close()
        os.remove(tmp)
        c.execute(f"VACUUM INTO '{tmp}'")
        v = sqlite3.connect(tmp)
        if drops is None:
            for t in ("edges", "nodes_fts"):
                v.execute(f"DROP TABLE IF EXISTS {t}")
        else:
            for d in drops:
                v.execute(d)
        v.execute("VACUUM")
        v.close()
        mb = size_mb(tmp)
        out.append((name, mb))
        os.remove(tmp)
    print(f"  -- {label} 体积拆解（MB）--")
    prev = None
    for name, mb in out:
        delta = "" if prev is None else f"  （上一层减 {prev - mb:.1f}）"
        print(f"     {name:<22} {mb:7.1f}{delta}")
        prev = mb


def bench(c, label, sql, params=(), n=10):
    plan = " / ".join(r[3] for r in c.execute("EXPLAIN QUERY PLAN " + sql, params))[:70]
    t0 = time.time()
    for _ in range(n):
        list(c.execute(sql, params))
    ms = (time.time() - t0) / n * 1000
    print(f"     {label:<26}{ms:9.2f} ms  {plan}")
    return ms


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--files", type=int, default=10000)
    ap.add_argument("--spf", type=int, default=20, help="每个文件的符号数")
    ap.add_argument("--eps", type=int, default=4, help="每个符号的出边数（乘上符号数就是边数）")
    args = ap.parse_args()

    db = os.path.join(tempfile.gettempdir(), "jy_probe.db")
    for suffix in ("", "-wal", "-shm"):
        if os.path.exists(db + suffix):
            os.remove(db + suffix)
    c = connect(db)
    print(f"SQLite {sqlite3.sqlite_version} | 规模 文件={args.files} 符号/文件={args.spf} 边/符号={args.eps}")
    rows, dt = fill(c, args.files, args.spf, args.eps)
    print(f"灌注 {rows:,} 行  {dt:.2f}s  |  主库 {size_mb(db):.1f} MB")

    n_sym = args.files * args.spf
    a = sym_id(5, args.spf)
    print("  -- 查询（实测 + 计划）--")
    bench(c, "点查 by id", "SELECT * FROM nodes WHERE id=?", (a,), 200)
    bench(c, "列某文件的符号", "SELECT * FROM nodes WHERE file_path=? AND kind='symbol'",
          (f"src/m0/f5.rs",), 50)
    bench(c, "1 跳正向", "SELECT dst FROM live_edges WHERE src=? AND kind='calls' LIMIT 50", (a,), 50)
    bench(c, "1 跳反向", "SELECT src FROM live_edges WHERE dst=? AND kind='calls' LIMIT 50", (a,), 50)
    bench(c, "3 跳（带上限）",
          "WITH RECURSIVE r(id,d) AS (SELECT ?,0 UNION ALL SELECT e.dst,r.d+1 FROM r"
          " JOIN live_edges e ON e.src=r.id WHERE e.kind='calls' AND r.d<3 LIMIT 500)"
          " SELECT count(*) FROM r", (a,), 5)
    bench(c, "FTS 搜符号",
          "SELECT n.id FROM nodes_fts f JOIN nodes n ON n.rowid=f.rowid"
          " WHERE nodes_fts MATCH ? LIMIT 5", ("s1234",), 20)

    c.execute("ANALYZE")
    breakdown(c, db, "全量")
    print("  -- 不变量 --")
    print("     integrity_check:", c.execute("PRAGMA integrity_check").fetchone()[0])
    print("     foreign_key_check:", len(list(c.execute("PRAGMA foreign_key_check"))), "条问题")
    print("     FTS 与 nodes 行数差:", c.execute(
        "SELECT (SELECT count(*) FROM nodes_fts)-(SELECT count(*) FROM nodes)").fetchone()[0])
    t0 = time.time()
    c.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    print(f"     wal_checkpoint(TRUNCATE): {(time.time()-t0)*1000:.0f} ms")
    t0 = time.time()
    c.execute(f"VACUUM INTO '{db}.bak'")
    print(f"     VACUUM INTO 备份: {time.time()-t0:.2f}s, {size_mb(db+'.bak'):.1f} MB")
    c.close()
    for suffix in ("", "-wal", "-shm", ".bak"):
        if os.path.exists(db + suffix):
            os.remove(db + suffix)
    return 0


if __name__ == "__main__":
    sys.exit(main())
