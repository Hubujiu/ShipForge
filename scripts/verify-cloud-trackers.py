"""Exercise native Linear, ClickUp and ONES HTTP APIs, OAuth and batch ownership without a live tenant."""
import argparse
import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit

release = import_module("verify-release")
STAMP = 1735689600000


def linear(key, status="a"):
    return dict(id=key, identifier="QA-"+key, title="Linear "+key, description="original", priority=2,
                state={"id":status,"name":"Same label","type":"unstarted"},project={"id":"project"},
                assignee={"id":"external"}, creator={"id":"reporter"}, updatedAt="2025-01-01T00:00:00.000Z",
                url="https://linear.app/fixture/issue/"+key, parent=None,
                relations={"nodes":[],"pageInfo":{"hasNextPage":False,"endCursor":None}})


def clickup(key):
    return dict(id=key, name="ClickUp "+key, description="original", list={"id":"list"},status={"status":"ready for QA","type":"custom"},
                priority={"id":1,"priority":"urgent"}, assignees=[{"id":7},{"id":8}], creator={"id":9},
                date_updated=str(STAMP),custom_item_id=9, time_estimate=20000,custom_fields=[{"id":"field","value":"x"}],
                dependencies=[],linked_tasks=[],url="https://app.clickup.com/t/"+key)


def ones(key, kind, label):
    return dict(id=key,number=1,title="ONES "+key,descriptionText="source detail",
                issueType={"id":kind,"name":label},priority={"id":"h","name":"High"},project={"id":"project"},
                status={"id":"open","name":"Open","category":"to_do"},assignee={"id":"external"},creator={"id":"reporter"})


class Source(BaseHTTPRequestHandler):
    def log_message(self,*args):
        pass

    def reply(self, code, data):
        raw=json.dumps(data).encode()
        self.send_response(code)
        self.send_header("Content-Type","application/json")
        self.send_header("Content-Length",str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def cursor(self, rows, size, cursor, key="nodes"):
        offset=int(cursor[1:]) if cursor else 0
        page=rows[offset:offset+size]
        return {key:page,"pageInfo":{"hasNextPage":offset+size<len(rows),"endCursor":"c"+str(offset+size)}}

    def request(self):
        site=self.server
        path=urlsplit(self.path)
        query={k:v[0] for k,v in parse_qs(path.query).items()}
        route=path.path
        raw=self.rfile.read(int(self.headers.get("Content-Length",0)))
        body=(dict((k,v[0]) for k,v in parse_qs(raw.decode()).items()) if self.headers.get("Content-Type","").startswith("application/x-www-form-urlencoded") else json.loads(raw or "{}"))
        verb=self.command
        auth=self.headers.get("Authorization")
        if route=="/oauth/token":
            if body.get("grant_type")=="refresh_token" and body.get("refresh_token")=="linear-refresh":
                return self.reply(200,{"access_token":"linear-renewed","refresh_token":"next"})
            if body.get("grant_type")=="authorization_code" and body.get("code")=="linear-code" and body.get("client_id")=="client" and len(body.get("code_verifier",""))>=43 and body.get("redirect_uri")=="http://localhost/callback":
                return self.reply(200,{"access_token":"linear-user","refresh_token":"linear-refresh"})
            return self.reply(400,{})
        if route=="/api/v2/oauth/token":
            if verb=="POST" and body=={"client_id":"client","client_secret":"fixture","code":"clickup-code"}:
                return self.reply(200,{"access_token":"clickup-user"})
            return self.reply(400,{})
        if route=="/graphql":
            op=body.get("operationName")
            if auth not in ("linear-key","Bearer linear-renewed") and not(auth=="Bearer linear-user" and not site.expired):
                return self.reply(401,{})
            if verb!="POST" or not body.get("query") or op not in body["query"]: return self.reply(400,{})
            v=body.get("variables",{});site.calls.append(("linear",op,v))
            if op=="CurrentOrganization":
                payload={"organization":{"id":"org","name":"Synthetic org"}}
            elif op=="DiscoverProjects":
                payload={"projects":self.cursor([{"id":"project","name":"A"},{"id":"alt","name":"B"}],v["first"],v.get("after"))}
            elif op=="ReadIssues" and v["filter"].get("project")=={"id":{"eq":"project"}}:
                if site.bad_graph:return self.reply(200,{"data":{"issues":{}},"errors":[{"message":"partial"}]})
                rows=list(copy.deepcopy(site.linear).values())
                if site.foreign:rows[0]["project"]["id"]="foreign"
                payload={"issues":self.cursor(rows,v["first"],v.get("after"))}
            elif op=="ReadRelations":
                payload={"issue":{"id":v["id"],"project":{"id":"project"},"relations":{"nodes":[{"id":"edge-2","type":"related","relatedIssue":{"id":"unimported"}}],"pageInfo":{"hasNextPage":False,"endCursor":None}}}}
            elif op=="IssueDetails":
                payload={"issue":site.linear[v["id"]]}
            elif op=="SetIssueState":
                item=site.linear[v["id"]];item["state"]={"id":v["state"],"name":"Done","type":"completed"}
                site.writes["linear"]+=1
                if site.lost_status:
                    site.lost_status=False
                    return self.reply(503,{})
                payload={"issueUpdate":{"success":True,"issue":{"id":v["id"]}}}
            elif op=="PublishComment":
                cid="comment-"+str(len(site.comments["linear"])+1)
                site.comments["linear"][cid]=dict(id=cid,body=v["body"],issue={"id":v["id"]})
                payload={"commentCreate":{"success":True,"comment":{"id":cid}}}
                if site.lost_comment:site.lost_comment=False;return self.reply(503,{})
            elif op=="CheckComment":
                payload={"comment":site.comments["linear"].get(v["id"])}
            else:return self.reply(400,{})
            return self.reply(200,{"data":payload})
        if route.startswith("/api/v2/"):
            tail=route[len("/api/v2"):];site.calls.append(("clickup",tail,query))
            if auth not in ("clickup-key-rotated","clickup-user") and not (auth=="clickup-key" and not site.clickup_rotated):return self.reply(401,{})
            if tail=="/team":return self.reply(200,{"teams":[{"id":45,"name":"Home"},{"id":46,"name":"Another"}]})
            if tail=="/team/45/space":return self.reply(200,{"spaces":[{"id":"spaces","name":"R&D"}]})
            if tail=="/space/spaces/folder":return self.reply(200,{"folders":[{"id":"folders","name":"Delivery"}]})
            if tail=="/folder/folders/list":return self.reply(200,{"lists":[{"id":"list","name":"Backlog","space":{"id":"spaces"}}]})
            if tail=="/space/spaces/list":return self.reply(200,{"lists":[{"id":"solo","name":"Folderless","space":{"id":"spaces"}}]})
            if tail=="/list/list/task":
                if query.get("include_closed")!="true" or query.get("subtasks")!="true":return self.reply(400,{})
                records=list(copy.deepcopy(site.clickup).values())
                if "date_updated_gt" in query:records=[v for v in records if int(v["date_updated"])>int(query["date_updated_gt"])]
                if site.foreign:records[0]["list"]["id"]="outside"
                page=int(query["page"])
                return self.reply(200,{"tasks":records[page:page+1],"last_page":page+1>=len(records)})
            if tail=="/list/list":
                return self.reply(200,{"id":"list","statuses":[{"status":"ready for QA"},{"id":"closed","status":"all done"}]})
            if tail.startswith("/task/"):
                key=tail.split("/")[2]
                if key not in site.clickup:return self.reply(404,{})
                if tail.endswith("/comment") and verb=="POST" and body.get("notify_all") is False:
                    cid="comment-"+str(len(site.comments["clickup"])+1)
                    site.comments["clickup"][cid]={"id":cid,"comment_text":body["comment_text"],"task":key,"date":str(STAMP)}
                    if site.lost_comment:site.lost_comment=False;return self.reply(503,{})
                    return self.reply(200,{"id":cid})
                if tail.endswith("/comment") and verb=="GET":
                    return self.reply(200,{"comments":[v for v in site.comments["clickup"].values() if v["task"]==key and "start" not in query]})
                if verb=="GET":return self.reply(200,site.clickup[key])
                if verb=="PUT" and body=={"status":"all done"}:
                    site.clickup[key]["status"]={"id":"closed","status":"all done","type":"closed"};site.writes["clickup"]+=1
                    if site.lost_status:site.lost_status=False;return self.reply(503,{})
                    return self.reply(200,site.clickup[key])
            return self.reply(404,{})
        if not route.startswith("/openapi/v2/") or auth!="Bearer ones-key":return self.reply(401,{})
        tail=route[len("/openapi/v2"):]
        site.calls.append(("ones",tail,query))
        if tail=="/account/teams":data={"teams":[{"id":"team","name":"Regional"}]}
        elif query.get("teamID")!="team":return self.reply(403,{})
        elif tail=="/project/projects":
            data=self.cursor([{"id":"project","name":"Release","isArchive":False},{"id":"old","name":"Archive","isArchive":True}],int(query["limit"]),query.get("cursor"),"list")
        elif tail=="/project/issueTypes":
            data=self.cursor([{"id":"bugs","name":"缺陷"},{"id":"stories","name":"需求"}],int(query["limit"]),query.get("cursor"),"list")
        elif tail=="/project/issues" and query.get("projectID")=="project" and query.get("issueTypeID"):
            rows=[copy.deepcopy(v) for v in site.ones.values() if v["issueType"]["id"]==query["issueTypeID"]]
            for row in rows:row.pop("descriptionText")
            if site.foreign and rows:rows[0]["project"]["id"]="foreign"
            data=self.cursor(rows,int(query["limit"]),query.get("cursor"),"list")
        elif tail.startswith("/project/issues/"):
            key=tail.split("/")[3]
            if key not in site.ones:return self.reply(404,{})
            if tail.endswith("/comments"):
                if verb=="POST" and len(body.get("text",""))<=128:
                    cid="comment-"+str(len(site.comments["ones"])+1)
                    site.comments["ones"][cid]={"id":cid,"text":body["text"],"issue":key}
                    if site.lost_comment:site.lost_comment=False;return self.reply(503,{})
                    return self.reply(200,{"result":"SUCCESS"})
                data=self.cursor([c for c in site.comments["ones"].values() if c["issue"]==key],100,query.get("cursor"),"list")
            elif query.get("action")=="executableWorkflow" and verb=="GET":
                data=[{"id":t,"start":"open","end":"closed"} for t in ("trans1","trans2")]
            elif query.get("action")=="executeWorkflow" and verb=="POST" and body=={"id":"trans2"}:
                site.ones[key]["status"]={"id":"closed","name":"Done","category":"done"};site.writes["ones"]+=1
                if site.lost_status:site.lost_status=False;return self.reply(503,{})
                data=None
            elif verb=="GET":return self.reply(200,site.ones[key])
            else:return self.reply(400,{})
        else:return self.reply(404,{})
        self.reply(200,{"result":"SUCCESS","data":data})

    do_GET=request
    do_POST=request
    do_PUT=request


def verify(directory,start,stop):
    cli=release.binary(directory,"shipforge")
    srv=release.binary(directory,"shipforge-server")
    worker=release.binary(directory,"shipforge-worker")
    site=ThreadingHTTPServer(("127.0.0.1",0),Source)
    site.linear={"one":linear("one"),"two":linear("two","alt")}
    site.linear["one"]["relations"]={"nodes":[{"id":"edge","type":"blocks","relatedIssue":{"id":"two"}}],"pageInfo":{"hasNextPage":True,"endCursor":"next"}}
    site.clickup={k:clickup(k) for k in ("one","two")}
    site.clickup["one"]["dependencies"]=[{"task_id":"two","depends_on":"one"}]
    site.ones={"one":ones("one","bugs","缺陷"),"two":ones("two","stories","需求")}
    site.writes={s:0 for s in ("linear","clickup","ones")}
    site.comments={s:{} for s in site.writes}
    site.calls=[]
    site.expired=site.lost_status=site.lost_comment=site.foreign=site.bad_graph=site.clickup_rotated=False
    thread=threading.Thread(target=site.serve_forever,daemon=True)
    thread.start()
    clean={k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
    try:
        with tempfile.TemporaryDirectory(prefix="shipforge-cloud-") as tmp:
            data=Path(tmp)/"server"; origin=f"http://127.0.0.1:{site.server_port}"
            def issue(actor,scope):
                return json.loads(subprocess.check_output([srv,"--data-dir",data,"token","issue","--actor",actor,"--scope",scope],encoding="utf8",env=clean))["token"]
            admin,readonly,alice=issue("owner","write"),issue("owner","read"),issue("alice","write")
            server,address=start(srv,data,"127.0.0.1:0",env=clean)
            try:
                env=dict(clean,SHIPFORGE_DATA_DIR=str(Path(tmp)/"client"),SHIPFORGE_SERVER="http://"+address,SHIPFORGE_TOKEN=admin,SHIPFORGE_VENDOR_LOOPBACK=origin)
                def call(op,*,ok=True,token=admin,**fields):
                    res=subprocess.run([cli,"request"],input=json.dumps(dict(op=op,**fields)),env=dict(env,SHIPFORGE_TOKEN=token),capture_output=True,text=True,encoding="utf8",timeout=20)
                    resp=json.loads(res.stdout)
                    assert resp["ok"]==ok,(op,fields,resp)
                    return resp["data"] if ok else resp["error"]
                def edit(kind,key,**fields):
                    row=call("get",kind=kind,id=key)
                    return call("update",kind=kind,id=key,expected_rev=row["rev"],fields=fields)
                def configure(key,provider,**fields):
                    return call("create",kind="connector",id=key,title=key,fields=dict(provider=provider,enabled=True,**fields))
                def local_tasks(key):
                    return {r["fields"]["external_id"].replace("linear:","workitem:",1):r for r in call("list",kind="task") if r["fields"].get("connector")==key}
                def linear_event(record):
                    return {"type":"Issue","action":"update","organizationId":"org","data":copy.deepcopy(record)}
                def poll(key):
                    res=subprocess.run([worker,"tracker-sync","--connector",key],env=env,capture_output=True,timeout=20)
                    assert res.returncode==0,res.stderr.decode()
                    return json.loads(res.stdout)
                call("create",kind="member",id="alice",title="Alice",fields={"role":"member"})
                configure("lin","linear",cloud_auth="key",vendor_write=True,mapping={"status":{"a":"in_progress","alt":"backlog"},"write_status":{"done":"closed"}})
                legacy=call("provider.ingest",id="lin",provider="linear",event_id="prior-version",payload=linear_event(site.linear["one"]))["task"]
                edit("task",legacy,description="Protected local note")
                edit("connector","lin",remote_scope="project")
                configure("click","clickup",cloud_auth="key",remote_scope="list",vendor_write=True,assignee_map={"7":"alice"},mapping={"status":{"ready for QA":"in_progress"},"write_status":{"done":"all done"}})
                configure("one","ones",ones_url=origin,remote_scope="project",vendor_write=True,mapping={"write_status":{"done":"closed"},"write_workflow":{"done":"trans2"}})
                call("create",kind="connector",title="Doc domain",fields={"provider":"ones","ones_url":"https://open.ones.cn"},ok=False)
                for vendor,key,account in [("linear","lin","org"),("clickup","click","45"),("ones","one","team")]:
                    token=vendor+"-key"
                    before=len(site.calls)
                    assert call(vendor+".credential",id=key,access_token=token,token=readonly,ok=False)["code"]=="forbidden"
                    assert len(site.calls)==before
                    call(vendor+".credential",id=key,access_token="wrong",ok=False)
                    found=call(vendor+".accounts",id=key,access_token=token)["accounts"]
                    assert account in [r["id"] for r in found]
                    assert call(vendor+".credential",id=key,account_id=account)["account_id"]==account
                    assert call("get",kind="connector",id=key)["fields"]["enterprise_id"]==account
                    projects=call(vendor+".projects",id=key,page_size=1)["projects"]
                    assert len(projects)==2,projects
                    if vendor=="clickup":
                        assert next(p for p in projects if p["id"]=="list")["folder"]["id"]=="folders"
                        assert next(p for p in projects if p["id"]=="solo")["folder"] is None
                        assert call(vendor+".credential",id=key,access_token=token,account_id="46",ok=False)["code"]=="forbidden"
                        call("clickup.accounts",id=key,access_token="clickup-key-rotated")
                        assert call("clickup.credential",id=key,account_id=account)["saved"]
                        site.clickup_rotated=True
                        assert len(call("clickup.projects",id=key)["projects"])==2
                    site.foreign=True
                    assert call(vendor+".preview",id=key,ok=False)["code"]=="forbidden"
                    site.foreign=False
                    if vendor=="linear":
                        site.bad_graph=True
                        call("linear.preview",id=key,ok=False)
                        site.bad_graph=False
                    preview=call(vendor+".preview",id=key,page_size=1)
                    expected_new=1 if vendor=="linear" else 2
                    assert preview["counts"]["created"]==expected_new and len(local_tasks(key))==2-expected_new
                    committed=call("imports.commit",id=key,batch=preview["batch"])
                    assert committed["counts"]["created"]==expected_new
                    assert call("imports.commit",id=key,batch=preview["batch"])==committed
                    out=local_tasks(key)
                    task=out["workitem:one"];other=out["workitem:two"]
                    if vendor=="linear":
                        assert task["id"]==legacy and task["fields"]["description"]=="Protected local note"
                        assert task["fields"]["status"]=="in_progress" and other["fields"]["status"]=="backlog"
                        assert committed["relations"]=={"created":1,"unavailable_or_conflicting":1}
                    elif vendor=="clickup":
                        assert committed["relations"]["created"]==1
                        assert task["fields"]["status"]=="in_progress" and task["fields"]["source_status_id"]=="ready for QA"
                        assert task["fields"]["assignees"]==["alice"] and task["fields"]["source_assignees"]==["8"]
                        assert any(w["field"]=="time_estimate" for w in task["fields"]["source_warnings"])
                    else:
                        assert task["fields"]["type"]=="bug" and other["fields"]["type"]=="requirement"
                        assert task["fields"]["description"]=="source detail"
                    count=len(call("imports.history",id=key)["items"])
                    assert poll(key)["state"]=="unchanged" and len(call("imports.history",id=key)["items"])==count
                    native=getattr(site,vendor)["two"]
                    field="name" if vendor=="clickup" else "title"
                    if vendor=="linear":
                        watcher=subprocess.Popen([worker,"tracker-sync","--connector",key,"--interval","300","--watch"],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,encoding="utf8")
                        lines=queue.Queue()
                        output=threading.Thread(target=lambda:[lines.put(json.loads(line)) for line in watcher.stdout],daemon=True)
                        output.start()
                        try:
                            assert lines.get(timeout=20)["state"]=="unchanged"
                            native[field]="Changed"
                            event=linear_event(native)
                            kwargs=dict(provider="linear",id=key,event_id="linear-delivery",payload=event)
                            for foreign in [dict(event,organizationId="wrong"),dict(event,data=dict(event["data"],project={"id":"other"}))]:
                                assert call("provider.ingest",**dict(kwargs,payload=foreign),ok=False)["code"]=="forbidden"
                            assert call("provider.preview",**kwargs)["action"]=="sync_requested"
                            assert call("imports.binding",id=key)["signal_revision"]==0
                            delivery=call("provider.ingest",**kwargs)
                            assert delivery["action"]=="sync_requested" and delivery==call("provider.ingest",**kwargs)
                            assert call("provider.ingest",**dict(kwargs,payload=dict(event,data=dict(native,title="another"))),ok=False)["code"]=="conflict"
                            assert lines.get(timeout=20)["counts"]["updated"]==1
                            assert len(local_tasks(key))==2
                            assert call("connector.ingest",id=key,external_id="another-task",event_id="bypass",title="other",source_version="2",ok=False)["code"]=="invalid"
                        finally:
                            watcher.terminate();watcher.wait(timeout=8);output.join(timeout=3);watcher.stdout.close()
                    else:
                        native[field]="Changed"
                        assert poll(key)["counts"]["updated"]==1
                    assert local_tasks(key)["workitem:two"]["title"]=="Changed"
                    edit("task",task["id"],status="done")
                    site.lost_status=True
                    call("imports.push",id=key,task_id=task["id"],ok=False)
                    receipt=call("imports.push",id=key,task_id=task["id"])
                    assert receipt["state"]=="sent" and site.writes[vendor]==1
                    assert call("imports.push",id=key,task_id=task["id"])==receipt
                    comment=call("create",kind="comment",title="Update",fields={"task":task["id"],"body":"Synthetic update"})
                    kwargs={"id":key,"task_id":task["id"],"intent":"comment","comment_id":comment["id"]}
                    site.lost_comment=True
                    call("imports.push",**kwargs,ok=False)
                    assert call("imports.push",**kwargs,ok=False)["code"]=="conflict"
                    call("imports.push",**kwargs,confirm_comment="wrong",ok=False)
                    assert call("imports.push",**kwargs,confirm_comment="comment-1")["state"]=="sent" and len(site.comments[vendor])==1
                    assert call("imports.push_history",id=key)["items"][0]["state"]=="sent"
                    assert call("imports.rollback",id=key,batch=preview["batch"],token=alice,ok=False)["code"]=="forbidden"
                    if vendor=="ones":
                        c=call("create",kind="comment",title="Next",fields={"task":task["id"],"body":"Ack only"})
                        assert call("imports.push",id=key,task_id=task["id"],intent="comment",comment_id=c["id"])["remote_id"] is None
                    undone=call("imports.rollback",id=key,batch=preview["batch"])
                    assert undone["deleted"]==expected_new
                    hits=len(site.calls)
                    assert poll(key)["state"]=="paused" and len(site.calls)==hits and len(local_tasks(key))==2-expected_new
                    if vendor=="linear":
                        assert undone["released_existing"]==1 and local_tasks(key)["workitem:one"]["id"]==legacy
                        assert call("provider.ingest",provider="linear",id=key,event_id="paused",payload=event)["action"]=="paused"
                    assert call("imports.sync_control",id=key,action="resume")=={"paused":False}
                assert any("updatedAt" in v["filter"] for s,op,v in site.calls if s=="linear" and op=="ReadIssues")
                assert any("date_updated_gt" in v for s,op,v in site.calls if s=="clickup" and op=="/list/list/task")
                for vendor,scope,account in [("linear","alt","org"),("clickup","solo","45")]:
                    key=vendor+"-oauth"
                    configure(key,vendor,cloud_auth="oauth",remote_scope=scope)
                    auth=call(vendor+".authorize",id=key,client_id="client",redirect_uri="http://localhost/callback",write=True)
                    params=parse_qs(urlsplit(auth["authorize_url"]).query)
                    assert params["client_id"]==["client"] and params["state"]
                    if vendor=="linear":assert params["code_challenge_method"]==["S256"] and params["scope"]==["read,write"]
                    assert call(vendor+".credential",id=key,code=vendor+"-code",state="mismatch",ok=False)["code"]=="forbidden"
                    opts={"id":key,"code":vendor+"-code","client_secret":"fixture","state":params["state"][0]}
                    found=call(vendor+".accounts",**opts)["accounts"]
                    assert account in [a["id"] for a in found]
                    assert call(vendor+".credential",id=key,account_id=account)["saved"]
                    call(vendor+".credential",**opts,ok=False)
                    if vendor=="linear":site.expired=True
                    assert len(call(vendor+".projects",id=key)["projects"])==2
                call("ones.credential",id="one",clear=True)
                call("ones.projects",id="one",ok=False)
            finally:
                stop(server)
    finally:
        site.shutdown();site.server_close();thread.join(timeout=3)
    print("Verified native Linear, ClickUp and ONES auth, scopes, pagination, relations, resilient writes, polling and rollback over isolated HTTP.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    team=import_module("verify-team")
    verify(parser.parse_args().directory.resolve(),team.start,team.stop)
