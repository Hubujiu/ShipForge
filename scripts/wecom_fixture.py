"""Isolated WeCom intelligent bot API simulator using standard-library WebSocket framing."""
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import struct
import subprocess
import threading


class WeCom(ThreadingHTTPServer):
    def __init__(self):
        super().__init__(("127.0.0.1",0),Handler)
        self.base=f"http://127.0.0.1:{self.server_port}"
        self.events=[]
        self.auth=[]
        self.replies=[]
        self.files={}
        self.uploads={}
        self.lock=threading.Lock()
        self.revoked=False
        self.lose_ack=False
        self.read_media=[]
        self.errors=[]

    def push(self,identifier,text=None,*,kind="text",who="alice-ww",group=None,content=None,bot="wecom-test",cmd="aibot_msg_callback"):
        body={"aibotid":bot,"msgid":identifier,"chattype":"group" if group else "single","msgtype":kind,
              "from":{"userid":who},kind:content if content is not None else {"content":text}}
        if group:
            body["chatid"]=group
        self.events.append({"cmd":cmd,"headers":{"req_id":"incoming-"+identifier},"body":body})

    def media(self,key,data,filename):
        secret=bytes(range(32))
        # Generate ciphertext independently with OpenSSL via Node, as documented by WeCom's SDK.
        js="""const c=require('crypto'),p=Buffer.from(process.argv[1],'base64'),k=Buffer.from(process.argv[2],'base64');
const a=c.createCipheriv('aes-256-cbc',k,k.subarray(0,16));a.setAutoPadding(false);
const n=32-p.length%32;process.stdout.write(Buffer.concat([a.update(Buffer.concat([p,Buffer.alloc(n,n)])),a.final()]));"""
        self.files[key]=(subprocess.check_output(["node","-e",js,base64.b64encode(data).decode(),base64.b64encode(secret).decode()]),filename)
        return {"url":self.base+"/media/"+key,"aeskey":base64.b64encode(secret).decode().rstrip("=")}

    def handle_error(self,request,address):
        import sys
        self.errors.append(str(sys.exc_info()[1]))


class Handler(BaseHTTPRequestHandler):
    protocol_version="HTTP/1.1"
    def log_message(self,*args): pass

    def do_GET(self):
        s=self.server
        if self.path.startswith("/media/"):
            key=self.path.split("/")[-1]
            data,name=s.files[key]
            s.read_media.append(key)
            self.send_response(200)
            self.send_header("Content-Disposition",'attachment; filename="'+name+'"')
            self.send_header("Content-Length",str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        assert self.path=="/" and self.headers["Upgrade"].lower()=="websocket"
        key=self.headers["Sec-WebSocket-Key"]
        accept=base64.b64encode(hashlib.sha1((key+"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.send_response(101)
        for k,v in [("Upgrade","websocket"),("Connection","Upgrade"),("Sec-WebSocket-Accept",accept)]:
            self.send_header(k,v)
        self.end_headers()
        self.close_connection=True
        def write(v):
            data=json.dumps(v).encode()
            length=len(data)
            head=bytes([129,length]) if length<126 else bytes([129,126])+struct.pack("!H",length) if length<65536 else bytes([129,127])+struct.pack("!Q",length)
            self.wfile.write(head+data)
        def read():
            head=self.rfile.read(2)
            if len(head)!=2 or head[0]&15==8:
                return None
            n=head[1]&127
            if n==126:n=struct.unpack("!H",self.rfile.read(2))[0]
            if n==127:n=struct.unpack("!Q",self.rfile.read(8))[0]
            assert head[1]&128 and n<=2*1024*1024
            mask=self.rfile.read(4)
            return json.loads(bytes(b^mask[i%4] for i,b in enumerate(self.rfile.read(n))))
        try:
            auth=read()
            assert auth["cmd"]=="aibot_subscribe"
            creds=auth["body"]
            s.auth.append(creds["bot_id"])
            valid=creds=={"bot_id":"wecom-test","secret":"isolated-wecom-test-secret"} and not s.revoked
            write({"headers":auth["headers"],"errcode":0 if valid else 40014,"errmsg":"ok" if valid else "invalid"})
            if not valid:return
            with s.lock:
                incoming,s.events=s.events,[]
            for frame in incoming:
                write(frame)
            while (frame:=read()) is not None:
                cmd,req,body=frame["cmd"],frame["headers"]["req_id"],frame.get("body",{})
                result={}
                if cmd=="ping":pass
                elif cmd=="aibot_upload_media_init":
                    assert body["type"]=="file" and body["total_size"]>0
                    uid=f"upload-{len(s.uploads)}"
                    s.uploads[uid]={"meta":body,"chunks":{}}
                    result={"upload_id":uid}
                elif cmd=="aibot_upload_media_chunk":
                    s.uploads[body["upload_id"]]["chunks"][body["chunk_index"]]=base64.b64decode(body["base64_data"],validate=True)
                elif cmd=="aibot_upload_media_finish":
                    upload=s.uploads[body["upload_id"]]
                    chunks=upload["chunks"]
                    data=b"".join(chunks[i] for i in range(upload["meta"]["total_chunks"]))
                    assert hashlib.md5(data).hexdigest()==upload["meta"]["md5"] and len(data)==upload["meta"]["total_size"]
                    upload["bytes"]=data
                    result={"type":"file","media_id":"media-"+body["upload_id"]}
                else:
                    assert cmd in ("aibot_send_msg","aibot_respond_msg")
                    if cmd=="aibot_respond_msg":assert req.startswith("incoming-") and "chatid" not in body
                    else:assert body["chatid"] in ("alice-ww","group-ww")
                    s.replies.append((cmd,req,body))
                    if s.lose_ack:
                        s.lose_ack=False
                        return
                write({"headers":{"req_id":req},"errcode":0,"errmsg":"ok","body":result})
        except (BrokenPipeError,ConnectionResetError):
            pass

if __name__=="__main__":
    with WeCom() as f:
        print(f.base,flush=True)
        f.serve_forever()
