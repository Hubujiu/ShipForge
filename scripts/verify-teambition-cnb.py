"""Run the shipped CLI, server and worker against two isolated vendor HTTP replicas."""
import argparse
import copy
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlsplit
from urllib.request import Request, urlopen


class Vendor(BaseHTTPRequestHandler):
    def log_message(self,*_):
        pass

    def reply(self,status,data):
        raw=json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type","application/json")
        self.send_header("Content-Length",str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def handle_request(self):
        site=self.server
        u=urlsplit(self.path)
        p=u.path
        q={k:v[0] for k,v in parse_qs(u.query).items()}
        raw=self.rfile.read(int(self.headers.get("Content-Length","0")))
        body=json.loads(raw or "{}")
        auth=self.headers.get("Authorization")
        actor=self.headers.get("X-Operator-Id")
        site.calls.append((self.command,p,actor,q))
        if p.startswith("/api/"):
            def tb(value,**rest):
                self.reply(200,dict(code=0,result=value,**rest))
            def page(rows):
                index=int(q.get("pageToken","0"))
                size=int(q["pageSize"])
                tb(rows[index:index+size],nextPageToken=str(index+size) if index+size<len(rows) else "")
            if p=="/api/appToken" and body=={"appId":"test-app","appSecret":"tb-app-key"}:return tb({"appToken":"app","expire":3600})
            if auth!="Bearer app" or self.headers.get("X-Tenant-Id")!="tenant":return self.reply(403,{})
            if p=="/api/org/info":return tb({"orgId":"tenant"})
            if p=="/api/oauth/userAccessToken" and body=={"code":"fixture-code","grantType":"authorization_code"}:return tb({"accessToken":"user-worker","userId":"external-member"})
            if p=="/api/oauth/userInfo":
                users={"user-owner":"external-owner","user-worker":"external-member","user-no-access":"other"}
                who=users.get(body.get("userAccessToken"))
                return tb({"userId":who}) if who else self.reply(401,{})
            if p=="/api/v3/project/user-joined":
                return page(["project","other-project"] if actor in ("external-member","external-owner") else [])
            if p=="/api/v3/project/query":
                ids=q.get("projectIds","").split(",")
                return page([dict(id=i,name="Project "+i,organizationId="tenant") for i in ids])
            if p=="/api/v3/project/project/taskflowstatus/search":
                return page([{"id":"open","name":"进行中"},{"id":"complete","name":"已完成"}])
            if p=="/api/v3/project/project/task/query":
                if actor is not None or q.get("includeArchived")!="false":return self.reply(403,{})
                rows=[copy.deepcopy(i) for i in site.tasks.values()]
                if site.foreign:rows[0]["projectId"]="foreign"
                return page(rows)
            if p=="/api/v3/task/query":
                task=site.tasks.get(q.get("taskId"))
                return tb([task] if task else [])
            if p.startswith("/api/v3/task/") and self.command in ("PUT","POST"):
                if actor!="external-member":return self.reply(403,{})
                _,_,_,_,key,cmd=p.split("/")
                row=site.tasks[key]
                if cmd=="delete" and self.command=="POST":del site.tasks[key]
                elif cmd=="taskflowstatus" and self.command=="PUT":row["tfsId"]=body["taskflowstatusId"];row["isDone"]=True
                elif cmd in ("content","note") and self.command=="PUT":row[cmd]=body[cmd]
                else:return self.reply(400,{})
                site.writes.append((cmd,actor))
                return tb({})
        else:
            if auth not in ("Bearer cnb-group-key","Bearer cnb-rotated"):return self.reply(403,{})
            if p=="/user/groups":return self.reply(200,[{"id":1,"path":"group"}])
            if p=="/group/-/repos":
                repos=[{"id":88,"path":"group/sub/repo","name":"repo"},{"id":89,"path":"group/demo","name":"demo"}]
                idx=int(q.get("page",1))-1;size=int(q.get("page_size",100))
                if q.get("descendant")!="all":return self.reply(400,{})
                return self.reply(200,repos[idx*size:(idx+1)*size])
            if p=="/group/sub/repo":return self.reply(200,{"id":88,"path":"group/sub/repo"})
            tail=p.removeprefix("/group/sub/repo/-/")
            if tail.startswith("git/commits/"):return self.reply(200,{"sha":tail.split("/")[-1],"commit":{"message":"Implement "+site.number}})
            if tail=="pulls/7":return self.reply(200,{"number":7,"title":"Review","body":site.number,"head":{"ref":"feature/"+site.number,"sha":"deadbeef"}})
            if tail=="issues/3":return self.reply(200,{"number":3,"title":"Follow "+site.number,"body":"See "+site.private_number})
            if tail=="issues/4":return self.reply(200,{"number":4,"title":"Restricted "+site.number,"invisible":True})
            if tail in ("issues/3/comments","pulls/7/comments"):
                notes=site.notes.setdefault(tail,[])
                if self.command=="GET":
                    if site.fail_lookup:
                        site.fail_lookup=False
                        return self.reply(503,{})
                    return self.reply(200,notes)
                if self.command=="POST" and body.get("body","").startswith("["):
                    note={"id":str(100+len(notes)),"body":body["body"]}
                    notes.append(note)
                    if site.lose:
                        site.lose=False
                        return self.reply(503,{})
                    return self.reply(201,note)
            if tail.startswith("issues/3/comments/"):
                key=tail.split("/")[-1];notes=site.notes.get("issues/3/comments",[])
                match=next((note for note in notes if note["id"]==key),None)
                if match:return self.reply(200,match)
        self.reply(404,{"fixture":p})

    do_GET=do_PUT=do_POST=handle_request


def verify(directory,start,stop):
    release=import_module("verify-release")
    cli,srv,worker=[release.binary(directory,n) for n in ("shipforge","shipforge-server","shipforge-worker")]
    site=ThreadingHTTPServer(("127.0.0.1",0),Vendor)
    site.calls=[];site.writes=[];site.notes={};site.lose=False;site.fail_lookup=False;site.foreign=False
    site.tasks={k:dict(id=k,content="Task "+k,note="original",tfsId="open",projectId="project",isDone=False,
                       uniqueId="TB-"+k,executorId="external-member",sfcId="bug",visible="members",updated="2025-11-10T01:00:00Z")
                for k in ("one","two")}
    site.tasks["limited"]=dict(site.tasks["two"],id="limited",visible="involves",content="Private source title")
    site.number="";site.private_number=""
    thread=threading.Thread(target=site.serve_forever,daemon=True);thread.start()
    clean={k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
    try:
        with tempfile.TemporaryDirectory(prefix="shipforge-teambition-cnb-") as temp:
            root=Path(temp); data=root/"team";env_server=dict(clean,SHIPFORGE_VENDOR_LOOPBACK=f"http://127.0.0.1:{site.server_port}")
            def issue(actor,scope="write"):
                return json.loads(subprocess.check_output([srv,"--data-dir",data,"token","issue","--actor",actor,"--scope",scope],env=clean))["token"]
            tokens={name:issue(name) for name in ("owner","alice","mallory")};tokens["reader"]=issue("owner","read")
            server,address=start(srv,data,"127.0.0.1:0",env=env_server)
            base="http://"+address
            def env(actor):
                return dict(clean,SHIPFORGE_SERVER=base,SHIPFORGE_TOKEN=tokens[actor],SHIPFORGE_DATA_DIR=str(root/actor))
            def call(op,actor="owner",ok=True,**fields):
                p=subprocess.run([cli,"request"],input=json.dumps(dict(op=op,**fields)),env=env(actor),capture_output=True,text=True,encoding="utf8",timeout=35)
                response=json.loads(p.stdout)
                assert response["ok"]==ok,(op,actor,response)
                return response["data" if ok else "error"]
            def edit(kind,key,**fields):
                row=call("get",kind=kind,id=key)
                return call("update",kind=kind,id=key,expected_rev=row["rev"],fields=fields)
            def sync():
                p=subprocess.run([worker,"tracker-sync","--connector","tb"],capture_output=True,text=True,encoding="utf8",env=env("owner"),timeout=35)
                assert p.returncode==0,p.stderr
                return json.loads(p.stdout)
            def callback(secret,body,expect=200,stamp=None):
                raw=json.dumps(body,separators=(",",":")).encode()
                timestamp=str(stamp if stamp is not None else int(time.time()))
                sig=hmac.new(secret.encode(),timestamp.encode()+b"."+raw,hashlib.sha256).hexdigest()
                req=Request(base+"/v1/cnb/default/cnb",data=raw,headers={"Content-Type":"application/json","X-ShipForge-Timestamp":timestamp,"X-ShipForge-Signature":sig})
                try:res=urlopen(req,timeout=35)
                except HTTPError as e:res=e
                with res:code=res.status;answer=json.loads(res.read())
                assert code==expect,(body,code,answer)
                return answer
            try:
                call("create",kind="member",id="alice",title="Alice",fields={"role":"member"})
                call("create",kind="connector",id="tb",title="Teambition",fields={"provider":"teambition","enterprise_id":"tenant","remote_scope":"project","enabled":True,"assignee_map":{"external-member":"alice"},"mapping":{"status":{"open":"in_progress"}}})
                app=dict(id="tb",app_id="test-app",app_secret="tb-app-key")
                for actor in ("alice","reader","mallory"):assert call("teambition.application",actor,False,**app)["code"]=="forbidden"
                assert call("teambition.application",**app)["suffix"]=="-key"
                call("teambition.projects",id="tb",ok=False)
                call("teambition.synchronize",id="tb",ok=False)
                call("teambition.personal",id="tb",access_token="user-owner")
                assert len(call("teambition.projects",id="tb",page_size=1)["projects"])==2
                call("teambition.personal","alice",id="tb",access_token="user-no-access")
                assert not call("teambition.projects","alice",id="tb")["projects"]
                opts=call("teambition.authorize","alice",id="tb",redirect_uri="http://localhost/callback")
                state=parse_qs(urlsplit(opts["authorize_url"]).query)["state"][0]
                call("teambition.personal","alice",False,id="tb",code="fixture-code",state="wrong")
                call("teambition.personal","alice",id="tb",code="fixture-code",state=state)
                call("teambition.personal","alice",False,id="tb",code="fixture-code",state=state)
                assert len(call("teambition.projects","alice",id="tb")["projects"])==2
                site.foreign=True
                assert call("teambition.preview",id="tb",ok=False)["code"]=="forbidden"
                site.foreign=False
                pre=call("teambition.preview",id="tb",page_size=1)
                assert pre["counts"]["created"]==2 and pre["excluded_restricted_or_archived"]==1
                done=call("imports.commit",id="tb",batch=pre["batch"]);assert done["counts"]["created"]==2
                tasks={r["fields"]["source_key"]:r for r in call("list",kind="task") if r["fields"].get("connector")=="tb"}
                one=tasks["TB-one"]["id"]
                assert tasks["TB-one"]["fields"]["assignees"]==["alice"] and tasks["TB-one"]["fields"]["status"]=="in_progress"
                assert sync()["state"]=="unchanged" and not site.writes
                edit("task",one,status="done",description="local data")
                call("teambition.push","alice",False,id="tb",task_id=one,intent="fields")
                edit("connector","tb",vendor_write=True)
                call("teambition.push","alice",False,id="tb",task_id=one,intent="status")
                edit("connector","tb",mapping={"status":{"open":"in_progress","complete":"done"},"write_status":{"done":"已完成"}})
                site.tasks["one"]["visible"]="involves"
                assert call("teambition.push","alice",False,id="tb",task_id=one,intent="fields")["code"]=="forbidden" and not site.writes
                site.tasks["one"]["visible"]="members"
                assert call("teambition.push","alice",id="tb",task_id=one,intent="fields")["state"]=="sent"
                assert call("teambition.push","alice",id="tb",task_id=one,intent="status")["state"]=="sent"
                assert site.tasks["one"]["note"]=="local data" and site.tasks["one"]["tfsId"]=="complete"
                assert len(site.writes)==2 and all(actor=="external-member" for _,actor in site.writes)
                sync()
                rev=call("get",kind="task",id=one)["rev"]
                args=dict(id="tb",task_id=one,intent="delete",confirm_external="one",confirm_rev=rev)
                call("teambition.push","alice",False,**args)
                edit("connector","tb",vendor_delete=True)
                call("teambition.push","alice",False,**dict(args,confirm_external="different"))
                call("teambition.push","alice",False,**dict(args,confirm_rev=rev-1))
                assert call("teambition.push","alice",**args)["state"]=="sent"
                assert call("teambition.push","alice",**args)["state"]=="sent" and "one" not in site.tasks
                assert call("get",kind="task",id=one)["id"]==one
                call("imports.rollback",id="tb",batch=pre["batch"])
                assert sync()["state"]=="paused"
                call("teambition.personal","alice",id="tb",clear=True);call("teambition.projects","alice",False,id="tb")
                assert call("teambition.projects",id="tb")["projects"]

                public=call("create",kind="task",title="Public");private=call("create",kind="task",title="Private",fields={"visibility":"private"})
                site.number=public["fields"]["number"];site.private_number=private["fields"]["number"]
                call("create",kind="connector",id="cnb",title="CNB",fields={"provider":"cnb","cnb_group":"group","enabled":True,"cnb_backlinks":True})
                call("cnb.credential","reader",False,id="cnb",access_token="cnb-group-key")
                call("cnb.credential","alice",False,id="cnb",access_token="cnb-group-key")
                assert call("cnb.credential",id="cnb",access_token="cnb-group-key")["suffix"]=="-key"
                assert len(call("cnb.repositories",id="cnb",page_size=1)["repositories"])==2
                call("cnb.pair",id="cnb",repository="group2/repo",server_url=base,ok=False)
                pair=call("cnb.pair",id="cnb",repository="group/sub/repo",server_url=base)
                secret=pair["signing_key"];assert secret not in pair["yaml"] and "pull_request.target" in pair["yaml"]
                assert "signing_key" not in call("cnb.ci",id="cnb")
                commit=dict(repository="group/sub/repo",event="push",sha="a"*40,build="build-1")
                callback("wrong",commit,401);callback(secret,commit,401,int(time.time())-601);callback(secret,dict(commit,repository="another/repo"),403)
                callback(secret,commit);callback(secret,commit)
                callback(secret,dict(repository=commit["repository"],event="issue.open",issue="4",build="restricted-source"))
                assert not site.notes.get("issues/4/comments")
                pr=dict(repository=commit["repository"],event="pull_request.target",pull="7",build="build-2")
                callback(secret,pr);callback(secret,pr)
                issue_event=dict(repository=commit["repository"],event="issue.open",issue="3",build="build-3")
                site.lose=True;callback(secret,issue_event,400)
                site.fail_lookup=True;callback(secret,issue_event,400);callback(secret,issue_event)
                assert len(site.notes["issues/3/comments"])==len(site.notes["pulls/7/comments"])==1
                callback(secret,dict(issue_event,build="build-4",event="issue.comment",comment_id="100"))
                assert len(site.notes["issues/3/comments"])==1
                activity=call("cnb.activity",id="cnb");assert activity["latest_event"]=="build-4" and len(activity["replies"])==2
                rows=[r for r in call("list",kind="comment") if r["fields"].get("source")=="vcs"]
                assert len(rows)==3 and all(r["fields"]["task"]==public["id"] for r in rows)
                call("cnb.credential",id="cnb",clear=True);call("cnb.repositories",id="cnb",ok=False)
                call("cnb.credential",id="cnb",access_token="cnb-rotated")
                rotated=call("cnb.pair",id="cnb",repository=commit["repository"],server_url=base,rotate=True)["signing_key"]
                callback(secret,commit,401);callback(rotated,commit)
                call("cnb.internal.claim",id="cnb",ok=False)
                for profile in ("owner","alice"):
                    assert not (root/profile/"vendor-credentials").exists()
            finally:
                stop(server)
    finally:
        site.shutdown();site.server_close();thread.join(timeout=4)
    print("Verified hosted Teambition app and individual writes, CI HMAC, real CNB lookup, duplicate-safe backlinks and repository permissions.")


if __name__=="__main__":
    p=argparse.ArgumentParser();p.add_argument("directory",type=Path);team=import_module("verify-team")
    verify(p.parse_args().directory.resolve(),team.start,team.stop)
