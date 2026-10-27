// 测试用 LSP server（fixture）：initialize + publishDiagnostics（push）+ pull + shutdown/exit。
// 用法：node lsp-echo.mjs <markerPath> [file]
import { writeFileSync } from "node:fs";

const marker = process.argv[2];
const target = process.argv[3] || "src/lib.rs";
if (marker) {
  try { writeFileSync(marker, `spawned ${new Date().toISOString()}\n`, "utf8"); } catch {}
}

const DIAG = [
  { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 8 } }, severity: 1, code: "E001", message: "lsp: unused function a" },
  { range: { start: { line: 1, character: 4 }, end: { line: 1, character: 5 } }, severity: 2, code: "W001", message: "lsp: unused variable x" },
  { range: { start: { line: 2, character: 0 }, end: { line: 2, character: 6 } }, severity: 3, code: "I001", message: "lsp: comment style" }
];

function send(obj) {
  const body = JSON.stringify(obj);
  process.stdout.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}

send({ jsonrpc: "2.0", id: 0, result: { capabilities: { diagnosticProvider: { interFileDependencies: true } }, serverInfo: { name: "jw-fixture-lsp", version: "0.1.0" } } });

setTimeout(() => {
  send({ jsonrpc: "2.0", method: "textDocument/publishDiagnostics", params: { uri: `file:///${target}`, diagnostics: DIAG } });
}, 60);

let buf = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buf += chunk;
  let idx;
  while ((idx = buf.indexOf("\r\n\r\n")) >= 0) {
    const header = buf.slice(0, idx);
    const m = /Content-Length:\s*(\d+)/i.exec(header);
    if (!m) { buf = buf.slice(idx + 4); continue; }
    const len = Number(m[1]);
    const start = idx + 4;
    if (buf.length < start + len) break;
    const body = buf.slice(start, start + len);
    buf = buf.slice(start + len);
    let msg;
    try { msg = JSON.parse(body); } catch { continue; }
    if (msg.method === "initialize") {
      send({ jsonrpc: "2.0", id: msg.id, result: { capabilities: { diagnosticProvider: { interFileDependencies: true } } } });
    } else if (msg.method === "textDocument/diagnostic") {
      send({ jsonrpc: "2.0", id: msg.id, result: { kind: "full", items: DIAG } });
    } else if (msg.method === "shutdown") {
      send({ jsonrpc: "2.0", id: msg.id, result: null });
    } else if (msg.method === "exit") {
      process.exit(0);
    }
  }
});
process.stdin.on("end", () => process.exit(0));
