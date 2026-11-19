"""Run isolated official-protocol WeChat and QQ endpoints against three team identities."""
import argparse
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import threading
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit

team = import_module("verify-team")
release = import_module("verify-release")


def crypto(script, data):
    result = subprocess.run(["node", "-e", script], input=json.dumps(data), encoding="utf-8",
                            capture_output=True, check=True, timeout=10)
    return result.stdout


class Official(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def reply(self, value, status=200, **headers):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Connection", "close")
        self.close_connection = True
        self.send_header("Content-Length", str(len(body)))
        for k, v in headers.items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def ws_send(self, data):
        raw = json.dumps(data).encode()
        size = bytes([len(raw)]) if len(raw) < 126 else b"\x7e" + struct.pack(">H", len(raw))
        self.wfile.write(b"\x81" + size + raw)
        self.wfile.flush()

    def ws_read(self):
        first, size = self.rfile.read(2)
        length = size & 127
        if length == 126:
            length = struct.unpack(">H", self.rfile.read(2))[0]
        assert first & 15 == 1 and size & 128 and length < 65536
        key = self.rfile.read(4)
        return json.loads(bytes(b ^ key[i % 4] for i, b in enumerate(self.rfile.read(length))))

    def websocket(self):
        server = self.server
        raw = (self.headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", base64.b64encode(hashlib.sha1(raw).digest()).decode())
        self.end_headers()
        self.ws_send({"op": 10, "d": {"heartbeat_interval": 30000}})
        hello = self.ws_read()
        server.hello.append(hello)
        assert hello["d"]["token"] == "QQBot synthetic-access"
        if hello["op"] == 2:
            assert hello["d"]["intents"] & 1 << 25
            self.ws_send({"op": 0, "s": 1, "t": "READY", "d": {"session_id": "synthetic-session"}})
        else:
            assert hello["op"] == 6 and hello["d"]["session_id"] == "synthetic-session"
        for name, d in server.qq_events:
            server.sequence += 1
            self.ws_send({"op": 0, "s": server.sequence, "t": name, "d": d})
        server.qq_events.clear()
        self.ws_send({"op": 7, "d": None})
        self.close_connection = True

    def do_GET(self):
        route = urlsplit(self.path)
        if route.path == "/socket":
            return self.websocket()
        if route.path == "/gateway":
            assert self.headers["Authorization"] == "QQBot synthetic-access"
            return self.reply({"url": self.server.base.replace("http:", "ws:") + "/socket"})
        if route.path == "/ilink/bot/get_qrcode_status":
            q = parse_qs(route.query)
            assert q["qrcode"] == ["wx-ticket"] and not self.headers.get("Authorization")
            reply = {"status": self.server.wx_status}
            if self.server.wx_status == "confirmed":
                assert q["verify_code"] == ["123456"]
                reply.update(baseurl=self.server.base, bot_token="wx-secret-synthetic",
                             ilink_bot_id="wx-bot", ilink_user_id="wx-owner")
            return self.reply(reply)
        self.reply({}, 404)

    def do_POST(self):
        route = urlsplit(self.path).path
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if route == "/cdn/upload":
            self.server.cipher = raw.hex()
            return self.reply({}, **{"x-encrypted-param": "synthetic-download-handle"})
        q = json.loads(raw)
        if route == "/ilink/bot/get_bot_qrcode":
            assert q == {"local_token_list": []} and self.headers["iLink-App-Id"] == "bot"
            return self.reply({"qrcode": "wx-ticket", "qrcode_img_content": "weixin://synthetic-qr"})
        if route.startswith("/ilink/bot/"):
            assert self.headers["Authorization"] == "Bearer wx-secret-synthetic"
            assert q["base_info"]["channel_version"].startswith("3.0.1/ShipForge-")
        if route == "/ilink/bot/getupdates":
            if getattr(self.server, "wx_expired", False):
                return self.reply({"ret": -14})
            self.server.cursors.append(q["get_updates_buf"])
            messages = self.server.wx_events[:]
            self.server.wx_events.clear()
            return self.reply({"ret": 0, "msgs": messages, "get_updates_buf": "cursor-" + str(len(self.server.cursors))})
        if route == "/ilink/bot/getuploadurl":
            self.server.upload = q
            return self.reply({"upload_full_url": self.server.base + "/cdn/upload"})
        if route == "/ilink/bot/sendmessage":
            self.server.wx_sent.append(q["msg"])
            return self.reply({"ret": 0})
        if route == "/lite/create_bind_task":
            self.server.qr_key = q["key"]
            return self.reply({"retcode": 0, "data": {"task_id": "synthetic-qr"}})
        if route == "/lite/poll_bind_result":
            assert q["task_id"] == "synthetic-qr"
            envelope = crypto("const c=require('node:crypto'),k=JSON.parse(require('node:fs').readFileSync(0,'utf8'));"
                              "const iv=Buffer.alloc(12,7),e=c.createCipheriv('aes-256-gcm',Buffer.from(k,'base64'),iv);"
                              "process.stdout.write(Buffer.concat([iv,e.update('qq-secret-synthetic'),e.final(),e.getAuthTag()]).toString('base64'))", self.server.qr_key)
            return self.reply({"retcode": 0, "data": {"status": 2, "bot_appid": "20001", "bot_encrypt_secret": envelope}})
        if route == "/app/getAppAccessToken":
            if q != {"appId": "20001", "clientSecret": "qq-secret-synthetic"}:
                return self.reply({}, 401)
            return self.reply({"access_token": "synthetic-access", "expires_in": "7200"})
        if route.endswith("/messages") and route.startswith("/v2/"):
            assert self.headers["Authorization"] == "QQBot synthetic-access" and self.headers["X-Union-Appid"] == "20001"
            self.server.qq_sent.append((route, q))
            if q["msg_id"] == "ambiguous":
                return self.reply({}, 503)
            return self.reply({"id": "receipt-" + str(len(self.server.qq_sent))})
        self.reply({}, 404)


def wechat(number, body, sender="wx-owner"):
    return {"message_id": number, "from_user_id": sender, "to_user_id": "wx-bot", "message_type": 1,
            "context_token": "context-" + str(number), "item_list": [{"type": 1, "text_item": {"text": body}}]}


def verify(bin_dir):
    server, cli, worker = (release.binary(bin_dir, name) for name in ("shipforge-server", "shipforge", "shipforge-worker"))
    with ThreadingHTTPServer(("127.0.0.1", 0), Official) as upstream, tempfile.TemporaryDirectory() as tmp:
        upstream.base = f"http://127.0.0.1:{upstream.server_port}"
        upstream.wx_events, upstream.wx_sent, upstream.cursors = [], [], []
        upstream.qq_events, upstream.qq_sent, upstream.hello = [], [], []
        upstream.sequence = 1
        thread = threading.Thread(target=upstream.serve_forever, daemon=True)
        thread.start()
        root = Path(tmp)
        with patch.dict(os.environ, {"SHIPFORGE_BOT_LOOPBACK": upstream.base}):
            keys = {}
            for actor in ("owner", "alice", "bob"):
                raw = subprocess.check_output([server, "--data-dir", root/"team", "token", "issue", "--actor", actor, "--scope", "write"], encoding="utf-8")
                keys[actor] = json.loads(raw)["token"]
            process, addr = team.start(server, root/"team", "127.0.0.1:0")

            def call(op, *, actor="alice", ok=True, **fields):
                return release.run_cli(cli, root/actor, "request", request={"op": op, **fields}, ok=ok)

            machine = dict(machine_id="test-box", machine_key="synthetic-local-machine-adapter-key")

            def finish(text, artifacts=None):
                job = call("run.claim", **machine)
                call("run.report", **machine, id=job["run"]["id"], lease=job["lease"], status="done", result=text, artifacts=artifacts or [])
                return job

            try:
                for actor, key in keys.items():
                    team.connect(cli, root/actor, addr, key)
                for actor in ("alice", "bob"):
                    call("create", actor="owner", kind="member", id=actor, title=actor, fields={"role": "member"})
                for agent in ("tencent-agent", "second-agent"):
                    call("create", kind="agent", id=agent, title=agent, fields={"runtime": "test"})
                call("machine.register", **machine, title="Synthetic local worker", runtimes=["test"])
                def pair(provider):
                    return call("bot.qr.start", provider=provider, agent="tencent-agent")
                expired = pair("wechat")
                upstream.wx_status = "expired"
                assert call("bot.qr.check", id=expired["id"])["state"] == "expired"
                wx = pair("wechat")
                assert wx["qr_matrix"] and "wx-ticket" not in str(wx)
                wid = wx["id"]
                upstream.wx_status = "need_verifycode"
                assert call("bot.qr.check", id=wid)["state"] == "need_verifycode"
                upstream.wx_status = "confirmed"
                bound = call("bot.qr.check", id=wid, verify_code="123456")
                assert bound["account"] == "wx-owner" and "secret" not in str(bound)
                assert call("bot.bindings", actor="bob")["items"] == []
                assert call("bot.policy", actor="bob", id=wid, dm=False, ok=False)["code"] in ("not_found", "forbidden")
                assert call("bot.claim", id=wid, device_key="bad"*12, ok=False)["code"] == "forbidden"
                upstream.wx_events.extend([wechat(1, "private input"), wechat(1, "private input"), wechat(2, "foreign input", "other")])
                env = {k:v for k,v in os.environ.items() if k not in ("SHIPFORGE_SERVER","SHIPFORGE_TOKEN","SHIPFORGE_DATA_DIR")}
                once = subprocess.check_output([worker, "bot-sync", "--directory", root/"alice", "--space", "default", "--binding", wid], env=env, encoding="utf-8", timeout=15)
                assert json.loads(once)["received"] == 1
                content = b"synthetic attached artifact"
                finish("WeChat result", [{"name": "test.txt", "filename": "test.txt", "content_base64": base64.b64encode(content).decode()}])
                assert call("bot.sync", id=wid)["outgoing"] == 2
                assert upstream.cursors[:2] == ["", "cursor-1"]
                assert [d["context_token"] for d in upstream.wx_sent] == ["context-1", "context-1"]
                assert upstream.wx_sent[0]["item_list"][0]["text_item"]["text"] == "WeChat result"
                media = upstream.wx_sent[1]["item_list"][0]["file_item"]
                assert media["file_name"] == "test.txt" and media["len"] == str(len(content))
                assert media["media"]["encrypt_query_param"] == "synthetic-download-handle"
                assert base64.b64decode(media["media"]["aes_key"]).decode() == upstream.upload["aeskey"]
                plain = crypto("const c=require('node:crypto'),[key,data]=JSON.parse(require('node:fs').readFileSync(0,'utf8'));"
                               "const d=c.createDecipheriv('aes-128-ecb',Buffer.from(key,'hex'),null);"
                               "process.stdout.write(Buffer.concat([d.update(Buffer.from(data,'hex')),d.final()]).toString('base64'))", [upstream.upload["aeskey"], upstream.cipher])
                assert base64.b64decode(plain) == content
                original = call("list", kind="chat")[0]["id"]
                assert call("get", actor="bob", kind="chat", id=original, ok=False)["code"] == "not_found"
                upstream.wx_events.extend([wechat(3, "/new"), wechat(3, "/new")])
                assert call("bot.sync", id=wid)["outgoing"] == 1
                assert len(call("list", kind="chat")) == 2 and call("list", kind="task") == []
                qq = pair("qq")
                assert qq["qr_content"].startswith("https://q.qq.com/") and "synthetic-qr" in qq["qr_content"]
                qid = qq["id"]
                assert call("bot.qr.check", id=qid)["account"] == "20001"
                direct = ("C2C_MESSAGE_CREATE", {"id": "q-direct", "author": {"user_openid": "friend"}, "content": "direct hello"})
                group = ("GROUP_AT_MESSAGE_CREATE", {"id": "q-group", "group_openid": "class", "author": {"member_openid": "member"}, "content": "group hello"})
                upstream.qq_events.extend([direct, direct, group, ("GROUP_MESSAGE_CREATE", group[1])])
                assert call("bot.sync", id=qid)["received"] == 2
                finish("QQ result")
                finish("QQ result")
                assert call("bot.sync", id=qid)["outgoing"] == 2
                assert [h["op"] for h in upstream.hello[:2]] == [2,6]
                assert {(p,b["msg_id"],b["msg_seq"]) for p,b in upstream.qq_sent} == {("/v2/groups/class/messages","q-group",1),("/v2/users/friend/messages","q-direct",1)}
                upstream.qq_events.append((direct[0], {**direct[1], "id": "ambiguous"}))
                assert call("bot.sync", id=qid)["received"] == 1
                finish("check the original conversation")
                assert call("bot.sync", id=qid)["outgoing"] == 0
                pending = next(b for b in call("bot.bindings")["items"] if b["id"] == qid)["deliveries"]
                assert len(pending) == 1 and pending[0]["state"] == "uncertain"
                count = len(upstream.qq_sent)
                assert call("bot.sync", id=qid)["outgoing"] == 0 and len(upstream.qq_sent) == count
                assert call("bot.retry", id=qid, delivery=pending[0]["id"], decision="confirm")["state"] == "sent"
                call("bot.policy", id=qid, groups=False)
                upstream.qq_events.append((group[0],{**group[1],"id":"ignored-group"}))
                assert call("bot.sync", id=qid)["received"] == 0
                replaced = call("bot.qq.connect", app_id="20001", app_secret="qq-secret-synthetic", agent="second-agent")
                assert replaced["agent"] == "second-agent"
                assert call("bot.sync", id=qid, ok=False)["code"] == "conflict"
                assert call("bot.qq.connect", actor="bob", app_id="20001", app_secret="qq-secret-synthetic", agent="second-agent", ok=False)["code"] in ("conflict","not_found","forbidden")
                upstream.wx_expired = True
                call("bot.sync", id=wid, ok=False)
                assert next(b for b in call("bot.bindings")["items"] if b["id"] == wid)["state"] == "expired"
                call("bot.disconnect", id=wid)
                call("bot.disconnect", id=replaced["id"])
                assert call("bot.sync", id=wid, ok=False)["code"] == "conflict"
                assert all(b["state"] != "connected" for b in call("bot.bindings")["items"])
                for db in (root/"team").glob("*.sqlite*"):
                    assert b"wx-secret-synthetic" not in db.read_bytes() and b"qq-secret-synthetic" not in db.read_bytes()
            finally:
                team.stop(process)
                upstream.shutdown()
                thread.join(5)
    print("Personal WeChat and QQ passed isolated QR, gateway, private turn, delivery, file and rebind checks.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("bin_dir",type=Path)
    verify(parser.parse_args().bin_dir)
