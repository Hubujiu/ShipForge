"""Verify the repository UI protocol against disposable official-shaped HTTP providers."""
import argparse
from contextlib import ExitStack
from hashlib import sha256
import hmac
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import parse_qs, urlencode, urlsplit
from urllib.request import Request, build_opener, ProxyHandler
from urllib.error import HTTPError
from uuid import uuid4
from forge_fixture import Forge, app_key, stamp

team, release = import_module("verify-team"), import_module("verify-release")


def verify(bin_dir):
    server=release.binary(bin_dir,"shipforge-server")
    cli=release.binary(bin_dir,"shipforge")
    with ExitStack() as ctx:
        root=Path(ctx.enter_context(tempfile.TemporaryDirectory(prefix="shipforge-forge-")))
        clouds={p:ctx.enter_context(Forge(p)).start() for p in ("github","gitlab","gitlab-self")}
        for cloud in clouds.values():
            ctx.callback(cloud.shutdown)
        signing_key=root/"app.pem"
        clouds["github"].jwt_key=app_key(signing_key)
        env=dict(os.environ,SHIPFORGE_FORGE_GITHUB_LOOPBACK=clouds["github"].base,SHIPFORGE_FORGE_GITLAB_LOOPBACK=clouds["gitlab"].base,
                 SHIPFORGE_FORGE_SELF_LOOPBACK=clouds["gitlab-self"].base,SHIPFORGE_GITHUB_PRIVATE_KEY_FILE=str(signing_key),
                 SHIPFORGE_GITHUB_APP_ID="88",SHIPFORGE_GITHUB_APP_SLUG="shipforge-test",SHIPFORGE_GITHUB_WEBHOOK_SECRET="fixture-app-signing")
        callback="http://127.0.0.1:8765/consent"
        for kind in ("GITHUB","GITLAB"):
            env.update({f"SHIPFORGE_{kind}_CLIENT_ID":"fixture-client",f"SHIPFORGE_{kind}_CLIENT_SECRET":"fixture-secret",f"SHIPFORGE_{kind}_REDIRECT_URI":callback})
        tokens={actor:json.loads(subprocess.check_output([server,"--data-dir",root/"team","token","issue","--scope","write","--actor",actor],encoding="utf-8"))["token"] for actor in ("owner","alice","bob")}
        proc,address=team.start(server,root/"team","127.0.0.1:0",env=env)
        ctx.callback(team.stop,proc)
        opener=build_opener(ProxyHandler({}))
        def post(path,payload,headers):
            raw=json.dumps(payload,separators=(",",":"),ensure_ascii=False).encode()
            request=Request("http://"+address+path,raw,{"Content-Type":"application/json",**headers})
            try:
                response=opener.open(request,timeout=20)
            except HTTPError as e:
                response=e
            with response:
                return response.status,json.load(response)
        def call(op,actor="owner",ok=True,**params):
            status,data=post("/v1/command",{"op":op,**params},{"Authorization":"Bearer "+tokens[actor]})
            assert (status==200)==ok,(op,params.get("id"),data)
            return data["data"] if ok else data["error"]
        for actor in ("alice","bob"):
            call("create",kind="member",id=actor,title=actor,fields={"role":"member"})
        space=call("get",kind="space",id="default")
        call("update",kind="space",id="default",expected_rev=space["rev"],fields={"triage_enabled":True})
        team.connect(cli,root/"profile",address,tokens["owner"])
        assert release.run_cli(cli,root/"profile","request",request={"op":"forge.status","id":"github"})["account"] is None
        def connect(f,purpose,login,actor="owner",expect=True,installation=77):
            p=f.platform
            if p=="gitlab-self":
                return call("forge.credential",actor,expect,id=p,purpose=purpose,instance="https://gitlab.example.test",token=login)
            auth=call("forge.authorize",actor,id=p,purpose=purpose)
            qs=parse_qs(urlsplit(auth["authorize_url"]).query)
            code=str(uuid4())
            f.codes[code]=(login,qs["code_challenge"][0])
            result=callback+"?"+urlencode({"code":code,"state":qs["state"][0]})
            wrong=call("forge.authorize",actor,False,id=p,purpose=purpose,callback=result.replace("state=","state=x"),installation=installation)
            assert wrong["code"]=="forbidden",wrong
            return call("forge.authorize",actor,expect,id=p,purpose=purpose,callback=result,installation=installation)
        def hook(f,route,secret,event,body,delivery=None):
            eid=delivery or str(uuid4())
            if f.gh:
                raw=json.dumps(body,separators=(",",":"),ensure_ascii=False).encode()
                headers={"X-GitHub-Delivery":eid,"X-GitHub-Event":event,"X-Hub-Signature-256":"sha256="+hmac.new(secret.encode(),raw,sha256).hexdigest()}
            else:
                headers={"X-GitLab-Token":secret,"X-GitLab-Event-UUID":eid}
            return post(route,body,headers)
        for p,f in clouds.items():
            gh=f.gh
            assert call("forge.pair","alice",False,id=p,direction="incoming",repository_id=42)["code"]=="forbidden"
            if gh:
                assert connect(f,"organization","alice",expect=False)["code"]=="forbidden"
                assert connect(f,"organization","admin",expect=False,installation=96)["code"]=="forbidden"
            if p=="gitlab-self":
                assert connect(f,"organization","bad-scope",expect=False)["code"]=="forbidden"
            assert connect(f,"organization","admin")["connected"]
            assert connect(f,"personal","alice","alice")["connected"]
            assert connect(f,"personal","alice","bob",False)["code"]=="conflict"
            assert call("forge.status","bob",id=p)["personal"] is None
            assert not any("credential" in r for r in [call("forge.status","alice",id=p)["account"],call("forge.status","alice",id=p)["personal"]])
            assert sorted(x["repository_id"] for x in call("forge.repositories",id=p))==["42","43"]
            assert call("forge.pair",ok=False,id=p,direction="incoming",repository_id=99)["code"]=="forbidden"
            assert call("forge.pair",ok=False,id=p,direction="both",repository_id=43)["code"]=="conflict"
            pair=call("forge.pair",id=p,direction="both",repository_id=42)
            b,route=pair["id"],pair["webhook_path"]
            assert pair["backlinks"] is True
            secret=env["SHIPFORGE_GITHUB_WEBHOOK_SECRET"] if gh else pair["webhook_token"]
            assert call("forge.pair",ok=False,id=p,direction="both",repository_id=43,confirm_public=True)["code"]=="conflict"
            assert not call("forge.pair",id=p,direction="incoming",repository_id=43)["backlinks"]
            assert call("delete",ok=False,kind="connector",id=b)["code"]=="forbidden"
            event="issues" if gh else "issue"
            for number,delay in [(7,-3600),(8,0)]:
                f.issue(number,delay)
                if number==8:
                    f.issues[8]["assignees"]=[{"id":12,"login":"alice"},{"id":199,"login":"external"}]
                payload=f.event(number,"opened" if gh else "open")
                assert hook(f,route,"wrong",event,payload)[0]==401
                code,body=hook(f,route,secret,event,payload,"issue-"+str(number))
                assert code==200,(p,code,body)
                assert hook(f,route,secret,event,payload,"issue-"+str(number))[0]==200
            rows=[r for r in call("list",kind="task") if r["fields"].get("connector")==b]
            assert len(rows)==1 and rows[0]["title"]==f.issues[8]["title"],rows
            t=rows[0]["id"];args=dict(id=p,pair=b,task_id=t)
            assert rows[0]["fields"]["status"]=="triage"
            assert call("forge.thread","bob",id=p,task_id=t)[0]["author"]["member"]=="alice"
            assert rows[0]["fields"]["assignees"]==["alice"] and rows[0]["fields"]["source_assignees"]==["199"]
            assert len(f.notes[8])==1 and "shipforge://task/" in f.notes[8][0]["body"],f.notes
            assert call("connector.ingest",ok=False,id=b,external_id="22",event_id="bypass",source_version="1",title="foreign")["code"]=="forbidden"
            before=len(f.writes)
            call("create","alice",kind="comment",title="local discussion",fields={"task":t,"body":"not sent"})
            assert len(f.writes)==before
            issue=f.issues[8]
            def send_note(action,text):
                n={"id":11,"body":text,"note":text,"noteable_type":"Issue","system":False,"internal":False,"created_at":stamp(),"updated_at":stamp(),"action":action}
                user={"id":12,"login":"alice","username":"alice"}
                if gh: payload={"repository":f.rep(42),"installation":{"id":77},"action":action,"issue":issue,"comment":dict(n,user=user)}
                else: payload={"project":f.rep(42),"object_kind":"note","issue":issue,"object_attributes":n,"user":user}
                code,body=hook(f,route,secret,"issue_comment" if gh else "note",payload)
                assert code==200,body
            send_note("created" if gh else "create","a source comment")
            check=call("forge.thread","bob",id=p,task_id=t)[0]
            assert check["comments"][0]["member"]=="alice" and check["comments"][0]["body"]=="a source comment",check
            assert call("forge.reply","alice",**args,body="personal",request_id="once")["identity"]=="personal"
            assert call("forge.reply","alice",**args,body="personal",request_id="once")["state"]=="sent"
            assert sum(1 for v in f.writes if v[2].get("body","").startswith("personal\n"))==1
            assert call("forge.reply","alice",False,**args,body="changed",request_id="once")["code"]=="conflict"
            f.drop_once=True
            call("forge.reply","alice",False,**args,body="lost response",request_id="lost")
            assert call("forge.reply","alice",**args,body="lost response",request_id="lost")["state"]=="sent"
            assert sum(1 for v in f.writes if v[2].get("body","").startswith("lost response\n"))==1
            assert connect(f,"personal","bob","bob")["connected"]
            local=call("get",kind="task",id=t)
            call("update","bob",kind="task",id=t,expected_rev=local["rev"],fields={"assignees":["bob"]})
            call("forge.push","bob",**args)
            assert sorted(x["id"] for x in f.issues[8]["assignees"])==[13,199]
            assert f.writes[-1][0]=="bob"
            # GitHub timestamps can be rounded to one second. A stale snapshot must not overwrite a different payload in that second.
            f.issues[8]["title"]="new source title"
            assert call("forge.push","alice",False,**args)["code"]=="conflict"
            call("forge.refresh","alice",**args)
            f.revoked.add("alice")
            assert call("forge.reply","alice",**args,body="fallback",request_id="again")["fallback"] is True
            assert f.writes[-1][0]==("app-bot" if gh else "admin")
            item=call("get",kind="task",id=t)
            call("update","bob",kind="task",id=t,expected_rev=item["rev"],title="local changed")
            f.issues[8]["updated_at"]=stamp()
            assert call("forge.push","bob",False,**args)["code"]=="conflict"
            call("forge.refresh","bob",**args)
            call("forge.push","bob",**args)
            assert f.issues[8]["title"]=="local changed"
            for action,state,local in [("closed" if gh else "close","closed","done"),("reopened" if gh else "reopen","open" if gh else "opened","todo")]:
                f.issues[8].update(state=state,updated_at=stamp())
                assert hook(f,route,secret,event,f.event(8,action))[0]==200
                assert call("get",kind="task",id=t)["fields"]["status"]==local
            if not gh:
                f.issues[8]["confidential"]=True
                count=len(f.writes)
                assert call("forge.reply","bob",False,**args,body="private",request_id="blocked-private")["code"]=="forbidden"
                assert len(f.writes)==count
                f.issues[8]["confidential"]=False
            call("forge.pair",id=p,repository_id=42,direction="incoming")
            assert call("forge.reply","bob",False,**args,body="blocked",request_id="no")["code"]=="forbidden"
            assert call("forge.internal","bob",False,id=p,action="config")["code"]=="forbidden"
            call("forge.unlink","bob",**args)
            assert call("forge.thread","alice",id=p,task_id=t)[0]["active"] is False
            assert hook(f,route,secret,event,f.event(8,"edited" if gh else "update"))[0]==200
            if gh:
                notice={"action":"removed","installation":{"id":77},"repositories_removed":[{"id":42}],"sender":{"id":11}}
                assert hook(f,route,secret,"installation_repositories",notice)[0]==200
                assert all(r["repository_id"]!="42" for r in call("forge.status",id=p)["pairs"])
            else:
                f.revoked.add("admin")
                assert hook(f,route,secret,event,f.event(8,"update"))[0]!=200
            call("forge.disconnect",id=p,purpose="organization")
            assert call("forge.status","alice",id=p)["personal"] is None
            assert call("get",kind="task",id=t)["id"]==t
            assert not call("forge.status",id=p)["pairs"]
        for key in ("gitlab","gitlab-self"):
            clouds[key].revoked.clear()
            connect(clouds[key],"organization","admin")
        connect(clouds["gitlab"],"personal","alice","alice")
        changed=connect(clouds["gitlab-self"],"personal","alice","alice")
        assert changed["replaced_platform"]=="gitlab"
        assert call("forge.status","alice",id="gitlab")["personal"] is None
        assert call("forge.status","alice",id="gitlab-self")["personal"]["login"]=="alice"
        print("Repository OAuth, App JWT, self-managed PAT, three identities, issue events and explicit replies passed.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("bin_dir",type=Path)
    verify(parser.parse_args().bin_dir.resolve())
