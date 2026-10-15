// 只给开发用：把 ui/ 目录用 HTTP 服务起来，方便在浏览器里直接检查布局与样式。
// 生产外壳不会用到它（Tauri 自己是把 ui/ 打进二进制里）。
const http = require('http');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', 'ui');
const types = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png' };

http.createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  const file = path.join(root, url === '/' ? 'index.html' : url);
  if (!file.startsWith(root)) { res.writeHead(403).end(); return; }
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404).end('not found'); return; }
    res.writeHead(200, { 'content-type': types[path.extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(data);
  });
}).listen(7317, '127.0.0.1', () => console.log('ui on http://127.0.0.1:7317'));
