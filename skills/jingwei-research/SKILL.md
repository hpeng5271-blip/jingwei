---
name: jingwei-research
description: 引入"我之外"的信息并标注可信度。要查外部项目/人物/技术、要读社区（SO/GitHub/HN/Lobsters/dev.to/V2EX/Reddit 档案/论坛）、或要引用外部结论时使用。
---

# 外网取信：先选通道，再标可信度

## 通道实测（2026-10-22，走代理 `127.0.0.1:7897`，**都不需要账号**）

| 通道 | 状态 | 命令 |
|---|---|---|
| StackOverflow / SE | ✓ | `so "<q>"` · **`soq <id>`（提问+答案正文，走 API 绕开 Cloudflare）** |
| GitHub issues | ✓（技术问题第一优先） | `gh "<q>" [--repo o/n]` |
| Hacker News | ✓ | `hn "<q>"` |
| Discourse 论坛 | ✓ | `discourse users.rust-lang.org "<q>"` |
| **Reddit（PullPush 档案）** | ✓ | `pullpush "<q>"`（有标题/分数/子版；**帖子链接打不开**） |
| Lobsters | ✓ | `lobsters [tag]` |
| dev.to | ✓ | `devto <tag>` |
| V2EX（中文） | ✓ | `v2ex hot` |
| Reddit 直连 / DDG / Bing RSS | ✗ | 403 登录墙 / 挑战页 / 垃圾结果 |
| Discord | ✗ | 要 bot token；self-bot 会封号 |

多源一条命令：`node tools/web.mjs search "<q>"`（GitHub + SO + HN + PullPush）。
落盘：`node tools/web.mjs fetch <url> --save 主题` → `docs/research/<日期>-<主题>.md`。

## 用之前先问三句

1. 必须来自外部吗？（仓库里能查的别上网）
2. 是一手吗？（官方文档/源码 > issue > 论坛帖 > 博客）
3. 对得上我这台机器/这个版本吗？（版本、平台、时间）

## 引用规矩

- 每条结论后跟 `[已核实]`（自己读过原文）或 `[R]`（只读到摘要/二手）；
- 写清**抓取时间**与 **URL**，社区答案会改；
- **查不到就写"查不了 + 为什么"**（登录墙/风控/无字幕），不许把"没读到"写成"没有"。

## 抓取频率（硬约束）

间隔 ≥3 秒；每小时 ≤60 次；只读列表/正文；**不点赞、不发帖、不加入社区**。
PullPush/redlib 这类公共实例经常 429——**等一会儿再说，别狂试**。

## 别做的

- 不用别人的账号密码自动登录（违反条款 + 封号风险 + 凭据泄露）；
- 不上浏览器自动化去点按钮（中低配机器；而且真人操作会被判异常）；
- 不把社区帖子当判据——判据仍由人定。
