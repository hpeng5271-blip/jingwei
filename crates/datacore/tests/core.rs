use datacore::compose::{compose, is_conflict, Outcome};
use datacore::ids::{evidence_id, judgment_id, stable_hash};
use datacore::model::{Judgment, Scope, Verdict};
use datacore::store::Store;

fn j(subject: &str, v: Verdict, cr: i64, ct: i64, at: i64) -> Judgment {
    Judgment {
        id: judgment_id("repo", "test", subject, at),
        subject: subject.to_string(),
        verdict: v,
        kind: "test".into(),
        scope: Scope::default(),
        role: None,
        expires_at: None,
        at,
        coverage_read: cr,
        coverage_total: ct,
    }
}

#[test]
fn hash_is_stable_and_distinct() {
    assert_eq!(stable_hash("abc"), stable_hash("abc"));
    assert_ne!(stable_hash("abc"), stable_hash("abd"));
    assert_eq!(judgment_id("r", "k", "s", 1), judgment_id("r", "k", "s", 1));
    assert_ne!(judgment_id("r", "k", "s", 1), judgment_id("r", "k", "s2", 1));
    assert_ne!(judgment_id("r", "k", "s", 1), judgment_id("r", "k", "s", 2));
    assert!(evidence_id("j", "p", "supports") != evidence_id("j", "p", "contradicts"));
}

#[test]
fn compose_no_data() {
    assert_eq!(compose(&[], 0, 0), Outcome::Uncertain("no_data"));
}

#[test]
fn compose_held_when_covered() {
    let js = vec![j("s", Verdict::Held, 1, 1, 10)];
    assert_eq!(compose(&js, 1, 0), Outcome::Held);
}

#[test]
fn compose_conflict_gate() {
    let js = vec![j("s", Verdict::Held, 1, 1, 10)];
    assert_eq!(compose(&js, 1, 1), Outcome::Uncertain("conflict"));
}

#[test]
fn compose_coverage_gate() {
    let js = vec![j("s", Verdict::Held, 1, 10, 10)];
    assert_eq!(compose(&js, 1, 0), Outcome::Uncertain("coverage_short"));
}

#[test]
fn conflict_requires_opposite_and_full_coverage() {
    let a = j("s", Verdict::Held, 1, 1, 10);
    let b = j("s", Verdict::Refuted, 1, 1, 11);
    assert!(is_conflict(&a, &b));

    let partial = j("s", Verdict::Refuted, 1, 10, 11);
    assert!(!is_conflict(&a, &partial));
}

#[test]
fn ddl_blocks_refuted_without_full_coverage() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("ddl")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let store = Store::open(&dir).unwrap();
    let bad = j("s", Verdict::Refuted, 1, 10, 10);
    // 应当被 CHECK 拒绝
    assert!(store.put_judgment(&bad).is_err());

    let good = j("s", Verdict::Refuted, 10, 10, 10);
    assert!(store.put_judgment(&good).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn alternative_requires_existing_judgments() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("alt")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let a = j("a", Verdict::Held, 1, 1, 10);
    let b = j("b", Verdict::Held, 1, 1, 11);
    store.put_judgment(&a).unwrap();
    store.put_judgment(&b).unwrap();

    assert!(store.put_alternative(&a.id, &a.id).is_err());
    assert!(store.put_alternative("nope", &b.id).is_err());
    assert!(store.put_alternative(&a.id, &b.id).is_ok());
    assert_eq!(store.list_alternatives(&b.id).unwrap(), vec![a.id.clone()]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scope_roundtrip() {
    use std::collections::BTreeMap;
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("scope")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let mut row = j("s", Verdict::Held, 1, 1, 10);
    let mut env = BTreeMap::new();
    env.insert("repo".to_string(), "demo".to_string());
    row.scope = Scope {
        objects: vec!["src/a.rs".into()],
        time_window: Some((1, 2)),
        env,
        granularity: "symbol".into(),
    };
    store.put_judgment(&row).unwrap();

    let back = store.find_judgment("s").unwrap().unwrap();
    assert_eq!(back.scope.objects, vec!["src/a.rs".to_string()]);
    assert_eq!(back.scope.granularity, "symbol");
    assert_eq!(back.scope.time_window, Some((1, 2)));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn as_of_uses_t_for_expiry() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("asof")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let mut row = j("s", Verdict::Held, 1, 1, 10);
    row.expires_at = Some(200);
    store.put_judgment(&row).unwrap();

    // T=150：当时还没过期 → held
    let a = datacore::query::ask(&store, "s", Some(150)).unwrap();
    assert_eq!(a.outcome, Outcome::Held);
    // 边界：expires_at 含端点，T=199 仍 held，T=200 就过期
    let edge_ok = datacore::query::ask(&store, "s", Some(199)).unwrap();
    assert_eq!(edge_ok.outcome, Outcome::Held);
    let edge = datacore::query::ask(&store, "s", Some(200)).unwrap();
    assert_eq!(edge.outcome, Outcome::Uncertain("expired"));
    // T=250：当时已过期 → expired
    let b = datacore::query::ask(&store, "s", Some(250)).unwrap();
    assert_eq!(b.outcome, Outcome::Uncertain("expired"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expire_by_id_targets_that_judgment() {
    use datacore::model::{Evidence, Polarity, Source};
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("expid")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let old = j("s", Verdict::Held, 1, 1, 10);
    let new = j("s", Verdict::Refuted, 1, 1, 20);
    store.put_judgment(&old).unwrap();
    store.put_judgment(&new).unwrap();

    // 冲突来自**证据极性**（不是 verdict 本身）：一边 supports、一边 contradicts
    let src = Source {
        id: "src".into(),
        instrument: "test".into(),
        instrument_version: None,
        env: "t".into(),
        time: 10,
    };
    store.put_source(&src).unwrap();
    for (id, jid, polarity, premise) in [
        ("e1", &old.id, Polarity::Supports, "p1"),
        ("e2", &new.id, Polarity::Contradicts, "p2"),
    ] {
        store
            .put_evidence(&Evidence {
                id: id.into(),
                judgment_id: jid.clone(),
                premise: premise.into(),
                polarity,
                source_id: src.id.clone(),
            })
            .unwrap();
    }

    // 两条都活着 → conflict；只让 old 过期 → 剩下 refuted
    let before = datacore::query::ask(&store, "s", Some(100)).unwrap();
    assert_eq!(before.outcome, Outcome::Uncertain("conflict"));
    store.set_expiry(&old.id, 20).unwrap();
    let after = datacore::query::ask(&store, "s", Some(100)).unwrap();
    assert_eq!(after.outcome, Outcome::Refuted, "过期点错了判断");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_skips_expired() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("scanexp")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let a = j("s", Verdict::Held, 1, 1, 10);
    let b = j("s", Verdict::Refuted, 1, 1, 20);
    store.put_judgment(&a).unwrap();
    store.put_judgment(&b).unwrap();

    assert_eq!(store.scan_conflicts(50).unwrap(), 1);
    store.set_expiry(&a.id, 60).unwrap();
    assert_eq!(store.scan_conflicts(60).unwrap(), 0, "过期判断仍参与冲突扫描");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_clears_conflict() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("resolve")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let a = j("s", Verdict::Held, 1, 1, 10);
    let b = j("s", Verdict::Refuted, 1, 1, 11);
    store.put_judgment(&a).unwrap();
    store.put_judgment(&b).unwrap();
    store.put_conflict(&a.id, &b.id).unwrap();
    assert_eq!(store.list_conflicts().unwrap().len(), 1);

    let out = store
        .resolve_conflict("repo", "s", Verdict::Held, "alice", "复验后确认还在", 100)
        .unwrap();
    assert!(out.is_some(), "有冲突却返回 no_conflict");

    // 冲突清账；旧判断还在（过期不删）；结论是 held
    assert!(store.list_conflicts().unwrap().is_empty());
    assert_eq!(store.judgments_for("s").unwrap().len(), 3, "旧判断被删了");
    assert_eq!(
        datacore::query::ask(&store, "s", Some(200)).unwrap().outcome,
        Outcome::Held
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_detects_unregistered_kind_and_bad_log() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("checkkind")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let mut row = j("s", Verdict::Held, 1, 1, 1000);
    row.kind = "zzz".into();
    store.put_judgment(&row).unwrap();
    assert_eq!(store.unregistered_kinds().unwrap(), 1, "未注册 kind 没被查出来");

    store.note_write(&["a".into()]).unwrap();
    store.note_write(&["b".into()]).unwrap();
    assert!(store.write_log_ok().unwrap(), "正常的写日志被误判");

    // 把日志改成 seq 不递增 → 必须判坏
    store
        .set_meta("write_log", r#"[{"seq":2,"subjects":[]},{"seq":1,"subjects":[]}]"#)
        .unwrap();
    assert!(!store.write_log_ok().unwrap(), "seq 倒序没被查出来");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn begin_commit_actually_batches() {
    // 事务必须真的生效：begin 之后，另一个连接看不到未提交的行
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("tx")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    store.begin().unwrap();
    let row = j("s", Verdict::Held, 1, 1, 1000);
    store.put_judgment(&row).unwrap();

    let db = dir.join(".jingwei").join("jingwei.db");
    let probe = rusqlite::Connection::open(&db).unwrap();
    let n: i64 = probe
        .query_row("SELECT COUNT(*) FROM judgment", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "未提交的行被另一个连接看到了");

    store.commit().unwrap();
    let n: i64 = probe
        .query_row("SELECT COUNT(*) FROM judgment", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_log_reports_gap_when_truncated() {
    // 事件日志有界（50）：老的被挤掉时 watch 必须报 gap，客户端全量重取
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("watchgap")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    for i in 0..60 {
        store.note_write(&[format!("s{i}")]).unwrap();
    }

    let (events, gap) = store.watch_events(0).unwrap();
    assert!(gap, "日志被截断却没报 gap");
    assert_eq!(events.len(), 50);
    assert_eq!(events.first().unwrap().0, 11);

    let (events, gap) = store.watch_events(10).unwrap();
    assert!(!gap, "从 10 开始不该断档：{:?}", events.first());
    assert_eq!(events.len(), 50);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_fold_keeps_ends_and_adds_gist() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("applyfold")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();
    let long_ago = now - 100 * 86_400;

    for i in 0..12 {
        let mut row = j("s", Verdict::Held, 1, 1, 1000 + i);
        row.expires_at = Some(long_ago);
        store.put_judgment(&row).unwrap();
    }

    let plans = store.fold_plans(now, 8, 90 * 86_400).unwrap();
    assert_eq!(plans.len(), 1);
    let (subject, verdict, entity, _n, t0, t1) = plans[0].clone();
    let deleted = store
        .apply_fold(&subject, &verdict, entity.as_deref(), t0, t1)
        .unwrap();
    assert_eq!(deleted, 10, "中间观测没删对");

    let rows = store.judgments_for("s").unwrap();
    assert_eq!(rows.len(), 3, "应剩 首 + 尾 + gist：{rows:?}");
    assert!(
        rows.iter().any(|r| r.kind == "gist"),
        "没有 gist 判断：{rows:?}"
    );
    // as_of 回看仍是 held（gist 未过期，段内端点已过期）
    let ans = datacore::query::ask(&store, "s", Some(2000)).unwrap();
    assert_eq!(ans.outcome, Outcome::Held);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fold_plans_splits_runs_by_entity() {
    // 同一 subject 被不同实体复用时（删除后重建、路径复用），折叠必须分段，不许并成一段
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("foldentity")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();
    let long_ago = now - 100 * 86_400;

    for (i, ent) in (0..10).map(|i| (i, "e1")).chain((10..20).map(|i| (i, "e2"))) {
        let mut row = j("s", Verdict::Held, 1, 1, 1000 + i);
        row.expires_at = Some(long_ago);
        row.scope.env.insert("entity".into(), ent.to_string());
        store.put_judgment(&row).unwrap();
    }

    let plans = store.fold_plans(now, 8, 90 * 86_400).unwrap();
    assert_eq!(plans.len(), 2, "不同 entity 被并成一段：{plans:?}");
    assert_eq!(plans[0].2.as_deref(), Some("e1"));
    assert_eq!(plans[1].2.as_deref(), Some("e2"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fold_plans_finds_only_old_same_verdict_runs() {
    // 记忆折叠：只有「同向 + 段首已过期 ≥90 天 + 条数 ≥8」的连续段才可折
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("fold")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();
    let long_ago = now - 100 * 86_400;

    for i in 0..10 {
        let mut row = j("s", Verdict::Held, 1, 1, 1000 + i);
        row.expires_at = Some(long_ago);
        store.put_judgment(&row).unwrap();
    }
    // 另一条未过期的，不该被折
    let fresh = j("w", Verdict::Held, 1, 1, now - 10);
    store.put_judgment(&fresh).unwrap();

    let plans = store.fold_plans(now, 8, 90 * 86_400).unwrap();
    assert_eq!(plans.len(), 1, "plans={plans:?}");
    let (subject, verdict, _entity, n, t0, t1) = &plans[0];
    assert_eq!(subject, "s");
    assert_eq!(verdict, "held");
    assert_eq!(*n, 10);
    assert_eq!((*t0, *t1), (1000, 1009));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expire_older_than_skips_newer_at() {
    // at=100/200，给 150 → 只过期 100；200（比 at 新）不许误杀（B4.2 的必要条件）
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("expguard")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let old = j("s", Verdict::Held, 1, 1, 100);
    let new = j("s", Verdict::Held, 1, 1, 200);
    store.put_judgment(&old).unwrap();
    store.put_judgment(&new).unwrap();

    let n = store.expire_older_than("s", None, 150).unwrap();
    assert_eq!(n, 1, "过期条数不对");
    let all = store.judgments_for("s").unwrap();
    let o = all.iter().find(|x| x.id == old.id).unwrap();
    let nw = all.iter().find(|x| x.id == new.id).unwrap();
    assert_eq!(o.expires_at, Some(150));
    assert_eq!(nw.expires_at, None, "比 at 新的判断被误杀");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn future_at_does_not_affect_other_subject() {
    // 逻辑时钟是 per-subject：A 有一条未来 at 的判断，不能把 B 判过期。
    // 这条钉住 threshold = max(now, 本 subject 最新 at) 的作用域（B3.2）。
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("futurescope")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();

    let far = j("a", Verdict::Held, 1, 1, now + 100);
    store.put_judgment(&far).unwrap();

    let mut b = j("b", Verdict::Held, 1, 1, now);
    b.expires_at = Some(now + 50);
    store.put_judgment(&b).unwrap();

    let ans = datacore::query::ask(&store, "b", None).unwrap();
    assert_eq!(
        ans.outcome,
        Outcome::Held,
        "A 的未来 at 影响了 B 的过期基准（逻辑时钟串味）"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_writes_new_judgment() {
    // 同秒撞 id 的回归：resolve 用 now 当 at 会与现有判断同 id 被 DO NOTHING 吞掉。
    // 这里 now=11，夹具里已有一条 at=11 的 refuted → 新判断必须是 at=12 的第 3 条。
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("resolve-new")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let a = j("s", Verdict::Held, 1, 1, 10);
    let b = j("s", Verdict::Refuted, 1, 1, 11);
    store.put_judgment(&a).unwrap();
    store.put_judgment(&b).unwrap();
    store.put_conflict(&a.id, &b.id).unwrap();

    let out = store
        .resolve_conflict("repo", "s", Verdict::Held, "alice", "复验", 11)
        .unwrap();
    assert!(out.is_some());

    let all = store.judgments_for("s").unwrap();
    assert_eq!(all.len(), 3, "resolve 的新判断被静默丢了：{all:?}");
    let latest = store.find_judgment("s").unwrap().unwrap();
    assert_eq!(latest.verdict, Verdict::Held);
    assert_eq!(latest.at, 12, "新判断的 at 没有推进");
    assert_eq!(latest.expires_at, None);
    for old in all.iter().filter(|x| x.id != latest.id) {
        assert_eq!(
            old.expires_at,
            Some(latest.at),
            "旧判断没在“新观测时刻”过期：{old:?}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_without_conflict_is_no_conflict() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("noconf")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let a = j("s", Verdict::Held, 1, 1, 10);
    store.put_judgment(&a).unwrap();
    let out = store
        .resolve_conflict("repo", "s", Verdict::Held, "alice", "x", 100)
        .unwrap();
    assert!(out.is_none(), "没有冲突却写了一条判断");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_flags_orphan_evidence() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("orphan")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join(".jingwei").join("jingwei.db");

    // 先用 Store 建库，再用裸连接（关 FK）造孤儿：
    // 只有绕过 Store 直接改库才可能留下这种行，所以测试也走同一条路。
    drop(Store::open(&dir).unwrap());
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO source(id, instrument, env, time) VALUES ('s1','raw','t',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO judgment(id, subject, verdict, kind, scope, at, coverage_read, coverage_total)
             VALUES ('j1','orphan-a','held','test','{\"objects\":[],\"time_window\":null,\"env\":{},\"granularity\":\"unknown\"}',1,1,1),
                    ('j2','orphan-b','held','test','{\"objects\":[],\"time_window\":null,\"env\":{},\"granularity\":\"unknown\"}',1,1,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO evidence(id, judgment_id, premise, polarity, source_id)
             VALUES ('e1','j1','p','supports','s1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO alternative_of(judgment_id, parent_id) VALUES ('j1','j2')",
            [],
        )
        .unwrap();
        // 删掉 j1：FK 关着 → 证据和候选关系都成了孤儿
        conn.execute("DELETE FROM judgment WHERE id = 'j1'", [])
            .unwrap();
    }

    let store = Store::open(&dir).unwrap();
    assert_eq!(store.orphan_evidence().unwrap(), 1, "check 没查出孤儿依据");
    assert_eq!(store.orphan_alternative().unwrap(), 1, "check 没查出孤儿候选");
    // 这两条没有坏，别误报
    assert_eq!(store.false_coverage().unwrap(), 0);
    assert_eq!(store.refuted_without_full_coverage().unwrap(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reimport_does_not_wipe_verdict_or_evidence() {
    use datacore::model::{Evidence, Polarity, Source};
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("reimport")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    let row = j("s", Verdict::Held, 1, 1, 10);
    store.put_judgment(&row).unwrap();

    // verify 之后：结论翻 refuted，并留下一条 contradicts 证据
    store.update_verdict(&row.id, Verdict::Refuted).unwrap();
    let src = Source {
        id: "src".into(),
        instrument: "test".into(),
        instrument_version: None,
        env: "t".into(),
        time: 11,
    };
    store.put_source(&src).unwrap();
    let ev = Evidence {
        id: "ev".into(),
        judgment_id: row.id.clone(),
        premise: "recheck:missing".into(),
        polarity: Polarity::Contradicts,
        source_id: src.id.clone(),
    };
    store.put_evidence(&ev).unwrap();

    // 重跑 ingest：同一条判断（held）再写一遍 —— 不许冲掉上面的结论与证据
    store.put_judgment(&row).unwrap();
    let back = store.find_judgment("s").unwrap().unwrap();
    assert_eq!(back.verdict, Verdict::Refuted, "put_judgment 把结论冲掉了");
    assert_eq!(
        store.count_evidence(&row.id, "contradicts").unwrap(),
        1,
        "put_judgment 把证据冲掉了"
    );

    // 只有显式 force 才覆盖，且仍不删证据
    let mut forced = row.clone();
    forced.verdict = Verdict::Held;
    store.put_judgment_force(&forced).unwrap();
    assert_eq!(store.find_judgment("s").unwrap().unwrap().verdict, Verdict::Held);
    assert_eq!(store.count_evidence(&row.id, "contradicts").unwrap(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn record_verdict_maps_status() {
    use datacore::model::record_verdict;
    assert_eq!(
        record_verdict("已落地（`crates/cli/src/main.rs`）"),
        Verdict::Held
    );
    // 同时有「已定」和「待实施」：按 A2 的优先级，待实施赢 → uncertain
    assert_eq!(
        record_verdict("设计已定；3 处待实施、1 处待人口径"),
        Verdict::Uncertain
    );
    assert_eq!(
        record_verdict("部分完成（基线脚本已落并跑出可复现数字）"),
        Verdict::Uncertain
    );
    assert_eq!(record_verdict("已作废"), Verdict::Refuted);
    // 没有状态行 = 没写 = 不知道
    assert_eq!(record_verdict(""), Verdict::Uncertain);
}

#[test]
fn collection_kinds_registered() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("kinds")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();

    assert_eq!(store.kind_layer("change").unwrap().as_deref(), Some("code"));
    assert_eq!(store.kind_layer("doc").unwrap().as_deref(), Some("code"));
    // A2：decision 挂在 rationale 下，hypothesis 挂在 decision 下，都是 human 层
    assert_eq!(store.kind_layer("decision").unwrap().as_deref(), Some("human"));
    assert_eq!(store.kind_layer("hypothesis").unwrap().as_deref(), Some("human"));
    // A1 已裁掉 issue/pr：不该再登记
    assert_eq!(store.kind_layer("issue").unwrap(), None);
    assert_eq!(store.kind_layer("pr").unwrap(), None);
    assert_eq!(store.kind_layer("no-such-kind").unwrap(), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expired_judgment_is_filtered() {
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("expiry")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let store = Store::open(&dir).unwrap();
    let mut row = j("s", Verdict::Held, 1, 1, 1);
    row.expires_at = Some(2);
    store.put_judgment(&row).unwrap();

    let ans = datacore::query::ask(&store, "s", None).unwrap();
    assert_eq!(ans.outcome, Outcome::Uncertain("expired"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_fold_across_verdict_change() {
    // 10 held 之后 10 refuted：只能折出两段，而且每段只有一个 verdict，区间不相交
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("fold-verdict")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();
    let old = now - 200 * 86_400;

    for i in 0..10 {
        let mut row = j("s", Verdict::Held, 1, 1, old + i * 3600);
        row.expires_at = Some(old + 86_400);
        store.put_judgment(&row).unwrap();
    }
    for i in 0..10 {
        let mut row = j("s", Verdict::Refuted, 1, 1, old + 10 * 3600 + i * 3600);
        row.expires_at = Some(old + 86_400);
        store.put_judgment(&row).unwrap();
    }

    let plans = store.fold_plans(now, 8, 90 * 86_400).unwrap();
    assert_eq!(plans.len(), 2, "转折点被合成一段了：{plans:?}");
    let verdicts: std::collections::HashSet<&str> =
        plans.iter().map(|p| p.1.as_str()).collect();
    assert_eq!(verdicts.len(), 2, "两段必须是不同 verdict：{plans:?}");
    // 区间不相交
    let mut ranges: Vec<(i64, i64)> = plans.iter().map(|p| (p.4, p.5)).collect();
    ranges.sort();
    assert!(ranges[0].1 < ranges[1].0, "两段区间重叠：{ranges:?}");

    for (subject, verdict, entity, _n, t0, t1) in &plans {
        store
            .apply_fold(subject, verdict, entity.as_deref(), *t0, *t1)
            .unwrap();
    }
    // 折完：refuted 段起点之后不许再出现 held 的“中段”行
    let rows = store.judgments_for("s").unwrap();
    let boundary = ranges[1].0;
    for r in &rows {
        assert!(
            !(r.verdict == Verdict::Held && r.at >= boundary),
            "转折点之后还有 held 行：{:?}",
            r.at
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fold_splits_by_entity() {
    // 同一 subject、内容换过（entity A → B）、verdict 相同：这是两条谱系，不能并成一段
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("fold-entity2")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();
    let old = now - 300 * 86_400;

    let mk = |entity: &str, at: i64| {
        let mut r = j("s", Verdict::Held, 1, 1, at);
        r.expires_at = Some(at + 3_600);
        r.scope.env.insert("entity".into(), entity.to_string());
        r
    };
    for i in 0..8 {
        store.put_judgment(&mk("a:1", old + i * 3600)).unwrap();
    }
    for i in 0..8 {
        store.put_judgment(&mk("b:2", old + 100 * 3600 + i * 3600)).unwrap();
    }

    let plans = store.fold_plans(now, 8, 90 * 86_400).unwrap();
    assert_eq!(plans.len(), 2, "两个 entity 被并成一段了：{plans:?}");
    let ents: std::collections::HashSet<String> =
        plans.iter().filter_map(|p| p.2.clone()).collect();
    assert_eq!(ents.len(), 2, "两段必须属于不同 entity：{plans:?}");
    let mut ranges: Vec<(i64, i64)> = plans.iter().map(|p| (p.4, p.5)).collect();
    ranges.sort();
    assert!(ranges[0].1 < ranges[1].0, "两段区间重叠：{ranges:?}");

    // 每段自身仍能折（留首尾、删中间、加 gist）
    for (subject, verdict, entity, _n, t0, t1) in &plans {
        let deleted = store
            .apply_fold(subject, verdict, entity.as_deref(), *t0, *t1)
            .unwrap();
        assert_eq!(deleted, 6, "8 条应删 6 条（留首尾）：{deleted}");
    }
    assert_eq!(
        store
            .count("judgment")
            .unwrap(),
        2 * 2 + 2, // 每段留 2 条 + 每段 1 条 gist
        "折后行数不对"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fold_keeps_as_of_answers() {
    // 段中间被删后，as-of 落进空洞也必须答 held（段首存活到段尾）
    let dir = std::env::temp_dir().join(format!("jw-test-{}", stable_hash("fold-asof")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let now = datacore::store::now_secs();
    let base = now - 200 * 86_400;

    for i in 0..10 {
        let at = base + i * 86_400;
        let mut r = j("symbol:a.rs#f", Verdict::Held, 1, 1, at);
        r.expires_at = Some(at + 86_400);
        store.put_judgment(&r).unwrap();
    }
    let answer_at = |t: i64| -> Option<String> {
        let rows = store.judgments_for("symbol:a.rs#f").unwrap();
        let mut best: Option<&datacore::model::Judgment> = None;
        for r in &rows {
            if r.at <= t && best.map(|b| r.at > b.at).unwrap_or(true) {
                best = Some(r);
            }
        }
        best.filter(|r| r.expires_at.map(|e| e >= t).unwrap_or(true))
            .map(|r| r.verdict.as_str().to_string())
    };

    let mid = base + 5 * 86_400;
    assert_eq!(answer_at(mid).as_deref(), Some("held"), "折前就该答 held");

    let plans = store.fold_plans(now, 8, 90 * 86_400).unwrap();
    assert_eq!(plans.len(), 1, "{plans:?}");
    for (subject, verdict, entity, _n, t0, t1) in &plans {
        store
            .apply_fold(subject, verdict, entity.as_deref(), *t0, *t1)
            .unwrap();
    }
    assert_eq!(
        answer_at(mid).as_deref(),
        Some("held"),
        "折完之后 as-of 落进空洞了"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

