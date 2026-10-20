# 技能（skills）怎么管：源在仓库、装在 DSH、用门禁守

> 目标：**把"外部的好方法"变成"本项目随时能调用的动作"**，而不是又攒一堆读不完的文档。

## 1. 三个位置，各管一件事

| 位置 | 是什么 | 规矩 |
|---|---|---|
| `skills/<name>/SKILL.md` | **源**（跟代码一起 review） | frontmatter：`name` + `description`（写清触发时机）；正文 ≥3 个二级标题；≤60 行 |
| `skills/REGISTRY.json` | **治理信息**（来源/改编点/验收命令/责任人/复核日） | 每条 skill 必须登记；180 天不复核 → 体检警告 |
| `%USERPROFILE%\.dsh\skills\` | **装载点**（DSH 只从这里读） | 由 `node tools/skills-install.mjs` 复制；重开会话生效 |

## 2. 生命周期（四步，每步有判据）

1. **导入**：外部来源（论文/项目文档/别人家的 skill）先记 `source`（kind: original / adapted + url）。
2. **适配**：只留"本项目能执行的动作"——命令、路径、判据；把外部术语换成我们的（三态、门禁、债表）。
   判据：**每条 skill 至少绑定一条可执行命令**（`verify` 字段）。
3. **验证**：`node tools/skills-check.mjs` + `node tools/skills-doctor.mjs` 都过；能用真命令跑一次（不要求进 CI）。
4. **登记**：写 `REGISTRY.json`，填 `owner` / `added` / `reviewed`。

## 3. 什么时候退役

- 连续两次复核（180 天）没人调用 → `status: deprecated` + `reason`；
- 与 `AGENTS.md` / `SOP.md` 冲突 → 以那两份为准，skill 删掉重复内容；
- 绑定的命令失效（工具改名/删除）→ 要么改命令，要么退役。

## 4. 省 token 的三条硬规矩

1. **不抄入口文档**：`AGENTS.md`、`SOP.md` 已有的正文不重复，只写"怎么做"。
2. **一条 skill 一件事**：触发词写在 description 里；不要把"门禁 + 研究 + 文档"糅成一条。
3. **短**：≤60 行；超了先删例子，再拆条。

## 5. 快速调用

```
技能体检  node tools/skills-check.mjs && node tools/skills-doctor.mjs
技能安装  node tools/skills-install.mjs
```
会话里直接说"用 jingwei-gate" / "按 jingwei-graph 走"，模型会加载对应 skill（目录里能看到的 7 条）。

## 6. 当前清单（9 条）

| skill | 用途 | 来源 |
|---|---|---|
| `jingwei-gate` | 跑门禁、读红、数字真源 | 本项目事故账本 |
| `jingwei-graph` | 索引→位置→解释→交接 | 本项目 CLI |
| `jingwei-triage` | 三态与卡点协议 | 本项目铁律 |
| `jingwei-research` | 外部研究 + 可信度分级 + ASR | 原子能 #3 的来源实践 |
| `jingwei-docs` | 数字标记、结构约束、交付 | check_facts / pack.ps1 |
| `review-diff` | 提交前自查，只报真问题 | VS Code Copilot 技能结构（改编） |
| `commit-evidence` | 证据先行再提交 | VS Code Copilot 技能结构 + 我们的验收三样 |
| `systematic-debugging` | 先根因再修，四步 | superpowers-zh（MIT，改编） |
| `writing-plans` | 多步任务先写计划 | superpowers-zh（MIT，改编） |
