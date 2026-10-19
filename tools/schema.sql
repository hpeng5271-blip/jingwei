-- 经纬 · 图谱库结构（**唯一真源**）
--
-- 谁在读它：
--   tools/db_probe.py       建库、灌数据、量体积、看查询计划（数字必须能复跑）
--   crates/engine           include_str! 进来做迁移与建库——同一份 DDL，不许各写一份
--   《数据层设计》§3         引用它，**不再贴副本**
--
-- 为什么把 DDL 从文档里搬出来：文档里的代码块没法被执行，也没法被校验。
-- 实测改了三处设计之后，文档里那份 DDL 没跟着改，于是同一份表结构有了两个互相矛盾的版本
-- （Q1 记过这笔账）。放进文件之后，"谁是准的"这个问题就不存在了。
--
-- 改这里的规矩：
--   1. **只许向后兼容**：加列带默认值、加表加索引一律 IF NOT EXISTS；改类型要新建表迁数据
--   2. 改完必须跑 `python tools/db_probe.py`，确认压测数字没坏
--   3. 时间一律 INTEGER 毫秒（排序快、无时区歧义）
--   4. kind / source 的取值用 CHECK 写死：写错一个立刻失败，而不是留下一条"幽灵关系"

PRAGMA journal_mode = WAL;
-- NORMAL 而不是 FULL：WAL 下 NORMAL 保证库不会损坏，最多丢"掉电前还没 checkpoint 的那几笔"。
-- 图谱是可重算的派生数据，丢了下一轮补回来；真正不可丢的是笔记正文，那条路单独走 FULL。
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA wal_autocheckpoint = 400;
PRAGMA cache_size = -2000;
PRAGMA temp_store = MEMORY;

-- ── 元信息：schema_version / last_index_at ─────────────────────────
CREATE TABLE IF NOT EXISTS meta (
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);

-- ── 节点 ───────────────────────────────────────────────────────────
-- id 规则见《数据层设计》§2：<kind>:<规范化路径或符号名>。规范化（分隔符、大小写）
-- 在 Rust 侧做，库里只存结果——这样"同一个东西永远同一个 id"是类型保证的。
CREATE TABLE IF NOT EXISTS nodes (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL CHECK (kind IN (
                'project','file','module','symbol','dependency','toolchain',
                'diagnostic','rule','contract','note','commit','snapshot','run')),
  name        TEXT NOT NULL,
  file_path   TEXT,                 -- 相对路径，统一用 / 分隔
  line        INTEGER CHECK (line IS NULL OR line > 0),
  language    TEXT,
  data        TEXT,                 -- 只放该类实体独有的字段（JSON）；通用字段一律走真列
  source      TEXT NOT NULL CHECK (source IN ('ts','lsp','meta','user','self')),
  confidence  REAL NOT NULL DEFAULT 1.0 CHECK (confidence >= 0.0 AND confidence <= 1.0),
  checksum    TEXT,                 -- 节点级指纹，语义见《数据层设计》§8
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

-- ── 关系 ───────────────────────────────────────────────────────────
-- expires_at IS NULL = 有效；非空 = 已失效但**保留历史**（标记而不删）。
-- WITHOUT ROWID + 复合主键：点查与浅层遍历最快。
CREATE TABLE IF NOT EXISTS edges (
  src        TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  dst        TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  -- 取值顺序与 `EdgeKind::ALL` **一字不差**（含顺序）——engine 的 store 测试守这条。
  -- 顺序不是随便排的：先是结构类（包含/定义/实现），再是引用类（调用/引用/依赖），
  -- 最后是记录类（诊断/因果/时间）。加值时要照着这个语义顺序插。
  kind       TEXT NOT NULL CHECK (kind IN (
               'contains','defines','implements','calls','references','depends_on',
               'needs','diagnosed_at','reported_by','causes','affects','noted_at',
               'changed_at','snapshotted_at','produced','follows','registers')),
  source     TEXT CHECK (source IS NULL OR source IN ('ts','lsp','meta','user','self')),
  confidence REAL NOT NULL DEFAULT 1.0 CHECK (confidence >= 0.0 AND confidence <= 1.0),
  created_at INTEGER NOT NULL,
  expires_at INTEGER,
  PRIMARY KEY (src, dst, kind)
) WITHOUT ROWID;

-- ── 三张支持表 ─────────────────────────────────────────────
-- **解析结果指纹**（2026-10-19）：内容变了、但解析出来的东西没变时，
-- 就不用重写节点/关系（ commentaries 改动不该让图重排）。
-- 单独一张表而不是给 files 加列：老库直接 CREATE IF NOT EXISTS 就能用，不需要 ALTER 迁移。
CREATE TABLE IF NOT EXISTS file_results (
  path       TEXT PRIMARY KEY,
  result_sha TEXT NOT NULL,     -- 解析结果（符号+调用+引用+导入，排序后）的哈希
  at         INTEGER NOT NULL
);
-- **调用点**（2026-10-19）：边上没有行号，而编辑器要跳到"调用发生的那一行"。
-- 单独一张小表，老库 CREATE IF NOT EXISTS 即可用，不需要 ALTER 迁移。
CREATE TABLE IF NOT EXISTS edge_sites (
  src  TEXT NOT NULL,
  dst  TEXT NOT NULL,
  kind TEXT NOT NULL,
  file TEXT NOT NULL,
  line INTEGER NOT NULL,
  byte INTEGER
);

-- files：增量检测与校验的落脚点
CREATE TABLE IF NOT EXISTS files (
  path       TEXT PRIMARY KEY,      -- 相对路径，统一 /
  size       INTEGER NOT NULL,
  mtime_ns   INTEGER NOT NULL,
  content_sha TEXT,
  parsed_at  INTEGER,
  incomplete INTEGER NOT NULL DEFAULT 0,  -- 1 = 这次没解析完（不许当成"解析过"）
  -- 这个文件解析出了几个节点。**抽样校验的判据就是它**：
  -- 重算一遍，内容哈希对得上、节点数也对得上，才叫"图谱没坏"。
  -- 只比文件、不比图谱的话，检查器会漏掉"文件没变但图谱错了"这一类。
  -- NULL = 还没解析过（不是 0——"不知道"和"零个节点"是两回事）。
  nodes_count INTEGER
);

-- index_runs：索引证据。扫了什么、跳过什么、为什么——"这个结论哪来的"要能回答
CREATE TABLE IF NOT EXISTS index_runs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at  INTEGER NOT NULL,
  finished_at INTEGER,
  scanned     INTEGER NOT NULL DEFAULT 0,
  skipped     INTEGER NOT NULL DEFAULT 0,
  incomplete  INTEGER NOT NULL DEFAULT 0,
  skip_json   TEXT
);

-- query_cache：热点查询物化。invalidated_at 非空 = 已作废（保留行，便于观察命中率）
CREATE TABLE IF NOT EXISTS query_cache (
  k              TEXT PRIMARY KEY,
  v              TEXT NOT NULL,
  created_at     INTEGER NOT NULL,
  invalidated_at INTEGER
);

-- ── 索引：先列出真会跑的查询，再给它配索引（《数据层设计》§4）──────
-- ① 按文件找该文件的全部节点/关系（打开文件、变更重解析）
CREATE INDEX IF NOT EXISTS idx_nodes_file_kind ON nodes(file_path, kind);
-- ② 按类型数节点（"有多少个 symbol"、规则匹配）
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
-- ③ 反向遍历：谁指向我。**部分索引**——失效边不参与查询计划
CREATE INDEX IF NOT EXISTS idx_edges_dst_kind ON edges(dst, kind) WHERE expires_at IS NULL;
-- 说明：`edges` 的正向查询（src=?）由主键 (src,dst,kind) 覆盖，不再单独建 (src,kind)。
-- 实测把这些列进去之后：24.27ms → 0.03ms。

-- ── 全文搜索：FTS5 外部内容表 + 三个触发器 ─────────────────────────
-- 外部内容（content='nodes'）不复制正文，只在 nodes 变动时同步索引。
-- 三个触发器就是 FTS5 官方那套写法：插入/删除/更新各一个。
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
  name, kind, file_path,
  content='nodes', content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS nodes_fts_ai AFTER INSERT ON nodes BEGIN
  INSERT INTO nodes_fts(rowid, name, kind, file_path)
  VALUES (new.rowid, new.name, new.kind, new.file_path);
END;

CREATE TRIGGER IF NOT EXISTS nodes_fts_ad AFTER DELETE ON nodes BEGIN
  INSERT INTO nodes_fts(nodes_fts, rowid, name, kind, file_path)
  VALUES ('delete', old.rowid, old.name, old.kind, old.file_path);
END;

CREATE TRIGGER IF NOT EXISTS nodes_fts_au AFTER UPDATE ON nodes BEGIN
  INSERT INTO nodes_fts(nodes_fts, rowid, name, kind, file_path)
  VALUES ('delete', old.rowid, old.name, old.kind, old.file_path);
  INSERT INTO nodes_fts(rowid, name, kind, file_path)
  VALUES (new.rowid, new.name, new.kind, new.file_path);
END;

-- 生效边视图：查询层只许查它（《数据层设计》§6）。
-- 让"忘了过滤失效边"这件事在语法上做不到，而不是靠每处记得写 WHERE。
CREATE VIEW IF NOT EXISTS live_edges AS
  SELECT src, dst, kind, source, confidence, created_at
  FROM edges WHERE expires_at IS NULL;

-- ── 版本戳 ─────────────────────────────────────────────────────────
-- schema_version 是唯一的版本真相源（《数据层设计》§9）。迁移脚本读它决定跑哪几步。
INSERT OR IGNORE INTO meta(k, v) VALUES ('schema_version', '3');
