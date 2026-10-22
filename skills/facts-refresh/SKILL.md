---
name: facts-refresh
description: 改完测试/断言之后刷新真源并提交的固定链条。当要提交、要改文档里的数字、或问"facts 对不对/门禁绿不绿"时使用。
---

# 刷真源 → 跑门禁 → 提交（顺序不能换）

## 为什么要一条固定链

数字手抄会漂；门禁绿了但文档写着旧数字，等于**用一份假说明过门禁**。
所以：**先让工具写数字，再让门禁验，最后才提交**。

## 步骤

```powershell
cd D:\projects\jingwei
node tools/collect_facts.mjs                  # 1. 重算真源 → tools/facts.json
node tools/check_facts.mjs --write-docs       # 2. 把数字写进带 fact 标记的文档
node tools/check_facts.mjs                    #    再跑一次，必须逐个相等
node tools/check-links.mjs                    # 3. 断链（新文档最容易漏）
powershell -NoProfile -ExecutionPolicy Bypass -File tools\gates.ps1   # 4. 门禁
```

## 判断"能不能提交"（缺一不算）

1. 门禁结尾那张表全部通过；
2. `git status` 只剩你要提交的那些文件；
3. **改动前确实红过**的输出（新判据没有红证据 = 没验证）。

## 常见坑

- 改了 `.ps1` 先确认**文件带 BOM**，否则 PowerShell 5.1 把中文读成乱码、语法直接报错。
- 文档里写规模数字却没加 fact 标记 → `check_facts` 会红（它是对的；标记写法看其它文档示例）。
- 文档里**不要写标记的字面量**（会被当成真源标记，报"只有开头没有闭合"）。
- 恢复文件用 `Copy-Item` 会带走 mtime，cargo 认为没变就不重编 → "恢复了还是红"。`(Get-Item f).LastWriteTime = Get-Date` 一下。
