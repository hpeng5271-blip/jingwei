---
name: skills
description: 经纬的 5 条工作 skill（门禁/图谱/三态/研究/文档）。安装到 DSH 用 tools/skills-install.mjs。
---

# skills/

每条一个目录，`SKILL.md` 带 frontmatter（`name` + `description`，description 写清触发时机）。

| skill | 何时用 |
|---|---|
| `jingwei-gate` | 跑门禁、门禁红了、判断能否提交、写数字 |
| `jingwei-graph` | 了解仓库结构/找入口/查符号/交接 |
| `jingwei-triage` | 报结论、遇不确定、连续失败、判断"能不能说完成" |
| `jingwei-research` | 调研外部项目/人物/技术、视频转写、引用外部结论 |
| `jingwei-docs` | 改文档数字、写报告、打包、MANIFEST |

## 规矩

- **只写"怎么做"**，不抄 `AGENTS.md`（入口）与 `SOP.md`（流程）的正文；冲突时以那两份为准。
- 每条 ≤60 行；description ≤120 字，必须含触发词。
- 改完跑 `node tools/skills-check.mjs`（门禁里也跑）。

## 安装

```powershell
node tools/skills-install.mjs          # 复制到 %USERPROFILE%\.dsh\skills\
```

装完**重开会话**才会出现在技能目录（DSH 在会话启动时加载）。
