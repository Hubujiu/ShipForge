"""Verify TAPD company and personal API contracts against a disposable HTTP upstream."""
import argparse
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from urllib.parse import urlsplit, parse_qs

release = import_module("verify-release")
WORKSPACE = "20003270"
OTHER = "20003271"
ENTRY = "1120003270001000042"
STAMP = "2026-03-26 14:20:31"


class Tapd(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, data):
        raw=json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Length",str(len(raw)))
        self.send_header("Content-Type","application/json")
        if status==429:
            self.send_header("Retry-After","0")
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        req=urlsplit(self.path)
        args=parse_qs(req.query)
        basic=self.headers.get("Authorization")=="Basic "+base64.b64encode(b"company:passQA").decode()
        personal=self.headers.get("Authorization")=="Bearer tapd-personal"
        if not (basic or personal):
            return self.reply(401,{"status":0})
        self.server.hits.append((req.path,args,basic))
        current=OTHER if basic else WORKSPACE
        if req.path=="/users/info" and personal:
            data={"nick":"test_person"}
        elif req.path=="/workspaces/projects" and basic and args=={"company_id":["600"],"category":["project"]}:
            data=[{"Workspace":{"id":OTHER,"name":"company project","status":"normal","category":"project"}}]
        elif req.path=="/workspaces/user_participant_projects" and personal and args=={"nick":["test_person"]}:
            data=[{"Workspace":{"id":WORKSPACE,"name":"ended project","status":"closed","category":"product"}},
                  {"Workspace":{"id":"600","name":"org","status":"normal","category":"organization"}}]
        elif req.path in {"/bugs","/stories"} and args.get("workspace_id")==[current]:
            if self.server.limited:
                self.server.limited-=1
                return self.reply(429,{"status":0})
            if self.server.invalid:
                return self.reply(200,{"status":0,"info":"fixture failure","data":[]})
            kind="Bug" if req.path=="/bugs" else "Story"
            rows=[{kind:dict(self.server.data[kind],workspace_id=current)}]
            if self.server.foreign and kind=="Bug":
                rows[0][kind]["workspace_id"]="700"
            if self.server.extra and kind=="Bug":
                rows.append({kind:dict(rows[0][kind],id="1120003270001000043",current_owner="other",title="Additional item")})
            if "modified" in args:
                assert args["modified"][0].startswith(">")
                rows=[row for row in rows if row[kind]["modified"]>args["modified"][0][1:]]
            assert args["order"]==["modified ASC"]
            size=int(args["limit"][0])
            start=(int(args["page"][0])-1)*size
            data=rows[start:start+size]
        else:
            return self.reply(403,{"status":0})
        self.reply(200,{"status":1,"info":"success","data":data})


def verify(bin_dir,start,stop):
    cli=release.binary(bin_dir,"shipforge")
    server=release.binary(bin_dir,"shipforge-server")
    upstream=ThreadingHTTPServer(("127.0.0.1",0),Tapd)
    upstream.data={
      "Story":{"id":ENTRY,"name":"Fixture requirement","modified":STAMP,"status":"developing","creator":"origin","priority_label":"高","owner":"local-nick;outside;","description":"old"},
      "Bug":{"id":ENTRY,"title":"Fixture defect","modified":STAMP,"status":"new","creator":"outside","priority_label":"普通","current_owner":"outside","description":"known"},
    }
    upstream.limited=0
    upstream.invalid=upstream.foreign=upstream.extra=False
    upstream.hits=[]
    thread=threading.Thread(target=upstream.serve_forever,daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="shipforge-tapd-") as temp:
            root=Path(temp)
            data=root/"server"
            token=json.loads(subprocess.check_output([str(server),"--data-dir",str(data),"token","issue","--actor","owner","--scope","write"],encoding="utf-8"))["token"]
            clean={k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
            p,address=start(server,data,"127.0.0.1:0",env=clean)
            try:
                env=dict(clean,SHIPFORGE_SERVER="http://"+address,SHIPFORGE_TOKEN=token,SHIPFORGE_DATA_DIR=str(root/"client"),
                         SHIPFORGE_VENDOR_LOOPBACK=f"http://127.0.0.1:{upstream.server_port}")
                def call(op,*,ok=True,**arg):
                    response=subprocess.run([str(cli),"request"],input=json.dumps({"op":op,**arg}),text=True,encoding="utf-8",
                                  capture_output=True,timeout=15,env=env)
                    reply=json.loads(response.stdout)
                    assert reply["ok"]==ok,(op,reply,response.stderr)
                    return reply["data"] if ok else reply["error"]
                def config(name,auth,workspace,ok=True,**extra):
                    return call("create",kind="connector",id=name,title=name,ok=ok,fields={"provider":"tapd","tapd_auth":auth,
                             "enabled":True,"tapd_workspace":workspace,**extra})
                config("bad","personal",WORKSPACE,ok=False,company_id="600")
                config("bad","personal",WORKSPACE,ok=False,realtime=True)
                config("bad","company",WORKSPACE,ok=False)
                call("create",kind="member",id="alice",title="Alice",fields={"role":"member"})
                config("person","personal",WORKSPACE,assignee_map={"local-nick":"alice"})
                config("corp","company",OTHER,company_id="600")
                call("tapd.credential",id="person",access_token="tapd-personal")
                call("tapd.credential",id="corp",api_user="company",api_password="passQA")
                assert call("tapd.projects",id="person")["projects"]==[{"id":WORKSPACE,"name":"ended project","status":"closed","ended":True}]
                assert call("tapd.projects",id="corp")["projects"][0]["id"]==OTHER
                assert call("tapd.preview",id="corp",page_size=1)["counts"]["created"]==2
                space=call("get",kind="space",id="default")
                call("update",kind="space",id="default",expected_rev=space["rev"],fields={"triage_enabled":True})
                upstream.invalid=True
                call("tapd.preview",id="person",ok=False)
                upstream.invalid=False
                upstream.foreign=True
                assert call("tapd.preview",id="person",ok=False)["code"]=="forbidden"
                upstream.foreign=False
                assert not call("imports.history",id="person")["items"]
                upstream.limited=1
                preview=call("tapd.preview",id="person",page_size=1)
                assert preview["counts"]["created"]==2 and not call("list",kind="task")
                assert any(p=="/stories" and q.get("page")==["2"] for p,q,_ in upstream.hits)
                call("imports.commit",id="person",batch=preview["batch"])
                old={r["fields"]["external_id"]:r for r in call("list",kind="task")}
                story=old["story:"+ENTRY]
                defect=old["bug:"+ENTRY]
                assert story["fields"]["type"]=="requirement" and story["fields"]["status"]=="in_progress"
                assert story["fields"]["priority"]=="high" and story["fields"]["source_assignees"]==["outside"] and story["fields"]["assignees"]==["alice"]
                assert defect["fields"]["type"]=="bug" and defect["fields"]["status"]=="todo" and story["id"]!=defect["id"]
                upstream.data["Story"]["description"]="Edited at TAPD"
                duplicate=call("tapd.preview",id="person")
                assert duplicate["counts"]["created"]==0 and duplicate["counts"]["unchanged"]==2
                call("imports.commit",id="person",batch=duplicate["batch"])
                assert call("get",kind="task",id=story["id"])["fields"]["description"]=="old"
                upstream.extra=True
                changed=call("tapd.preview",id="person",import_mode="changes",page_size=1)
                assert changed["modified_after"]=="2026-03-26 14:20:30"
                assert changed["counts"]["created"]==1 and changed["counts"]["updated"]==1,changed
                call("imports.commit",id="person",batch=changed["batch"])
                current=call("list",kind="task")
                assert next(t for t in current if t["id"]==story["id"])["fields"]["description"]=="Edited at TAPD"
                assert next(t for t in current if t["title"]=="Additional item")["fields"]["status"]=="triage"
                assert call("imports.rollback",id="person",batch=changed["batch"])["deleted"]==1
                assert call("imports.rollback",id="person",batch=preview["batch"])["deleted"]==2
                assert call("tapd.credential",id="person",clear=True)=={"saved":False}
                call("tapd.projects",id="person",ok=False)
                assert not call("list",kind="task")
            finally:
                stop(p)
    finally:
        upstream.shutdown()
        upstream.server_close()
        thread.join(timeout=3)
    print("Verified both TAPD credential modes, authenticated discovery, Story/Bug pagination, throttling, scoped incremental detection and rollback via local HTTP.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    team=import_module("verify-team")
    verify(parser.parse_args().directory.resolve(),team.start,team.stop)
