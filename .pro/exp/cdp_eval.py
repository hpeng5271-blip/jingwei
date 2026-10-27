import socket, base64, os, json, struct, sys, urllib.request


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


def evaluate(ws, expr, rid=1):
    ws_send(ws, json.dumps({"id": rid, "method": "Runtime.evaluate",
                            "params": {"expression": expr, "returnByValue": True}}))
    while True:
        raw = ws_recv(ws)
        if not raw:
            return {}
        msg = json.loads(raw)
        if msg.get("id") == rid:
            return msg


def main():
    expr = sys.argv[1] if len(sys.argv) > 1 else "document.body.innerText.slice(0,120)"
    data = json.loads(urllib.request.urlopen("http://127.0.0.1:9222/json/list", timeout=5).read())
    page = [t for t in data if t.get("type") == "page"][0]
    path = page["webSocketDebuggerUrl"].replace("ws://127.0.0.1:9222", "")
    ws = ws_connect("127.0.0.1", 9222, path)
    res = evaluate(ws, expr)
    out = res.get("result", {}).get("result", {})
    print(json.dumps(out, ensure_ascii=False)[:800])
    ws.close()


if __name__ == "__main__":
    main()
