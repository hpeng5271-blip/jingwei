# LSP stdio：Content-Length 分帧，stdout 只走协议、stderr 只走日志

- 结论：`crates/engine/src/lsp/` 的 stdio 读写必须 Content-Length 分帧（`Content-Length: N\r\n\r\n{json}`），且 stdout 绝不允许混入日志（日志走 stderr）。
- 为什么现在能用：项目 LSP 客户端/服务端已走 stdio JSON-RPC，这条是协议底线，分帧或 stdout/stderr 混用会让整条会话崩。
- 落点：`crates/engine/src/lsp/mod.rs`（读/写帧函数）。
- 验证：现有 LSP 端到端测试（initialize→definition 往返）通过；补一条负向断言"stdout 不含日志行"。
- 来源：`inbox/2026-10-23-lsp-content-length分帧.md`
- 采用时间：2026-10-23

## 落地核查（2026-10-23）：**已符合，不需要改代码**

| 规范要求 | 现状（`crates/engine/src/lsp/mod.rs`） | 结论 |
|---|---|---|
| `Content-Length: N\r\n\r\n{json}`，N 是**字节数** | `write_message()` 先 `serde_json::to_vec(v)` 再 `body.len()` | 符合（UTF-8 中文不会算错） |
| 长度缺失就不猜、不阻塞 | `read_message()` 走 `strip_prefix("Content-Length:")`，无头即 `Ok(None)`；有专门用例 | 符合 |
| stdout 只走协议、日志只走 stderr | 全模块 `println!` **零处**；日志一律 `eprintln!`（`cmd_lsp.rs` 启动、`mod.rs` 的 `lsp-debug`） | 符合 |

**下次怎么查**（30 秒，不必重读全文）：

```powershell
cd D:\projects\jingwei
Select-String -Path crates\engine\src\lsp\*.rs -Pattern "Content-Length|println!" 
Select-String -Path crates\cli\src\cmd_lsp.rs -Pattern "println!|eprintln!"
```

判据：`println!` 一处都不该有；`Content-Length` 的数值必须来自 `to_vec(...).len()`（字节），不能是 `String::len()` 之外的估算。

> 这条 adopted 的教训：**先核现状再改**。它本来就是对实现，若为了“落地”去重构，纯属反向工作。
