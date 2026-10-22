# LSP stdio：Content-Length 分帧，stdout 只走协议、stderr 只走日志

- 结论：`crates/engine/src/lsp/` 的 stdio 读写必须 Content-Length 分帧（`Content-Length: N\r\n\r\n{json}`），且 stdout 绝不允许混入日志（日志走 stderr）。
- 为什么现在能用：项目 LSP 客户端/服务端已走 stdio JSON-RPC，这条是协议底线，分帧或 stdout/stderr 混用会让整条会话崩。
- 落点：`crates/engine/src/lsp/mod.rs`（读/写帧函数）。
- 验证：现有 LSP 端到端测试（initialize→definition 往返）通过；补一条负向断言"stdout 不含日志行"。
- 来源：`inbox/2026-10-23-lsp-content-length分帧.md`
- 采用时间：2026-10-23
