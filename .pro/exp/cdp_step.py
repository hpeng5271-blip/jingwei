# cdp_step.py — WebView2 CDP 单步驱动（本轮唯一真正的测试资产）
# 用法: 先设 env WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 并启动 jingwei-gui.exe，
#       再 `python cdp_step.py <state|detail|open|tab:run|tab:editor|tab:note|tab:graph|tab:machine|tab:clean|text>`，
#       每条打印 `step -> value`；配合 PowerShell 截图即为"自动点+每步截图+state 断言"。
import socket, base64, os, json, struct, time, sys, urllib.request


def ws_connect(host, port, path):
    s = socket.create_connection((host, port), timeout=15)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n"
        f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(req.encode())
    resp = b""
    while b"\r\n\r\n" not in resp:
        chunk = s.recv(4096)
        if not chunk:
            break
        resp += chunk
    return s


def ws_send(s, payload):
    data = payload.encode()
    mask = os.urandom(4)
    header = bytearray([0x81])
    ln = len(data)
    if ln < 126:
        header.append(0x80 | ln)
    elif ln < 65536:
        header.append(0x80 | 126)
        header += struct.pack(">H", ln)
    else:
        header.append(0x80 | 127)
        header += struct.pack(">Q", ln)
    header += mask
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    s.sendall(bytes(header) + masked)


def ws_recv(s):
    hdr = s.recv(2)
    if not hdr:
        return ""
    ln = hdr[1] & 0x7F
    if ln == 126:
        ln = struct.unpack(">H", s.recv(2))[0]
    elif ln == 127:
        ln = struct.unpack(">Q", s.recv(8))[0]
    data = b""
    while len(data) < ln:
        chunk = s.recv(ln - len(data))
        if not chunk:
            break
        data += chunk
    return data.decode()


def eval_expr(expr):
    data = json.loads(urllib.request.urlopen("http://127.0.0.1:9222/json/list", timeout=5).read())
    page = [t for t in data if t.get("type") == "page"][0]
    s = ws_connect("127.0.0.1", 9222, page["webSocketDebuggerUrl"].replace("ws://127.0.0.1:9222", ""))
    ws_send(s, json.dumps({"id": 1, "method": "Runtime.evaluate",
                           "params": {"expression": expr, "returnByValue": True}}))
    while True:
        raw = ws_recv(s)
        if not raw:
            s.close()
            return None
        msg = json.loads(raw)
        if msg.get("id") == 1:
            s.close()
            return msg.get("result", {}).get("result", {}).get("value")


STEPS = {
    "state": "document.title + '|detail=' + document.querySelectorAll('[data-act=\"detail\"]').length",
    "detail": "document.querySelectorAll('[data-act=\"detail\"]')[0].click(); 'ok'",
    "open": "document.querySelectorAll('[data-act=\"open\"]')[0].click(); 'ok'",
    "tab:run": "document.querySelector('[data-tab=\"run\"]').click(); 'ok'",
    "tab:home": "document.querySelector('[data-tab=\"home\"]').click(); 'ok'",
    "tab:editor": "document.querySelector('[data-tab=\"editor\"]').click(); 'ok'",
    "tab:note": "document.querySelector('[data-tab=\"note\"]').click(); 'ok'",
    "tab:graph": "document.querySelector('[data-tab=\"graph\"]').click(); 'ok'",
    "tab:machine": "document.querySelector('[data-tab=\"machine\"]').click(); 'ok'",
    "tab:clean": "document.querySelector('[data-tab=\"clean\"]').click(); 'ok'",
    "text": "document.body.innerText.replace(/\\s+/g,' ').slice(0,3000)",
    # 工具链区块（machine 页）的格子数：找包含 uv 的那个 .dgrid，数它的子节点。
    "envtools": "(()=>{const g=[...document.querySelectorAll('.dgrid')].find(d=>d.innerText.indexOf('uv')>=0); return g?g.children.length:-1;})()",
    # 工具链区块的每个格子文本（用来核对名字逐项相同）
    "envtool_names": "(()=>{const g=[...document.querySelectorAll('.dgrid')].find(d=>d.innerText.indexOf('uv')>=0); if(!g) return 'none'; return [...g.children].map(c=>{const k=c.querySelector('.k'); return (k?k.innerText:c.innerText.split('\\n')[0]).trim();}).join(',');})()",
    # clean 页渲染的 item 数（#cleanList 下 .item 元素数，29 = A16/B5/C8）；未渲染返回 0。
    "clean_items": "document.querySelectorAll('#cleanList .item').length",
    # clean 页数据源回落标记：null=引擎直供；字符串=引擎失败已回落旧源（判据②）。
    "clean_error": "typeof S!=='undefined' ? JSON.stringify(S.cleanError) : 'noS'",
}

step = sys.argv[1]
print(step, "->", eval_expr(STEPS[step]))
