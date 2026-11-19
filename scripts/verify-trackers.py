"""Exercise ZenTao REST v1 and PingCode OpenAPI v2 with a real CLI, worker and team server."""
import argparse
import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit


release = import_module("verify-release")
ZEN = "/zentao/api.php/v1"
PC = "/v2"
EPOCH = 1735000000


def zen_record(key, title, **fields):
    return dict(id=key, title=title, status="active", pri=2, product=7, project=19,
                assignedTo={"account": "external"}, openedBy={"account": "reporter"},
                lastEditedDate="2025-05-29T11:45:00Z", **fields)


def pc_record(key, **fields):
    row = dict(id=key, title="Work item " + key, description="original", whole_identifier="PC-" + key,
               project={"id": "proj"}, type={"id": "defect", "name": "Bug"},
               state={"id": "open", "name": "Open", "type": "pending"}, priority={"id": "p1", "name": "High"},
               assignee={"id": "foreign"}, created_by={"id": "upstream-user"}, updated_at=EPOCH,
               html_url="https://fixture.pingcode.com/pjm/workitems/" + key)
    row.update(fields)
    return row


class Vendor(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, code, obj):
        data = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def pages(self, rows, args, key, zen=False):
        begin = 1 if zen else 0
        field = "page" if zen else "page_index"
        size_field = "limit" if zen else "page_size"
        page = int(args[field][0])
        size = int(args[size_field][0])
        start = (page - begin) * size
        return self.reply(200, {key: rows[start:start + size], "total": len(rows), field: page, size_field: size})

    def dispatch(self):
        path = urlsplit(self.path)
        args = parse_qs(path.query)
        site = self.server
        data = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or "{}")
        method = self.command
        route = path.path
        site.calls.append((method, route, {k: v for k, v in args.items() if k not in ("code", "client_secret")}))
        if route == ZEN + "/tokens" and method == "POST":
            valid = data == {"account": "fixture", "password": "fixture-pass"}
            return self.reply(201 if valid else 401, {"token": "fixture-token"} if valid else {})
        if route == PC + "/auth/token" and method == "GET":
            mode = args.get("grant_type", [""])[0]
            if mode == "refresh_token":
                valid = args == {"grant_type":["refresh_token"],"refresh_token":["refresh-open"]}
                return self.reply(200 if valid else 401,{"access_token":"renewed","refresh_token":"refresh-next"} if valid else {})
            valid = args.get("client_id") == ["fixture-id"] and args.get("client_secret") == ["fixture-secret"]
            valid = valid and (mode == "client_credentials" or (mode == "authorization_code" and args.get("code") == ["fixture-code"]))
            return self.reply(200 if valid else 401, {"access_token":"oauth-token" if mode=="authorization_code" else "pc-token","refresh_token":"refresh-open"} if valid else {})
        if route.startswith(ZEN + "/"):
            if self.headers.get("Token") != "fixture-token":
                return self.reply(401, {})
            tail = route[len(ZEN):]
            if method == "GET":
                if tail in ("/products", "/projects") and args.get("status") == ["all"]:
                    group = tail[1:]
                    rows = [{"id": 7, "name": "Product", "status": "normal"}] if group == "products" else [{"id": 19, "name": "Project", "status": "closed"}]
                    return self.pages(rows, args, group, True)
                if tail in ("/products/7/bugs", "/projects/19/bugs"):
                    rows = list(copy.deepcopy(site.bugs).values())
                    rows[0].pop("steps",None)
                    if site.foreign:
                        rows[0]["product"] = 11
                    return self.pages(rows, args, "bugs", True)
                if tail in ("/products/7/stories", "/projects/19/stories"):
                    rows = [site.story] if args.get("type") != ["requirement"] else [site.requirement]
                    return self.pages(rows, args, "stories", True)
                if tail == "/projects/19/executions":
                    return self.pages([{"id": 9, "project": 20 if site.moved_execution else 19}], args, "executions", True)
                if tail == "/executions/9/tasks":
                    return self.pages([site.task], args, "tasks", True)
                if tail.startswith("/bugs/"):
                    found = site.bugs.get(tail.split("/")[-1])
                    if found:
                        return self.reply(200, dict(found,product=99) if site.moved_detail else found)
            if method == "POST" and tail in ("/bugs/37/resolve", "/bugs/37/close", "/bugs/37/active"):
                action = tail.rsplit("/", 1)[1]
                if action == "resolve" and data != {"resolution": "fixed", "resolvedBuild": "trunk"}:
                    return self.reply(400, {})
                if action == "active" and data != {"openedBuild": "trunk"}:
                    return self.reply(400, {})
                site.zen_writes.append(action)
                site.bugs["37"]["status"] = {"active": "active", "resolve": "resolved", "close": "closed"}[action]
                if site.uncertain_transition:
                    site.uncertain_transition = False
                    return self.reply(503, {})
                return self.reply(200, site.bugs["37"])
        if route.startswith(PC + "/"):
            allowed = {"Bearer pc-token","Bearer renewed"}
            if not site.expired:
                allowed.add("Bearer oauth-token")
            if self.headers.get("Authorization") not in allowed:
                return self.reply(401, {})
            tail = route[len(PC):]
            if tail == "/directory/team" and method == "GET":
                return self.reply(200, {"id": "enterprise", "name": "Synthetic team", "domain": "fixture"})
            if tail == "/pjm/projects" and method == "GET" and args.get("include_archived") == ["1"]:
                rows = [{"id": "proj", "name": "Bound project", "is_archived": 0},
                        {"id": "archived", "name": "Archive", "is_archived": 1}]
                return self.pages(rows, args, "values")
            if tail == "/pjm/workitems" and method == "GET" and args.get("project_id") == ["proj"] and args.get("include_deleted") == ["0"]:
                rows = list(copy.deepcopy(site.items).values())
                if site.foreign:
                    rows[0]["project"] = {"id":"another-project"}
                if args.get("updated_between"):
                    lower, upper = map(int, args["updated_between"][0].split(","))
                    rows = [r for r in rows if lower <= r["updated_at"] <= upper]
                if site.broken_page:
                    return self.reply(200, {"values":[],"total":3,"page_index":0})
                return self.pages(rows, args, "values")
            if tail.startswith("/pjm/workitems/"):
                row = site.items.get(tail.rsplit("/", 1)[1])
                if row and method == "GET":
                    return self.reply(200, row)
                if row and method == "PATCH" and data == {"state_id":"done-id"}:
                    site.pc_writes += 1
                    row["state"] = {"id":"done-id","name":"Closed","type":"done"}
                    return self.reply(200, row)
            if tail == "/comments" and method == "POST":
                if data.get("principal_type") != "workitem" or data.get("principal_id") not in site.items:
                    return self.reply(400, {})
                key = "comment-" + str(len(site.comments)+1)
                site.comments[key] = dict(data, id=key)
                if site.uncertain_comment:
                    site.uncertain_comment = False
                    return self.reply(503, {})
                return self.reply(201, site.comments[key])
            if tail.startswith("/comments/") and method == "GET":
                row = site.comments.get(tail.rsplit("/", 1)[1])
                if row and args == {"principal_type":["workitem"], "principal_id":[row["principal_id"]]}:
                    return self.reply(200, row)
        self.reply(404, {})

    do_GET = dispatch
    do_POST = dispatch
    do_PATCH = dispatch


def verify(directory, start, stop):
    cli = release.binary(directory, "shipforge")
    server = release.binary(directory, "shipforge-server")
    worker = release.binary(directory, "shipforge-worker")
    upstream = ThreadingHTTPServer(("127.0.0.1", 0), Vendor)
    upstream.bugs = {str(k): zen_record(k, "Bug " + str(k), steps="repro") for k in (37, 38)}
    upstream.story = zen_record(11, "Story", spec="intent", type="story", children=[zen_record(12, "Child", spec="detail", type="story")])
    upstream.requirement = zen_record(15, "Requirement", spec="intent", type="requirement")
    upstream.task = zen_record(16, "Task", name="Implementation", execution=9, desc="scoped task")
    upstream.items = {"a": pc_record("a", properties={"category":"origin-only"}), "b": pc_record("b", type={"id":"other"}, assignee={"id":"mapped"})}
    upstream.calls = []
    upstream.comments = {}
    upstream.pc_writes = 0
    upstream.expired = False
    upstream.moved_detail = upstream.moved_execution = False
    upstream.zen_writes = []
    upstream.foreign = upstream.broken_page = upstream.uncertain_transition = upstream.uncertain_comment = False
    thread = threading.Thread(target=upstream.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="shipforge-trackers-") as temp:
            root = Path(temp)
            data = root / "server"
            clean = {k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}

            def issue(actor, scope):
                result = subprocess.check_output([server, "--data-dir", data, "token", "issue", "--actor", actor, "--scope", scope],
                                                 encoding="utf-8", env=clean, timeout=20)
                return json.loads(result)["token"]

            owner, readonly, member = issue("owner","write"), issue("owner","read"), issue("alice","write")
            process, address = start(server, data, "127.0.0.1:0", env=clean)
            try:
                origin = f"http://127.0.0.1:{upstream.server_port}"
                env = dict(clean, SHIPFORGE_SERVER="http://" + address, SHIPFORGE_TOKEN=owner,
                           SHIPFORGE_DATA_DIR=str(root / "client"), SHIPFORGE_VENDOR_LOOPBACK=origin)

                def call(op, *, ok=True, token=owner, **args):
                    p = subprocess.run([cli,"request"], input=json.dumps({"op":op,**args}), env=dict(env,SHIPFORGE_TOKEN=token),
                                       capture_output=True, text=True, encoding="utf-8", timeout=20)
                    out = json.loads(p.stdout)
                    assert out["ok"] == ok, (op,args,out,p.stderr)
                    return out["data"] if ok else out["error"]

                def config(key, provider, *, ok=True, **fields):
                    return call("create",kind="connector",id=key,title=key,ok=ok,fields={"provider":provider,"enabled":True,**fields})

                def update(kind, key, **fields):
                    rev = call("get",kind=kind,id=key)["rev"]
                    return call("update",kind=kind,id=key,expected_rev=rev,fields=fields)

                def tasks(connector):
                    return {r["fields"]["external_id"]:r for r in call("list",kind="task") if r["fields"].get("connector") == connector}

                def commit(op, key, **args):
                    preview = call(op,id=key,page_size=1,**args)
                    committed = call("imports.commit",id=key,batch=preview["batch"])
                    assert committed["state"] == "completed" and committed["counts"] == preview["counts"]
                    return preview

                def poll(key):
                    p = subprocess.run([worker,"tracker-sync","--connector",key],env=env,capture_output=True,timeout=20)
                    assert p.returncode == 0,p.stderr.decode()
                    return json.loads(p.stdout)

                call("create",kind="member",id="alice",title="Alice",fields={"role":"member"})
                z = dict(instance_url=origin + "/zentao",scope_kind="product",remote_scope="7",zentao_edition="opensource",
                         vendor_write=True,mapping={"write_status":{"done":"resolved","cancelled":"closed","in_progress":"active"}})
                config("old","zentao",ok=False,zentao_version="16.4",**z)
                config("insecure","zentao",ok=False,**dict(z,instance_url="http://off-device.invalid"))
                config("zen","zentao",zentao_version="18.12",**z)
                config("project","zentao",**dict(z,scope_kind="project",remote_scope="19"))
                call("zentao.credential",id="zen",account="fixture",password="wrong",ok=False)
                for key in ("zen","project"):
                    assert call("zentao.credential",id=key,account="fixture",password="fixture-pass") == {"saved":True}
                assert call("zentao.projects",id="zen")["projects"][0]["id"] == "7"
                assert call("zentao.projects",id="project")["projects"][0]["status"] == "closed"
                assert call("zentao.preview",id="project",page_size=1)["counts"]["created"] == 5
                upstream.moved_execution = True
                assert call("zentao.preview",id="project",ok=False)["code"] == "forbidden"
                upstream.moved_execution = False
                upstream.moved_detail = True
                assert call("zentao.preview",id="zen",ok=False)["code"] == "forbidden"
                upstream.moved_detail = False
                upstream.foreign = True
                assert call("zentao.preview",id="zen",ok=False)["code"] == "forbidden"
                upstream.foreign = False
                assert not call("imports.history",id="zen")["items"]
                census = call("zentao.preview",id="zen",page_size=1)
                assert census["counts"]["created"] == 5 and not call("list",kind="task"),census
                call("imports.commit",id="zen",batch=census["batch"])
                bug = tasks("zen")["bug:37"]
                assert bug["fields"]["priority"] == "high" and bug["fields"]["source_assignees"] == ["external"]
                assert tasks("zen")["requirement:15"]["fields"]["type"] == "requirement"
                assert ("GET", ZEN+"/products/7/bugs", {"status":["all"],"order":["id_asc"],"limit":["1"],"page":["2"]}) in upstream.calls
                assert commit("zentao.preview","zen")["counts"]["unchanged"] == 5
                config("duplicate","zentao",**dict(z,instance_url=z["instance_url"]+"/"))
                assert call("imports.binding",id="duplicate",ok=False)["code"] == "conflict"
                update("task",bug["id"],status="done")
                upstream.uncertain_transition = True
                call("imports.push",id="zen",task_id=bug["id"],ok=False)
                delivered = call("imports.push",id="zen",task_id=bug["id"])
                assert delivered["state"] == "sent" and upstream.zen_writes == ["resolve"]
                assert call("imports.push",id="zen",task_id=bug["id"]) == delivered
                update("task",bug["id"],status="cancelled")
                assert call("imports.push",id="zen",task_id=bug["id"])["state"] == "sent" and upstream.zen_writes == ["resolve","close"]
                update("task",bug["id"],status="in_progress")
                upstream.bugs["37"]["status"] = "resolved"
                assert call("imports.push",id="zen",task_id=bug["id"],ok=False)["code"] == "conflict"
                assert upstream.zen_writes == ["resolve","close"]

                pconfig = dict(enterprise_id="enterprise",remote_scope="proj",pingcode_auth="token",vendor_write=True,
                               mapping={"type":{"defect":"bug"},"write_status":{"done":"done-id"}},assignee_map={"mapped":"alice"})
                config("pc","pingcode",**pconfig)
                config("foreign","pingcode",**dict(pconfig,enterprise_id="foreign"))
                assert call("pingcode.credential",id="foreign",access_token="pc-token",ok=False)["code"] == "forbidden"
                config("apps","pingcode",**dict(pconfig,pingcode_auth="application",remote_scope="archived"))
                config("oauth","pingcode",**dict(pconfig,pingcode_auth="authorization",remote_scope="archived"))
                call("pingcode.credential",id="apps",client_id="fixture-id",client_secret="fixture-secret")
                call("pingcode.credential",id="oauth",client_id="fixture-id",client_secret="fixture-secret",code="wrong",ok=False)
                call("pingcode.credential",id="oauth",client_id="fixture-id",client_secret="fixture-secret",code="fixture-code")
                upstream.expired = True
                assert len(call("pingcode.projects",id="oauth")["projects"]) == 2
                assert any(path==PC+"/auth/token" and args.get("grant_type")==["refresh_token"] for _,path,args in upstream.calls)
                call("pingcode.credential",id="pc",access_token="invalid",ok=False)
                call("pingcode.credential",id="pc",access_token="pc-token")
                hits = len(upstream.calls)
                assert call("pingcode.projects",id="pc",token=member,ok=False)["code"] == "forbidden"
                assert len(upstream.calls) == hits
                projects = call("pingcode.projects",id="pc",page_size=1)["projects"]
                assert {r["id"] for r in projects} == {"proj","archived"} and any(r["archived"] for r in projects)
                upstream.foreign = True
                assert call("pingcode.preview",id="pc",ok=False)["code"] == "forbidden"
                upstream.foreign = False
                upstream.broken_page = True
                call("pingcode.preview",id="pc",ok=False)
                upstream.broken_page = False
                preview = commit("pingcode.preview","pc")
                assert preview["counts"]["created"] == 2
                call("create",kind="space",id="isolated",title="Separate space",fields={"visibility":"public"})
                call("create",space="isolated",kind="connector",id="pc",title="Separate connector",fields={"provider":"pingcode","enabled":True,**dict(pconfig,assignee_map={})})
                assert call("imports.binding",id="pc",space="isolated",ok=False)["code"] == "conflict"
                assert call("imports.rollback",id="pc",space="isolated",batch=preview["batch"],ok=False)["code"] == "not_found"
                records = tasks("pc")
                work = records["workitem:a"]
                assert work["fields"]["type"] == "bug" and work["fields"]["source_assignees"] == ["foreign"]
                assert work["fields"]["source_properties"] == {"category":"origin-only"}
                assert records["workitem:b"]["fields"]["type"] == "task" and records["workitem:b"]["fields"]["assignees"] == ["alice"]
                independent = call("create",kind="task",title="local only",fields={})
                assert call("imports.push",id="pc",task_id=independent["id"],ok=False)["code"] == "not_found"
                old_batches = len(call("imports.history",id="pc")["items"])
                update("space","default",triage_enabled=True)
                assert poll("pc")["state"] == "unchanged" and len(call("imports.history",id="pc")["items"]) == old_batches
                upstream.items["a"]["description"] = "remote edit at same second"
                upstream.items["c"] = pc_record("c",updated_at=EPOCH+1)
                synced = poll("pc")
                assert synced["counts"]["created"] == synced["counts"]["updated"] == 1
                assert tasks("pc")["workitem:c"]["fields"]["status"] == "triage"
                assert tasks("pc")["workitem:a"]["fields"]["description"] == "remote edit at same second"
                assert any("updated_between" in args for _,path,args in upstream.calls if path.endswith("/pjm/workitems"))
                update("task",work["id"],description="edited locally")
                upstream.items["a"].update(description="independent edit",updated_at=EPOCH+1)
                assert poll("pc")["state"] == "needs_review"
                assert tasks("pc")["workitem:a"]["fields"]["description"] == "edited locally"
                upstream.items["a"]["description"] = "remote edit at same second"
                update("task",work["id"],status="done")
                assert call("imports.push",id="pc",task_id=work["id"])["state"] == "sent" and upstream.pc_writes == 1
                update("connector","pc",vendor_write=False)
                call("imports.push",id="pc",task_id=work["id"],ok=False)
                update("connector","pc",vendor_write=True)
                comment = call("create",kind="comment",title="fixture",fields={"task":work["id"],"body":"publish this text"})
                wrong = call("create",kind="comment",title="other",fields={"task":independent["id"],"body":"unrelated text"})
                assert call("imports.push",id="pc",task_id=work["id"],intent="comment",comment_id=wrong["id"],ok=False)["code"] == "forbidden"
                assert not upstream.comments
                args = dict(id="pc",task_id=work["id"],intent="comment",comment_id=comment["id"])
                upstream.uncertain_comment = True
                call("imports.push",**args,ok=False)
                assert call("imports.push",**args,ok=False)["code"] == "conflict"
                call("imports.push",**args,confirm_comment="wrong",ok=False)
                receipt = call("imports.push",**args,confirm_comment="comment-1")
                assert receipt["remote_id"] == "comment-1" and len(upstream.comments) == 1
                assert call("imports.push",**args) == receipt
                assert call("imports.push_history",id="pc")["items"][0]["state"] == "sent"
                assert call("imports.rollback",id="pc",batch=preview["batch"],token=readonly,ok=False)["code"] == "forbidden"
                denied = call("imports.rollback",id="pc",batch=preview["batch"],token=member,ok=False)
                assert denied["code"] == "forbidden",denied
                assert call("imports.rollback",id="pc",batch=census["batch"],ok=False)["code"] == "not_found"
                assert call("imports.rollback",id="pc",batch=synced["batch"])["deleted"] == 1
                assert call("imports.rollback",id="pc",batch=preview["batch"])["deleted"] == 2
                assert call("imports.rollback",id="zen",batch=census["batch"])["deleted"] == 5
                assert call("get",kind="task",id=independent["id"])["title"] == "local only"
                update("connector","pc",enabled=False)
                assert call("imports.push_history",id="pc")["items"]
                assert call("pingcode.credential",id="pc",clear=True) == {"saved":False}
                update("connector","pc",enabled=True)
                call("pingcode.projects",id="pc",ok=False)
                assert len(call("pingcode.projects",id="apps")["projects"]) == 2
            finally:
                stop(process)
    finally:
        upstream.shutdown()
        upstream.server_close()
        thread.join(timeout=3)
    print("Verified ZenTao and PingCode authentication, scoped pagination and mapping, recoverable native writes, polling and isolated batch rollback.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    team = import_module("verify-team")
    verify(parser.parse_args().directory.resolve(),team.start,team.stop)
