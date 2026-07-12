#!/usr/bin/env python3
import base64, json, os, pathlib, socket, struct, subprocess, sys, tempfile, time, urllib.parse, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = pathlib.Path(os.environ.get("COPILOTMAIL_BINARY", ROOT / "target" / "debug" / ("copilotmail.exe" if os.name == "nt" else "copilotmail")))

def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]

def http(base, method, path, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(base + path, data=data, method=method, headers={"Content-Type":"application/json", **(headers or {})})
    with urllib.request.urlopen(request, timeout=5) as response:
        payload = response.read()
        return response.status, json.loads(payload) if payload else None

def smtp(port, recipient, subject="SMTP integration"):
    with socket.create_connection(("127.0.0.1", port), timeout=5) as sock:
        stream = sock.makefile("rwb", buffering=0)
        def read():
            line = stream.readline().decode(); assert line[:1] in "235", line; return line
        read()
        for command in [b"EHLO test\r\n", b"MAIL FROM:<sender@example.test>\r\n", f"RCPT TO:<{recipient}>\r\n".encode(), b"DATA\r\n"]:
            stream.write(command)
            line = read()
            while len(line) > 3 and line[3] == "-": line = read()
        stream.write(f"From: sender@example.test\r\nTo: {recipient}\r\nSubject: {subject}\r\n\r\nhello agent\r\n.\r\n".encode())
        assert read().startswith("250")

def websocket(port, token):
    sock = socket.create_connection(("127.0.0.1", port), timeout=5)
    key = base64.b64encode(os.urandom(16)).decode()
    path = "/v1/events?token=" + urllib.parse.quote(token)
    request = f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    sock.sendall(request.encode())
    response = b""
    while b"\r\n\r\n" not in response: response += sock.recv(1024)
    assert response.startswith(b"HTTP/1.1 101"), response
    return sock

def ws_json(sock):
    first, second = sock.recv(2)
    length = second & 0x7f
    if length == 126: length = struct.unpack("!H", sock.recv(2))[0]
    elif length == 127: length = struct.unpack("!Q", sock.recv(8))[0]
    payload = b""
    while len(payload) < length: payload += sock.recv(length-len(payload))
    assert first & 0x0f == 1
    return json.loads(payload)

def main():
    http_port, smtp_port = free_port(), free_port()
    with tempfile.TemporaryDirectory() as temp:
        env = {**os.environ, "COPILOTMAIL_HTTP_ADDR":f"127.0.0.1:{http_port}", "COPILOTMAIL_SMTP_ADDR":f"127.0.0.1:{smtp_port}", "COPILOTMAIL_DATABASE":str(pathlib.Path(temp)/"mail.db"), "COPILOTMAIL_DOMAIN":"mail.local", "COPILOTMAIL_JWT_SECRET":"integration-jwt-secret-32-characters", "COPILOTMAIL_ADMIN_KEY":"integration-admin-key", "COPILOTMAIL_WEB_DIR":str(ROOT/"web"), "RUST_LOG":"error"}
        process = subprocess.Popen([str(BINARY)], cwd=ROOT, env=env)
        try:
            base = f"http://127.0.0.1:{http_port}"
            for _ in range(60):
                try:
                    if http(base,"GET","/healthz")[0] == 200: break
                except Exception: time.sleep(.1)
            else: raise RuntimeError("server did not start")
            mailboxes=[]
            for name in ("agent","peer"):
                status, mailbox=http(base,"POST","/v1/mailboxes",{"address":f"{name}@mail.local","password":"integration-pass"},{"X-Admin-Key":"integration-admin-key"}); assert status==201; mailboxes.append(mailbox)
            _, auth=http(base,"POST","/v1/auth/token",{"address":"agent@mail.local","password":"integration-pass"}); token=auth["token"]; bearer={"Authorization":"Bearer "+token}
            ws=websocket(http_port,token)
            smtp(smtp_port,"agent@mail.local")
            event=ws_json(ws); assert event["type"]=="message.received"
            _, inbox=http(base,"GET","/v1/messages",headers=bearer); assert len(inbox)==1
            message_id=inbox[0]["id"]
            _, detail=http(base,"GET",f"/v1/messages/{message_id}",headers=bearer); assert detail["text"].strip()=="hello agent"
            assert http(base,"PATCH",f"/v1/messages/{message_id}",{"seen":True},bearer)[0]==204
            assert http(base,"DELETE",f"/v1/messages/{message_id}",headers=bearer)[0]==204
            assert http(base,"POST","/v1/messages/send",{"to":"peer@mail.local","subject":"API integration","text":"hello peer"},bearer)[0]==202
            _, peer_auth=http(base,"POST","/v1/auth/token",{"address":"peer@mail.local","password":"integration-pass"}); peer_bearer={"Authorization":"Bearer "+peer_auth["token"]}
            _, peer_inbox=http(base,"GET","/v1/messages",headers=peer_bearer); assert peer_inbox[0]["subject"]=="API integration"
            with urllib.request.urlopen(base+"/",timeout=5) as response: assert b"CopilotMail" in response.read()
            assert http(base,"DELETE",f"/v1/mailboxes/{mailboxes[0]['id']}",headers=bearer)[0]==204
            print("E2E PASS: HTTP + auth + SMTP + WebSocket + send + CRUD + frontend")
        finally:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill()

if __name__ == "__main__": main()
