# LSP stdio 的 Content-Length 分帧写法（Rust 官方论坛 106810）

- 抓取时间：2026-10-23T06:54+08:00
- 来源：https://users.rust-lang.org/t/writing-a-client-for-rust-analyzer/106810
- 通道：`node tools/web.mjs fetch https://users.rust-lang.org/t/writing-a-client-for-rust-analyzer/106810.json`
- 可信度：原话（帖首代码）

## 原文摘录

> ```js
> const server = spawn(rustAnalyzerPath, [], { stdio: ['pipe', 'pipe', process.stderr] });
> const message = JSON.stringify({ jsonrpc: '2.0', method, params, id });
> const contentLength = Buffer.byteLength(message, 'utf8');
> server.stdin.write(`Content-Length: ${contentLength}\r\n\r\n${message}`);
> ```

## 它回答了哪个问题

Rust CLI 常驻进程走 stdio JSON 协议的稳妥分帧：每条消息 `Content-Length: N\r\n\r\n{json}`；stdout 只走协议帧、stderr 走日志（`process.stderr`）。
