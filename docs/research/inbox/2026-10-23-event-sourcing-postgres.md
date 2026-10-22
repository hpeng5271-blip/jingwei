# Event Sourcing 用 PostgreSQL 做事件存储（GitHub eugene-khyst）

- 抓取时间：2026-10-23T06:52+08:00
- 来源：https://github.com/eugene-khyst/postgresql-event-sourcing（raw README）
- 通道：`node tools/web.mjs fetch https://raw.githubusercontent.com/eugene-khyst/postgresql-event-sourcing/master/README.md`
- 可信度：原话（README 正文）

## 原文摘录

> We can store all changes to the domain object state as a sequence of events in an append-only event stream. Thus, event streams will contain an entire history of changes. … We can use event streams as a primary source of truth in a system. To get the current state of an object, we have to replay all events in the order of occurrence. This pattern is called event sourcing.

> 目录还含：Snapshotting / CQRS / Synchronously updating projections / Transactional outbox / Optimistic concurrency control …

## 它回答了哪个问题

"事件流 + 投影"的成熟做法：append-only 事件流是唯一真源，当前状态（投影）靠按序重放事件得到，投影可重建；快照/outbox 用于加速与派发。
