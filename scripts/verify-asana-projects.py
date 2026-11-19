"""Check native Asana workspace discovery, project imports and restricted publication through a team."""
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


class Vendor(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, data):
        raw = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def handle_api(self):
        site = self.server
        request = urlsplit(self.path)
        path = request.path.removeprefix("/api/1.0")
        params = {k: values[0] for k, values in parse_qs(request.query).items()}
        if self.headers.get("Authorization") not in ("Bearer asana-two", "Bearer asana-new"):
            return self.reply(401, {"errors":[{"message":"Access denied"}]})
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or "{}")

        def paged(rows):
            offset = int(params.get("offset", "0"))
            end = offset + int(params["limit"])
            next_offset = {"offset": str(end)} if end < len(rows) else None
            # A client must construct requests against its bound origin rather than follow this URI.
            if next_offset:
                next_offset["uri"] = "http://127.0.0.1:1/untrusted"
            return self.reply(200, {"data":rows[offset:end], "next_page":next_offset})

        if path == "/workspaces":
            return paged([{"gid":"11", "name":"Research"}, {"gid":"12", "name":"Support"}])
        if path == "/workspaces/11/projects":
            assert params.get("archived") == "false"
            return paged([{"gid":gid, "workspace":{"gid":"11"}, "name":"Project "+gid, "archived":False} for gid in ("21","22")])
        if path == "/projects/21":
            return self.reply(200, {"data":{"gid":"21","workspace":{"gid":"11"},"archived":False}})
        if path == "/tasks":
            assert params.get("project") == "21" and "workspace" not in params
            assert params.get("completed_since") == "1970-01-01T00:00:00Z" and "memberships.project.gid" in params["opt_fields"]
            since = params.get("modified_since")
            site.filters.append(since)
            rows = [copy.deepcopy(row) for row in site.tasks.values() if not since or row["modified_at"] >= since]
            if site.foreign:
                rows[0]["memberships"] = [{"project":{"gid":"89"}}]
            return paged(rows)
        if path == "/tasks/51/stories":
            if self.command == "POST":
                assert set(body["data"]) == {"text"}
                story = {"gid":str(61+len(site.stories)), "text":body["data"]["text"], "resource_subtype":"comment_added"}
                site.stories.append(story)
                if site.lose:
                    site.lose = False
                    return self.reply(503, {})
                return self.reply(201, {"data":story})
            return paged(site.stories)
        if path.startswith("/tasks/") and path.split("/")[-1] in site.tasks:
            row = site.tasks[path.split("/")[-1]]
            if self.command == "PUT":
                assert set(body["data"]) <= {"name", "notes"}
                site.writes.append(body["data"])
                row.update(body["data"])
            return self.reply(200, {"data":row})
        return self.reply(404, {"errors":[{"message":"Unknown fixture route"}]})

    do_GET = do_POST = do_PUT = handle_api


def verify(directory, start, stop):
    binary = import_module("verify-release").binary
    cli, service, worker = [binary(directory, name) for name in ("shipforge", "shipforge-server", "shipforge-worker")]
    site = ThreadingHTTPServer(("127.0.0.1", 0), Vendor)
    site.filters, site.stories, site.writes = [], [], []
    site.lose, site.foreign = False, False
    site.tasks = {key:{"gid":key,"resource_type":"task","workspace":{"gid":"11"},"memberships":[{"project":{"gid":"21"}},{"project":{"gid":"22"}}],
                       "notes":"Original","name":"Asana "+key,"modified_at":"2025-09-12T01:00:00.000Z","completed":key=="52",
                       "assignee":{"gid":"71" if key=="51" else "72"},"permalink_url":"https://app.asana.com/0/21/"+key} for key in ("51","52")}
    thread = threading.Thread(target=site.serve_forever, daemon=True)
    thread.start()
    clean = {k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
    try:
        with tempfile.TemporaryDirectory(prefix="shipforge-asana-") as temp:
            root = Path(temp)
            data = root / "team"
            def issue(actor, scope="write"):
                raw = subprocess.check_output([service,"--data-dir",data,"token","issue","--actor",actor,"--scope",scope], env=clean)
                return json.loads(raw)["token"]
            tokens = {s:issue(s) for s in ("owner","alice","stranger")}
            tokens["reader"] = issue("owner","read")
            server, address = start(service, data, "127.0.0.1:0", env=dict(clean, SHIPFORGE_VENDOR_LOOPBACK=f"http://127.0.0.1:{site.server_port}"))
            def env(user):
                return dict(clean, SHIPFORGE_SERVER="http://"+address, SHIPFORGE_TOKEN=tokens[user], SHIPFORGE_DATA_DIR=str(root/user))
            def call(op,user="owner",ok=True,**args):
                proc = subprocess.run([cli,"request"], input=json.dumps(dict(op=op,**args)), env=env(user), capture_output=True, text=True, encoding="utf8", timeout=30)
                result = json.loads(proc.stdout)
                assert result["ok"] == ok, (op, user, result)
                return result["data" if ok else "error"]
            def edit(kind, key, **fields):
                row=call("get",kind=kind,id=key)
                return call("update",kind=kind,id=key,expected_rev=row["rev"],fields=fields)
            def sync():
                raw=subprocess.check_output([worker,"tracker-sync","--connector","asana"],env=env("owner"),timeout=35)
                return json.loads(raw)
            try:
                call("create",kind="member",id="alice",title="Alice",fields={"role":"member"})
                call("create",kind="connector",id="asana",title="Asana",fields={"provider":"asana","enabled":True,"assignee_map":{"71":"alice"}})
                legacy=call("provider.ingest",id="asana",provider="asana",event_id="before-project",payload={"data":site.tasks["51"]})["task"]
                creds=dict(id="asana",access_token="asana-two")
                assert len(call("asana.accounts",**creds,page_size=1)["workspaces"])==2
                for user in ("reader","alice","stranger"):
                    assert call("asana.credential",user,False,**creds,workspace_id="11")["code"]=="forbidden"
                call("asana.credential",ok=False,**creds)
                call("asana.credential",ok=False,**creds,workspace_id="99")
                assert call("asana.credential",**creds,workspace_id="11")["suffix"]=="-two"
                assert len(call("asana.projects",id="asana",page_size=1)["projects"])==2
                edit("connector","asana",remote_scope="21",mapping={"status":{"open":"in_progress","completed":"done"}})
                site.foreign=True
                assert call("asana.preview",id="asana",ok=False)["code"]=="forbidden"
                site.foreign=False
                preview=call("asana.preview",id="asana",page_size=1)
                batch=call("imports.commit",id="asana",batch=preview["batch"])
                assert batch["counts"]["created"]==1 and batch["counts"]["updated"]==1 and not site.writes, batch
                tasks={r["fields"]["source_key"]:r for r in call("list",kind="task") if r["fields"].get("connector")=="asana"}
                assert tasks["51"]["id"]==legacy and tasks["51"]["fields"]["assignees"]==["alice"]
                assert tasks["52"]["fields"]["status"]=="done" and tasks["52"]["fields"]["source_assignees"]==["72"]
                assert sync()["state"]=="unchanged" and any(site.filters)
                site.tasks["51"].update(name="Updated upstream",modified_at="2025-09-13T00:00:00.000Z")
                args=dict(id="asana",provider="asana",event_id="fresh",payload={"data":site.tasks["51"]})
                signal=call("provider.ingest",**args)
                assert signal["action"]=="sync_requested" and call("provider.ingest",**args)==signal
                assert call("provider.ingest",id="asana",provider="asana",event_id="wrong-project",payload={"data":dict(site.tasks["51"],workspace={"gid":"12"})},ok=False)["code"]=="forbidden"
                applied=sync()
                assert applied["counts"]["updated"]==1 and applied["signal_revision"]==signal["signal_revision"]
                edit("task",legacy,description="Local note",status="done")
                fields=dict(id="asana",task_id=legacy,intent="fields")
                call("asana.push","alice",False,**fields)
                edit("connector","asana",vendor_write=True)
                call("asana.push","alice",False,**dict(fields,intent="status"))
                site.tasks["51"]["notes"]="Concurrent edit"
                assert call("asana.push","alice",False,**fields)["code"]=="conflict" and not site.writes
                site.tasks["51"]["notes"]="Original"
                assert call("asana.push","alice",**fields)["state"]=="sent"
                assert site.writes==[{"notes":"Local note"}] and not site.tasks["51"]["completed"]
                edit("task",legacy,description="Second local note")
                assert call("asana.push","alice",**fields)["state"]=="sent"
                note=call("create","alice",kind="comment",title="Note",fields={"task":legacy,"body":"Post to Asana"})
                comment=dict(fields,intent="comment",comment_id=note["id"])
                site.lose=True
                call("asana.push","alice",False,**comment)
                call("asana.push","alice",False,**comment)
                call("asana.push","alice",False,**comment,confirm_comment="77")
                assert call("asana.push","alice",**comment,confirm_comment="61")["remote_id"]=="61" and len(site.stories)==1
                new_comment=dict(fields,intent="comment",body="Send this new comment too",request_id="c882f530-f512-49f6-8565-0b638cf90311")
                first=call("asana.push","alice",**new_comment)
                assert first["comment_id"]==call("asana.push","alice",**new_comment)["comment_id"] and len(site.stories)==2
                assert call("get","alice",kind="comment",id=first["comment_id"])["fields"]["body"]==new_comment["body"]
                call("asana.credential",id="asana",clear=True)
                call("asana.projects",id="asana",ok=False)
                assert call("asana.credential",id="asana",access_token="asana-new")["suffix"]=="-new"
                call("imports.rollback","alice",False,id="asana",batch=preview["batch"])
                rolled=call("imports.rollback",id="asana",batch=preview["batch"])
                assert rolled["deleted"]==1 and rolled["released_existing"]==1 and sync()["state"]=="paused"
                assert call("get",kind="task",id=legacy)["fields"]["description"]=="Second local note"
                assert not (root/"alice"/"vendor-credentials").exists()
            finally:
                stop(server)
    finally:
        site.shutdown()
        site.server_close()
        thread.join(timeout=4)
    print("Verified hosted Asana workspace selection, legacy adoption, native filtered synchronization and restricted writeback.")


if __name__=="__main__":
    p=argparse.ArgumentParser()
    p.add_argument("directory",type=Path)
    team=import_module("verify-team")
    verify(p.parse_args().directory.resolve(),team.start,team.stop)
