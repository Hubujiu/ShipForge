"""Two loopback Feishu/Lark clouds with device authorization, official WS frames and IM APIs."""
import base64
import hashlib
import json
import struct
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


def number(n):
    out = bytearray()
    while n > 127:
        out.append(128 | (n & 127))
        n >>= 7
    return bytes([*out, n])


def scalar(tag, value):
    return number(tag << 3) + number(value)


def buf(tag, data):
    if isinstance(data, str):
        data = data.encode()
    return number(tag << 3 | 2) + number(len(data)) + data


def decode(data):
    from io import BytesIO
    stream, values = BytesIO(data), {}

    def integer():
        size = bits = 0
        while True:
            b = stream.read(1)[0]
            size |= (b & 127) << bits
            if b < 128:
                return size
            bits += 7

    while stream.tell() < len(data):
        tag = integer()
        assert tag & 7 in (0, 2)
        n = integer()
        values.setdefault(tag >> 3, []).append(stream.read(n) if tag & 7 else n)
    return values


def frame(sequence, method, data, headers):
    encoded = b"".join(buf(5, buf(1, k) + buf(2, str(v))) for k, v in headers.items())
    return scalar(1, sequence) + scalar(2, sequence + 100) + scalar(3, 17) + scalar(4, method) + encoded + buf(8, data)


class Cloud(ThreadingHTTPServer):
    def __init__(self, brand):
        super().__init__(("127.0.0.1", 0), Handler)
        self.brand, self.base = brand, f"http://127.0.0.1:{self.server_port}"
        self.next_brand = brand
        self.events, self.replies, self.acks, self.reactions, self.uploads = [], [], [], [], []
        self.accounts, self.messages = {}, {}
        self.sequence = self.connects = 0
        self.revoked = False

    def push(self, msg_id, text=None, *, room="oc_group", chat_type="group", who="ou_member",
             mention=None, root="", topic="", kind="text", body=None, tenant="tenant_sample",
             app="cli_sample", event=None):
        content = json.dumps(body if body is not None else {"text": text})
        user = {"key": "@_user_1", "id": {"open_id": f"ou_{mention}"}, "name": "Synthetic"}
        m = dict(message_id=msg_id, chat_id=room, chat_type=chat_type, message_type=kind, content=content,
                 mentions=[user] if mention else [], root_id=root, thread_id=topic)
        self.messages[msg_id] = m
        self.events.append({"schema": "2.0", "header": dict(event_id=event or "ev_"+msg_id, tenant_key=tenant, app_id=app, event_type="im.message.receive_v1"),
                            "event": {"sender": {"sender_type": "user", "sender_id": {"open_id": who}, "tenant_key": tenant}, "message": m}})

    def react(self, source, emoji, eid):
        self.events.append({"schema": "2.0", "header": {"event_id": eid, "tenant_key": "tenant_sample", "app_id": "cli_sample", "event_type": "im.message.reaction.created_v1"},
                            "event": {"message_id": source, "operator_type": "user", "user_id": {"open_id": "ou_another"}, "reaction_type": {"emoji_type": emoji}}})


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def reply(self, payload, code=200, mime=None):
        body = payload if mime else json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", mime or "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def send_frame(self, payload, opcode=2):
        length = bytes([len(payload)]) if len(payload) < 126 else b"\x7e" + struct.pack(">H", len(payload))
        self.wfile.write(bytes([0x80 | opcode]) + length + payload)
        self.wfile.flush()

    def read_frame(self):
        first, size = self.rfile.read(2)
        n = struct.unpack(">H", self.rfile.read(2))[0] if size & 127 == 126 else (size & 127)
        assert first == 0x82 and size & 128 and n < 65536
        mask = self.rfile.read(4)
        return decode(bytes(b ^ mask[i % 4] for i, b in enumerate(self.rfile.read(n))))

    def socket(self):
        s = self.server
        self.connection.settimeout(10)
        accept = hashlib.sha1((self.headers["Sec-WebSocket-Key"]+"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        self.send_response(101)
        for k, val in [("Upgrade", "websocket"), ("Connection", "Upgrade"), ("Sec-WebSocket-Accept", base64.b64encode(accept).decode())]:
            self.send_header(k, val)
        self.end_headers()
        ping = self.read_frame()
        assert ping[4] == [0] and ping[3] == [17] and decode(ping[5][0])[2] == [b"ping"]
        self.send_frame(frame(0, 0, b'{"PingInterval":60}', {"type": "pong"}))
        events, s.events = s.events, []
        for event in events:
            data = json.dumps(event).encode()
            mid = len(data)//2
            s.sequence += 1
            # Deliver both halves in reverse order, under a single message key.
            for i, part in [(1, data[mid:]), (0, data[:mid])]:
                self.send_frame(frame(s.sequence, 1, part, dict(type="event", message_id=f"wire-{s.sequence}", seq=i, sum=2)))
            ack = self.read_frame()
            assert ack[1] == [s.sequence] and ack[2] == [s.sequence+100]
            code = json.loads(ack[8][0])["code"]
            s.acks.append(code)
            if code != 200:
                break
        self.send_frame(b"", 8)
        self.close_connection = True

    def authorized(self):
        assert self.headers.get("Authorization") == "Bearer token-"+self.server.brand

    def do_GET(self):
        s = self.server
        route = urlsplit(self.path).path
        if route == "/socket":
            return self.socket()
        self.authorized()
        if route == "/open-apis/bot/v3/info":
            return self.reply({"code": 0, "bot": {"open_id": "ou_robot"}})
        if "/resources/" in route:
            assert route.endswith("/resources/img_demo") and self.path.endswith("?type=image")
            return self.reply(b"\x89PNG\r\n\x1a\nsynthetic-input", mime="image/png")
        if "/chats/" in route:
            channel = route.split("/")[-1]
            return self.reply({"code": 0, "data": {"chat_mode": {"oc_topic":"topic","oc_dm":"p2p"}.get(channel,"group"), "chat_type": "private"}})
        if "/messages/" in route:
            m = s.messages[route.split("/")[-1]]
            return self.reply({"code": 0, "data": {"items": [{**{k:v for k,v in m.items() if k not in ("message_type","content","chat_type")}, "msg_type": m["message_type"], "body": {"content": m["content"]}}]}})
        self.reply({}, 404)

    def do_DELETE(self):
        self.authorized()
        assert self.path.endswith("/reactions/mark")
        self.server.reactions.append(("removed", self.path))
        self.reply({"code": 0})

    def do_POST(self):
        route, s = urlsplit(self.path).path, self.server
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if route == "/oauth/v1/app/registration":
            q = parse_qs(raw.decode())
            if q["action"] == ["begin"]:
                assert s.brand == "feishu" and q["archetype"] == ["PersonalAgent"] and q["auth_method"] == ["client_secret"]
                ticket = s.next_brand + "-" + str(len(s.accounts))
                s.accounts[ticket] = s.next_brand
                return self.reply({"device_code": "device-"+ticket, "user_code": "user-"+ticket, "expire_in": 300, "interval": 1,
                                   "verification_uri": "https://accounts.feishu.cn"})
            ticket = q["device_code"][0].removeprefix("device-")
            brand = ticket.split("-")[0]
            if brand == "denied":
                return self.reply({"error": "access_denied"}, 400)
            if s.brand != brand:
                return self.reply({"error": "authorization_pending", "user_info": {"tenant_brand": brand}}, 400)
            return self.reply({"client_id": "cli_sample", "client_secret": "secret-"+brand, "user_info": {"tenant_brand": brand}})
        if route == "/open-apis/im/v1/files":
            self.authorized()
            assert self.headers["Content-Type"].startswith("multipart/form-data; boundary=")
            s.uploads.append(raw)
            return self.reply({"code": 0, "data": {"file_key": "file_return"}})
        q = json.loads(raw)
        if route == "/open-apis/auth/v3/tenant_access_token/internal":
            assert q == {"app_id": "cli_sample", "app_secret": "secret-"+s.brand}
            return self.reply({"code": 0, "tenant_access_token": "token-"+s.brand, "expire": 7200})
        if route == "/callback/ws/endpoint":
            assert q == {"AppID": "cli_sample", "AppSecret": "secret-"+s.brand}
            s.connects += 1
            if s.revoked:
                return self.reply({"code": 514, "msg": "invalid credentials"})
            return self.reply({"code": 0, "data": {"URL": s.base.replace("http:", "ws:")+"/socket?service_id=17", "ClientConfig": {"PingInterval": 60}}})
        self.authorized()
        if route.endswith("/reactions"):
            assert q == {"reaction_type": {"emoji_type": "OnIt"}}
            s.reactions.append(("added", route))
            return self.reply({"code": 0, "data": {"reaction_id": "mark"}})
        if route.endswith("/reply"):
            s.replies.append((route.split("/")[-2], q["msg_type"], json.loads(q["content"]), q["reply_in_thread"], q["uuid"]))
            return self.reply({"code": 0, "data": {"message_id": f"receipt_{len(s.replies)}"}})
        self.reply({}, 404)
