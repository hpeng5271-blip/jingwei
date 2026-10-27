PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- 开放 kind 登记：地图长什么样由它记录。
CREATE TABLE IF NOT EXISTS kind_registry (
  kind        TEXT PRIMARY KEY,
  layer       TEXT NOT NULL CHECK (layer IN ('code','human','tx','meta')),
  parent      TEXT,
  description TEXT,
  since       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshot (
  id           TEXT PRIMARY KEY,
  commit_hash  TEXT NOT NULL,
  parent_hash  TEXT,
  created_at   INTEGER NOT NULL
);

-- 关系（不是判断）：mod 包含 / use 引用 / impl 关系。最小边层，只由 parse 落。
CREATE TABLE IF NOT EXISTS edge (
  src       TEXT NOT NULL,
  dst       TEXT NOT NULL,
  kind      TEXT NOT NULL,
  at        INTEGER NOT NULL,
  source_id TEXT,
  leaf      TEXT,
  line      INTEGER,
  PRIMARY KEY (src, dst, kind, leaf, line)
);

CREATE TABLE IF NOT EXISTS change (
  id          TEXT PRIMARY KEY,
  commit_hash TEXT NOT NULL,
  path        TEXT NOT NULL,
  status      TEXT NOT NULL,
  author      TEXT NOT NULL,
  at          INTEGER NOT NULL,
  subject     TEXT NOT NULL,
  old_path    TEXT,
  blob        TEXT
);
CREATE INDEX IF NOT EXISTS ix_change_path ON change(path);
CREATE INDEX IF NOT EXISTS ix_change_commit ON change(commit_hash);
CREATE INDEX IF NOT EXISTS ix_change_path_at ON change(path, at);
CREATE INDEX IF NOT EXISTS ix_change_old_path ON change(old_path);

CREATE TABLE IF NOT EXISTS source (
  id                 TEXT PRIMARY KEY,
  instrument         TEXT NOT NULL,
  instrument_version TEXT,
  env                TEXT NOT NULL,
  time               INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS judgment (
  id             TEXT PRIMARY KEY,
  subject        TEXT NOT NULL,
  verdict        TEXT NOT NULL CHECK (verdict IN ('held','refuted','uncertain')),
  kind           TEXT NOT NULL,
  scope          TEXT NOT NULL,
  role           TEXT,
  expires_at     INTEGER,
  at             INTEGER NOT NULL,
  coverage_read  INTEGER NOT NULL DEFAULT 0,
  coverage_total INTEGER NOT NULL DEFAULT 0,
  -- 红线：说“不是”必须看全了
  CHECK (verdict <> 'refuted' OR coverage_read >= coverage_total),
  -- 过期时间不能早于产生时间
  CHECK (expires_at IS NULL OR expires_at >= at)
);
CREATE INDEX IF NOT EXISTS ix_judgment_subject ON judgment(subject);
CREATE INDEX IF NOT EXISTS ix_judgment_kind ON judgment(kind, expires_at);
CREATE INDEX IF NOT EXISTS ix_judgment_subject_exp ON judgment(subject, expires_at);
CREATE INDEX IF NOT EXISTS ix_judgment_kind_subject ON judgment(kind, expires_at, subject);

CREATE TABLE IF NOT EXISTS evidence (
  id          TEXT PRIMARY KEY,
  judgment_id TEXT NOT NULL REFERENCES judgment(id) ON DELETE CASCADE,
  premise     TEXT NOT NULL,
  polarity    TEXT NOT NULL CHECK (polarity IN ('supports','contradicts')),
  source_id   TEXT NOT NULL REFERENCES source(id)
);
CREATE INDEX IF NOT EXISTS ix_evidence_judgment ON evidence(judgment_id);

-- 冲突是两条判断之间的关系，不是判断本身的状态。
CREATE TABLE IF NOT EXISTS conflict (
  id          TEXT PRIMARY KEY,
  judgment_a  TEXT NOT NULL,
  judgment_b  TEXT NOT NULL,
  detected_at INTEGER NOT NULL,
  resolved    INTEGER NOT NULL DEFAULT 0,
  resolved_at INTEGER,
  CHECK (judgment_a <> judgment_b)
);

CREATE TABLE IF NOT EXISTS alternative_of (
  judgment_id TEXT NOT NULL,
  parent_id   TEXT NOT NULL,
  PRIMARY KEY (judgment_id, parent_id),
  CHECK (judgment_id <> parent_id)
);
