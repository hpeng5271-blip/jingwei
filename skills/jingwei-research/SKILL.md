---
name: jingwei-research
description: 引入"我之外"的信息并标注可信度。要查外部项目/人物/技术、要读社区讨论（StackOverflow/GitHub/HN/论坛）、或要引用外部结论时使用。
---

# 外网取信：先选通道，再标可信度

## 通道实测（2026-10-22，本机走代理 `127.0.0.1:7897`）

| 通道 | 状态 | 入口 |
|---|---|---|
| **StackOverflow / StackExchange** | ✓ 可用 | `node tools/web.mjs so "<q>"`（官方 API，无 key） |
| **GitHub issues** | ✓ 可用（技术问题第一优先） | `node tools/web.mjs gh "<q>" [--repo owner/name]` |
| **Hacker News** | ✓ 可用 | `node tools/web.mjs hn "<q>"`（Algolia） |
| **Discourse 论坛** | ✓ 可用 | `node tools/web.mjs discourse users.rust-lang.org "<q>"` |
| **Reddit** | ✗ 匿名读不了（登录墙/403） | 要登录态或 API key；**别因此说"没人讨论"** |
| **DuckDuckGo / Bing RSS** | ✗ 挑战页 / 返回垃圾 | 不用 |
| **Discord** | ✗ 要 bot token | 只能人工看邀请链接 |

一条命令多源：`node tools/web.mjs search "webview2 white screen"` → GitHub + SO + HN。

## 用之前先问三句

1. 这个结论**必须**来自外部吗？（仓库里能查的别上网）
2. 来源是**一手**吗？（官方文档/源码 > issue > 论坛帖 > 博客）
3. 它和我这台机器/这个版本**对得上**吗？（版本、平台、时间）

## 引用规矩

- 每个外部结论后跟 `[已核实]`（自己打开过原文）或 `[R]`（只读到摘要/二手）；
- 写清**抓取时间**与 **URL**，社区答案会改；
- **查不到就写"查不了 + 为什么"**（登录墙/无字幕/风控），不许把"没读到"写成"没有"。

## 落盘

```powershell
node tools/web.mjs fetch <url> --chars 4000 --save 主题词
# → docs/research/<日期>-<主题>.md：原文去标签 + 来源 + 抓取方式，不做摘要
```

长文/转写放 `docs/research/transcripts/`，按那里 README 的可靠性规则标注。

## 别做的

- 不用需要登录态的抓取（会让人以为"能自动化"）；
- 不上浏览器/无头渲染（中低配机器，一个 curl 够）；
- 不把社区帖子当判据——判据仍由人定。
