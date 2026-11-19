"""Loopback GitHub/GitLab contracts. Generates a disposable RSA App key with Node."""
import base64
from datetime import datetime, timezone, timedelta
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import subprocess
import threading
from urllib.parse import urlsplit


def stamp(offset=0):
    return (datetime.now(timezone.utc) + timedelta(seconds=offset)).isoformat()


def app_key(path):
    script = """const c=require('crypto'),fs=require('fs');
const {publicKey,privateKey}=c.generateKeyPairSync('rsa',{modulusLength:2048});
fs.writeFileSync(process.argv[1],privateKey.export({type:'pkcs1',format:'pem'}));
console.log(JSON.stringify(publicKey.export({format:'jwk'})));"""
    key=json.loads(subprocess.check_output(["node", "-e", script, str(path)], encoding="utf-8"))
    return tuple(int.from_bytes(unb64(key[k]), "big") for k in ("n","e"))


def unb64(value):
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


class Forge(ThreadingHTTPServer):
    def __init__(self,platform):
        super().__init__(("127.0.0.1",0),Handler)
        self.platform=platform
        self.gh=platform=="github"
        self.base=f"http://127.0.0.1:{self.server_port}"
        self.repos={42:("qa/demo",False),43:("qa/public",True)}
        self.issues={}
        self.notes={}
        self.members={"admin":(11,"maintainer"),"alice":(12,"alice"),"bob":(13,"bob")}
        self.revoked=set()
        self.codes={}
        self.jwt_key=None
        self.writes=[]
        self.drop_once=False

    def start(self):
        threading.Thread(target=self.serve_forever,daemon=True).start()
        return self

    def verify_jwt(self,token):
        a,b,sig=token.split(".")
        n,e=self.jwt_key
        signed=pow(int.from_bytes(unb64(sig),"big"),e,n).to_bytes(256,"big")
        digest=bytes.fromhex("3031300d060960864801650304020105000420")+hashlib.sha256((a+"."+b).encode()).digest()
        return signed==b"\x00\x01"+b"\xff"*(253-len(digest))+b"\x00"+digest and json.loads(unb64(b))["iss"]=="88"

    def rep(self,k):
        name,public=self.repos[k]
        if self.gh:
            return {"id":k,"full_name":name,"private":not public}
        return {"id":k,"path_with_namespace":name,"visibility":"public" if public else "private","permissions":{"project_access":{"access_level":40},"group_access":None}}

    def issue(self,number,offset=0):
        t=stamp(offset)
        row={"id":300+number,"number":number,"iid":number,"project_id":42,"title":f"{self.platform} issue {number}",
             "body":"source body","description":"source body","state":"open" if self.gh else "opened","created_at":t,"updated_at":t,"assignees":[],"author_id":12,"author":{"id":12,"username":"alice"},"user":{"id":12,"login":"alice"}}
        self.issues[number]=row
        return row

    def event(self,number,action):
        issue=self.issues[number].copy()
        if self.gh:
            return {"repository":self.rep(42),"installation":{"id":77},"action":action,"issue":issue}
        issue["action"]=action
        return {"project":self.rep(42),"object_kind":"issue","object_attributes":issue}


class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args): pass

    def answer(self,code,v=None):
        self.send_response(code)
        self.send_header("Content-Type","application/json")
        self.end_headers()
        self.wfile.write(json.dumps(v or {}).encode())

    def route(self):
        f=self.server
        path=urlsplit(self.path).path
        token=self.headers.get("Authorization","").removeprefix("Bearer ")
        body=json.loads(self.rfile.read(int(self.headers.get("Content-Length",0))) or "{}")
        if path in ("/login/oauth/access_token","/oauth/token"):
            login,challenge=f.codes.pop(body.get("code",""),(None,None))
            proof=base64.urlsafe_b64encode(hashlib.sha256(body.get("code_verifier","").encode()).digest()).decode().rstrip("=")
            return self.answer(200,{"access_token":login,"scope":"api","expires_in":7200}) if login and proof==challenge else self.answer(403)
        if path=="/app/installations/77/access_tokens" and self.command=="POST":
            return self.answer(201,{"token":"app-bot"}) if f.verify_jwt(token) else self.answer(401)
        path=path.removeprefix("/api/v4")
        if token in f.revoked or token not in {*f.members,"app-bot","bad-scope"}:
            return self.answer(401)
        if path=="/user":
            n,name=f.members.get(token,(99,"limited"))
            return self.answer(200,{"id":n,"login":name,"username":name})
        if path=="/personal_access_tokens/self":
            return self.answer(200,{"user_id":f.members.get(token,(99,""))[0],"active":True,"revoked":False,"scopes":["api"] if token!="bad-scope" else ["read_api"],"expires_at":"2030-01-01"})
        if path=="/user/installations":
            return self.answer(200,{"installations":[{"id":77,"app_id":88,"account":{"id":555,"login":"qa","type":"Organization"}}]})
        if path=="/user/memberships/orgs/qa":
            return self.answer(200,{"state":"active","role":"admin" if token=="admin" else "member"})
        if path=="/installation/repositories":
            return self.answer(200,{"repositories":[f.rep(k) for k in f.repos]}) if token=="app-bot" else self.answer(403)
        if path=="/projects":
            assert "min_access_level=40" in self.path
            return self.answer(200,[f.rep(k) for k in f.repos]+[dict(f.rep(42),id=99,permissions={"project_access":{"access_level":20}})])
        root="/repos/qa/demo" if f.gh else "/projects/42"
        if path==root:
            return self.answer(200,f.rep(42)) if 42 in f.repos else self.answer(404)
        if not path.startswith(root+"/issues/") or 42 not in f.repos:
            return self.answer(403)
        parts=path[len(root+"/issues/"):].split("/")
        number=int(parts[0])
        issue=f.issues.get(number)
        if not issue:
            return self.answer(404)
        if len(parts)==1:
            if self.command in ("PATCH","PUT"):
                issue.update({k:v for k,v in body.items() if k not in ("state_event","assignees","assignee_ids")})
                if "assignees" in body or "assignee_ids" in body:
                    keys=body.get("assignees",body.get("assignee_ids"))
                    people=[*f.members.values(),(199,"external")]
                    issue["assignees"]=[{"id":n,"login":name,"username":name} for n,name in people if (name if f.gh else n) in keys]
                if "state_event" in body:
                    issue["state"]="closed" if body["state_event"]=="close" else "opened"
                issue["updated_at"]=stamp()
                f.writes.append((token,number,body))
            return self.answer(200,issue)
        assert parts[1:]==(["comments"] if f.gh else ["notes"])
        rows=f.notes.setdefault(number,[])
        if self.command=="POST":
            n,user=f.members.get(token,(300,"shipforge-test[bot]"))
            who={"id":n,"login":user,"username":user,"type":"Bot" if token=="app-bot" else "User"}
            row={"id":800+len(rows),"body":body["body"],"user":who,"author":who,"updated_at":stamp()}
            rows.append(row)
            f.writes.append((token,number,body))
            if f.drop_once:
                f.drop_once=False
                return self.answer(503)
            return self.answer(201,row)
        self.send_response(200)
        self.end_headers()
        self.wfile.write(json.dumps(rows).encode())

    do_GET=do_POST=do_PUT=do_PATCH=route
