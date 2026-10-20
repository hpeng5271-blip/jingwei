# `docs/knowledge/` · 知识条目（漏斗最细的那道颈）

> 这里的每条 `.md` 是一个**知识条目**，格式固定、可被机器严筛。
> 筛它的命令：`jingwei knowledge check` —— 缺来源/缺等级/等级写错/来源路径不存在的条目**当场红**；
> 关联符号在图谱里查不到的，只算**查不了**（三态），不许算「挂上图谱」。

## 条目格式

```text
---
claim: 一句话主张（这条到底在说什么）
source: https://… 或 docs/…（仓库内相对路径必须真的存在）
level: 一手 | 转写 | 二手 | 查不了
check: 怎么验（命令、测试名或判据；写"懂了就好"不算）
symbol: symbol:crates/engine/src/graph/store.rs#Store::apply_file_update
---
正文（可选：语境、边界、别处怎么用）
```

- `level` 四个值不许自创：**一手** = 论文/官方文档/源码/本项目实测；**转写** = ASR 文本（有错字）；
  **二手** = 只有标题/分章/摘录；**查不了** = 拿不到，或拿不到就不当判据。
- `symbol` 可写多行，也可写 `symbols: a, b`；**不带 `symbol:` 前缀的按名字查**（同名多义也算查到）。
  路径要从**仓库根**算。
- `check-run: <命令>`（可选）让判据**真的跑**：`jingwei knowledge verify` 逐条执行，期望 exit 0；
  要期望非 0 就加 `check-exit: N`。命令只许是**构建/测试类**（`cargo ` / `node ` / `python ` /
  `powershell -File tools\…` 白名单），带时中文参数也行（直接 spawn，不经 shell）。
  **没写 `check-run` 的条目算「不可执行」，不算错**——可执行判据是增量，不是强制。

## 让判据真的跑：`jingwei knowledge verify`

```powershell
jingwei knowledge verify              # 工作区必须干净；脏了就拒绝跑
jingwei knowledge verify --allow-dirty  # 开发时用；跑完会检查命令有没有改动工作区
```

- 输出三档：**通过 P / 没过 F / 不可执行 U**；F>0 → exit=1，U 不红。
- 两类判据：①`check-run` 的退出码 ②`check` 里**按约定形式**引用的测试名（反引号里的下划线标识符、
  `--test <名>`）必须真的存在——后者是存在性判定，拿不到测试名单时算 U，不冤枉条目。
- **别用 `target\debug\jingwei.exe` 跑**：cargo 要重编并替换这个 exe，Windows 拒绝删正在运行的文件
  （exit 101，看起来像判据没过）。用装好的 `jingwei`。
- 它**不进任何门禁**：自动跑命令会 flaky、会慢，verify 只手动跑。

## 回收半边：待重编清单 + 一键改挂

```powershell
jingwei knowledge stale                                  # 列出「条目引用的符号图谱里查不到」的条目，N>0 时 exit=1
jingwei knowledge fix <条目> --symbol <新符号|symbol:id>   # 精确改挂（名字重名会拒绝，要求用 id）
```

- `stale` **并进了真源门禁**：有 0 条才绿——否则"只报不修"会变成新的报而不回收。
- `fix` 只认**精确匹配**：名字同名多义会拒绝并要求 `symbol:` 全 id；新符号在图谱里查不到也拒绝。
- 没有图谱时 `stale` 打印「查不了」并 exit=0（干净克隆不该因为还没 `index` 而红）。

## 主张层：条目落进图谱（`claims` 表）

```powershell
jingwei knowledge sync     # 把 docs/knowledge/*.md 投影成图谱里的 claims + claim_edges
```

- 表 `claims` 与 `claim_edges` **单独存在**，不塞进 `nodes`：claim 不是代码实体，
  混进去会让"查符号"查到主张名、`positions` 原文回读错位。claim 只通过边与符号相连。
- `knowledge add` / `knowledge fix` 会自动同步；手改条目后要跑一次 `sync`，
  否则 `knowledge check` 会红：「图谱里的主张 N 条 ≠ 过筛条目 M 条——投影过期了」。
- `jingwei symbol` 现在是**走图边**返回主张（不再回头 grep Markdown）；
  `jingwei graph stats` 能看到 `claims` / `已挂符号` 两行；`jingwei handover` 多一行「③b 知识层」。

## 学到哪了：进度落在本地，不进仓库

```powershell
jingwei learn 原子能                                  # 6 步，○/● 标进度
jingwei learn --symbol <符号|symbol:id>               # 从代码出发：这块代码被哪几条主张解释过
jingwei learn 原子能 --mark 3                 # 打勾：可以给序号（第 3 步），也可以给条目名
jingwei progress 原子能                              # 同一视图：已学 N/总 M
```

- 进度存 `%LOCALAPPDATA%\jingwei\learn-progress.json`：它是**个人的学习状态**，不是项目事实，
  所以不进 git、不进任何门禁（否则每次学习都产生脏提交与冲突）。
- `--mark` 接受**序号**（`--mark 3` = 列表里第 3 步）或条目名；两者都只认 learn 真输出过的步骤，
  越界/不存在直接拒绝（防止乱标成"伪进度"）。

## 不算条目的文件

- `README.md`（本页）与 `_` 开头的文件：说明与草稿，不参与漏斗计数。
  临时验证用 `_` 前缀，验完删掉。

## 采集半边：候选 → 人确认 → 入账

```powershell
jingwei harvest docs/research/原子能-转写要点.md   # 抽逐字候选到 _inbox/
jingwei knowledge add 原子能-转写要点-L43.md       # 人确认（先补 check 字段）后入账
```

- 候选在 `_inbox/`，**不计入"采集"**——漏斗数字只算真条目。
- 候选的 `claim` 必须是源文件那一行的**原文子串**，`source` 写成 `文件:行号`；
  `knowledge check` 每次都回去核对：源文件删了那行 / claim 被改写 → **红**（来源失效 / 与原文不一致）。
- `add` 前必须把 `check` 从 `待验` 改成真判据，否则拒绝入库（不许把没有判据的东西放进库）。

## 漏斗表怎么读

```
知识条目：采集 4 / 过筛 4 / 挂图谱 2 / 缺口 0
```

| 数 | 含义 | 不满足时 |
|---|---|---|
| 采集 | 目录里条目数 | —— |
| 过筛 | 四个字段齐全 | 该条不计数，红（exit 1） |
| 挂图谱 | 关联符号**全部**在图谱里查到 | 该条只算查不了，进缺口 |
| 缺口 | 过筛了但挂不上的条数 | 如实报，不拦（查不了也是结论） |

**为什么这样设计**：采集和初筛容易做得热闹，最难的是「最严的那道颈」——
所以判据必须机械（缺字段就红），而"查不了"必须显式（不许拿"没报错"冒充"验过了"）。
