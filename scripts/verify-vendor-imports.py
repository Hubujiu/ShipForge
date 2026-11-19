"""Exercise the shipped Zoho and Bugly paths against disposable HTTP servers and databases."""
import argparse
import base64
import hashlib
import hmac
import json
import os
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.parse import parse_qs, quote, urlsplit
from urllib.request import ProxyHandler, Request, build_opener

release = import_module("verify-release")
PORTAL = "2063927"
PROJECT = "170876000003152069"
FIRST = 170876000001834047
PRODUCT = "syntheticProduct"
SIGN_KEY = "synthetic-company-secret"


def bug(number, title, modified=5000):
    return {"id": number, "title": title, "key": number - FIRST + 541,
            "status": {"type": "InProgress"}, "severity": {"type": "Major"}, "closed": False,
            "last_modified_time_long": modified, "reported_person": "Outside reporter",
            "reporter_id": "483", "assignee_name": "External engineer", "description": "upstream original"}


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

    def do_GET(self):
        if self.headers.get("Authorization") != "Zoho-oauthtoken " + self.server.token:
            return self.reply(401, {"error": {"code": 401}})
        parsed = urlsplit(self.path)
        path = parsed.path
        query = parse_qs(parsed.query)
        self.server.paths.append((path, query))
        projects = f"/restapi/portal/{PORTAL}/projects/"
        bugs = projects + PROJECT + "/bugs/"
        if path == "/restapi/portals/":
            return self.reply(200, {"portals": [{"id": int(self.server.portal), "name": "Synthetic"}]})
        if path == projects + PROJECT + "/":
            return self.reply(200, {"projects": [{"id": int(PROJECT), "name": "Synthetic"}]})
        if path == projects:
            rows = ([{"id": int(PROJECT), "name": "Synthetic", "status": "active"}]
                    if query["status"] == ["active"] else [{"id_string": "123", "name": "Archive", "status": "archived"}])
            group = "projects"
        elif path == bugs:
            assert query["sort_column"] == ["last_modified_time"] and query["sort_order"] == ["ascending"]
            rows = sorted(self.server.issues, key=lambda row: (row["last_modified_time_long"], row["id"]))
            if "last_modified_time" in query:
                rows = [r for r in rows if r["last_modified_time_long"] > int(query["last_modified_time"][0])]
            rows = [dict(row) for row in rows]
            for row in rows:
                if row["id"] == FIRST:
                    row.pop("description", None)
            group = "bugs"
        elif path.startswith(bugs) and path.endswith("/"):
            rows = [r for r in self.server.issues if str(r["id"]) == path[len(bugs):-1]]
            return self.reply(200, {"bugs": rows})
        else:
            return self.reply(404, {})
        offset, size = int(query["index"][0]) - 1, int(query["range"][0])
        if self.server.fail_page and path == bugs and offset:
            return self.reply(503, {})
        return self.reply(200, {group: rows[offset:offset + size]})

    def do_POST(self):
        raw = self.rfile.read(int(self.headers["content-length"]))
        try:
            assert self.path == "/v1/issue/modify_issue_status"
            assert self.headers["X-ProductId"] == PRODUCT
            assert self.headers["X-ProductKey"] == "synthetic-product-key"
            auth = self.headers["Authorization"]
            canonical, signature = auth.rsplit("&signature=", 1)
            def sign(value):
                digest = hmac.new(SIGN_KEY.encode(), value, hashlib.sha256).hexdigest().encode()
                return quote(base64.b64encode(digest).decode(), safe="")
            assert signature == sign(canonical.encode())
            pieces = canonical.split("&")
            assert len(pieces) == 6 and pieces[0] == "apiID=companyQA" and pieces[1] == f"hashedPayload={sign(raw)}"
            assert pieces[3] == "signMethod=HmacSHA256" and pieces[5] == "version=202100"
            nonce = int(pieces[2].removeprefix("nonce="))
            assert nonce >= 100000 and nonce not in self.server.nonces
            assert abs(time.time() - int(pieces[4].removeprefix("timestamp="))) < 60
            self.server.nonces.add(nonce)
            body = json.loads(raw)
            assert body == {"product_id": PRODUCT, "issue_key": "B-42", "issue_type": "crash",
                            "operator": "ShipForge", "issue_status": "repairing"}
            self.server.signed.append(body)
        except (AssertionError, KeyError, ValueError) as error:
            self.server.errors.append(repr(error))
            return self.reply(403, {"baseRsp": {"code": 1}})
        self.reply(200, {"baseRsp": {"code": int(self.server.fail_ack)}})


def verify(bin_dir, start, stop):
    cli = release.binary(bin_dir, "shipforge")
    server = release.binary(bin_dir, "shipforge-server")
    worker = release.binary(bin_dir, "shipforge-worker")
    upstream = ThreadingHTTPServer(("127.0.0.1", 0), Vendor)
    upstream.issues = [bug(FIRST, "Original"), bug(FIRST + 1, "Second")]
    upstream.portal, upstream.paths, upstream.token = PORTAL, [], "synthetic-zoho"
    upstream.signed, upstream.nonces, upstream.errors = [], set(), []
    upstream.fail_page = upstream.fail_ack = False
    thread = threading.Thread(target=upstream.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="shipforge-vendors-") as directory:
            root = Path(directory)
            data = root / "server"
            key = json.loads(subprocess.check_output([str(server), "--data-dir", str(data), "token", "issue",
                             "--actor", "owner", "--scope", "write"], encoding="utf-8"))["token"]
            env = {k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
            p, addr = start(server, data, "127.0.0.1:0", env=env)
            try:
                base = "http://" + addr
                env.update(SHIPFORGE_DATA_DIR=str(root / "client"), SHIPFORGE_SERVER=base, SHIPFORGE_TOKEN=key,
                           SHIPFORGE_VENDOR_LOOPBACK=f"http://127.0.0.1:{upstream.server_port}",
                           SHIPFORGE_BUGLY_API_ID="companyQA", SHIPFORGE_BUGLY_API_KEY=SIGN_KEY)
                env["SHIPFORGE_ZOHO_TOKEN_" + PORTAL] = "synthetic-zoho"
                env["SHIPFORGE_BUGLY_PRODUCT_KEY_" + PRODUCT] = "synthetic-product-key"
                def call(op, *, ok=True, token=key, **kw):
                    result = subprocess.run([str(cli), "request"], input=json.dumps({"op":op, **kw}),
                                   text=True, encoding="utf-8", capture_output=True, timeout=20,
                                   env=dict(env, SHIPFORGE_TOKEN=token))
                    reply = json.loads(result.stdout)
                    assert reply["ok"] == ok, (op, reply, result.stderr)
                    return reply["data"] if ok else reply["error"]
                opener = build_opener(ProxyHandler({}))
                def wire(url, data=None, token=None):
                    assert urlsplit(url).hostname == "127.0.0.1"
                    headers = {"Content-Type":"application/json"}
                    if token:
                        headers["Authorization"] = "Bearer " + token
                    request = Request(url, json.dumps(data).encode() if data is not None else None, headers=headers)
                    try:
                        result = opener.open(request, timeout=6)
                    except HTTPError as error:
                        result = error
                    with result:
                        body = result.read().decode()
                        return result.status, json.loads(body) if result.headers.get_content_type() == "application/json" else body

                def connector(id, **fields):
                    return call("create", kind="connector", id=id, title=id, fields={"enabled":True, **fields})

                call("create",kind="member",id="reviewer",title="Reviewer",fields={"role":"viewer"})
                viewer_credential = call("access.issue", actor="reviewer", scope="write")
                reader = viewer_credential["token"]
                restricted = call("access.issue", actor="owner", scope="read")["token"]
                connector("zo", provider="zoho", portal_id=PORTAL, zoho_project=PROJECT)
                call("create",kind="space",id="other",title="Other")
                call("create",space="other",kind="connector",id="zo",title="Other binding",
                     fields={"provider":"zoho","portal_id":PORTAL,"zoho_project":PROJECT,"enabled":True})
                assert call("zoho.projects",id="zo",ok=False,token=reader)["code"] == "forbidden"
                assert not upstream.paths
                call("zoho.credential",id="zo",access_token="bad",ok=False)
                assert call("zoho.credential",id="zo",access_token="synthetic-zoho")=={"saved":True}
                env.pop("SHIPFORGE_ZOHO_TOKEN_"+PORTAL)
                projects = call("zoho.projects",id="zo",page_size=1)["projects"]
                assert {r["id"] for r in projects} == {PROJECT,"123"}
                upstream.token="rotated-synthetic-zoho"
                call("zoho.projects",id="zo",ok=False)
                assert call("zoho.credential",id="zo",access_token=upstream.token)=={"saved":True}
                if os.name=="nt":
                    assert all(upstream.token.encode() not in path.read_bytes() for path in (root/"client"/"vendor-credentials").glob("*.json"))
                upstream.portal = "411"
                assert call("zoho.preview",id="zo",ok=False)["code"] == "forbidden"
                upstream.portal = PORTAL
                upstream.fail_page = True
                call("zoho.preview",id="zo",page_size=1,ok=False)
                assert not call("imports.history",id="zo")["items"]
                upstream.fail_page = False
                first = call("zoho.preview",id="zo",page_size=1)
                assert first["counts"]["created"] == 2 and not call("list",kind="task")
                assert any(path.endswith(f"/bugs/{FIRST}/") for path,_ in upstream.paths)
                assert call("imports.commit",id="zo",batch=first["batch"],token=restricted,ok=False)["code"]=="forbidden"
                stale = call("zoho.preview",id="zo",page_size=1)
                saved = call("imports.commit",id="zo",batch=first["batch"])
                assert call("imports.commit",id="zo",batch=first["batch"]) == saved
                assert call("imports.commit",id="zo",batch=stale["batch"],ok=False)["code"]=="conflict"
                assert call("imports.rollback",id="zo",batch=stale["batch"])["state"]=="cancelled"
                rows = {row["fields"]["external_id"]:row for row in call("list",kind="task")}
                one = rows[str(FIRST)]
                assert one["fields"]["priority"]=="high" and one["fields"]["status"]=="in_progress"
                assert one["fields"]["description"]=="upstream original" and one["fields"]["source_assignees"]==["External engineer"]
                assert one["fields"]["assignees"]==[] and one["fields"]["source_reporter_id"]=="483"
                assert call("zoho.preview",space="other",id="zo",ok=False)["code"]=="conflict"
                local = call("create",kind="task",title="Independent local task")
                call("update",kind="task",id=one["id"],expected_rev=one["rev"],title="Local title")
                upstream.issues[0].update(title="Remote competing title",description="Safely updated",last_modified_time_long=5001)
                upstream.issues[1]["description"]="Second updated"
                upstream.issues.append(bug(FIRST+2,"Newly reported",modified=5001))
                update = call("zoho.preview",id="zo",page_size=1)
                assert update["since"]==4999 and update["counts"]["conflict"]==1 and update["counts"]["created"]==1
                assert any(query.get("last_modified_time")==["4999"] for _,query in upstream.paths)
                rebased=call("imports.commit",id="zo",batch=update["batch"])
                assert rebased["counts"]["updated"]==1
                get=lambda id:call("get",kind="task",id=id,allow_cached=False)
                assert get(one["id"])["title"]=="Local title" and get(one["id"])["fields"]["description"]=="Safely updated"
                again=call("zoho.preview",id="zo",full=True)
                assert again["counts"]["created"]==0 and again["counts"]["unchanged"]==2
                call("imports.commit",id="zo",batch=again["batch"])
                assert call("imports.rollback",id="zo",batch=again["batch"])["deleted"]==0
                assert call("imports.rollback",id="zo",batch=update["batch"],token=reader,ok=False)["code"]=="forbidden"
                assert call("imports.rollback",id="zo",batch=update["batch"])["deleted"]==1
                assert get(rows[str(FIRST+1)]["id"])["fields"]["description"]=="Second updated"
                assert get(local["id"])["title"]=="Independent local task"
                assert call("imports.rollback",id="zo",batch=first["batch"])["deleted"]==2
                assert call("imports.rollback",id="zo",batch=first["batch"])["deleted"]==2
                assert call("zoho.preview",space="other",id="zo")["counts"]["created"]==3

                connector("b",provider="bugly",product_id=PRODUCT,default_task_type="bug",bugly_outbound=True,
                          mapping={"priority":{"P1":"urgent"},"status":{"in_progress":"repairing"}})
                assert call("bugly.credentials",id="b",api_id="companyQA",api_key=SIGN_KEY,product_key="synthetic-product-key")=={"saved":True}
                for k in ("SHIPFORGE_BUGLY_API_ID","SHIPFORGE_BUGLY_API_KEY","SHIPFORGE_BUGLY_PRODUCT_KEY_"+PRODUCT):
                    env.pop(k)
                call("bugly.rotate", id="b", token=reader, ok=False)
                old = call("bugly.rotate",id="b")["url"]
                url = call("bugly.rotate",id="b")["url"]
                submission={"ProductID":PRODUCT,"IssueKey":"B-42","ProjectName":"Mobile QA","IssueType":"crash",
                            "FieldValues":{"title":"Synthetic <crash>","description":"Captured on our local fixture",
                                           "priority":"P1","reporter":"outside","current_owner":"guest","deadline":"1天"}}
                assert wire(old,submission)[0]==401 and wire(url.replace("key=","key=x"),submission)[0]==401
                assert wire(url,dict(submission,ProductID="wrong"))[0]==403
                with ThreadPoolExecutor(4) as pool:
                    replies=list(pool.map(lambda _:wire(url,submission), range(4)))
                assert all(code==200 and body["code"]==0 for code,body in replies)
                link=replies[0][1]["url"]
                assert all(body["url"]==link for _,body in replies)
                assert wire(url,dict(submission,FieldValues={"title":"Changed delivery"}))[1]["url"]==link
                assert wire(link)[0]==401 and "&lt;crash&gt;" in wire(link,token=key)[1]
                task=get(link.rsplit("/",1)[1])
                assert task["title"]=="Synthetic <crash>" and task["fields"]["priority"]=="urgent" and task["fields"]["type"]=="bug"
                assert task["fields"]["source_assignees"]==["guest"] and task["fields"]["assignees"]==[]
                call("update",kind="task",id=task["id"],expected_rev=task["rev"],fields={"status":"in_progress"})
                assert call("bugly.outbox",id="b")["items"][0]["state"]=="pending"
                upstream.fail_ack=True
                call("bugly.deliver",id="b",ok=False)
                assert call("bugly.outbox",id="b")["items"][0]["state"]=="pending"
                upstream.fail_ack=False
                done=subprocess.run([str(worker),"bugly-deliver","--connector","b"],env=env,text=True,encoding="utf-8",capture_output=True,timeout=12)
                assert done.returncode==0 and json.loads(done.stdout)["sent"]==1,(done.stdout,done.stderr,upstream.errors)
                assert call("bugly.deliver",id="b")["sent"]==0
                assert len(upstream.signed)==2 and len(upstream.nonces)==2 and not upstream.errors
                assert call("bugly.outbox",id="b")["items"][0]["attempts"]==2
                c=call("get",kind="connector",id="b")
                t=get(task["id"])
                t=call("update",kind="task",id=t["id"],expected_rev=t["rev"],fields={"status":"todo"})
                call("update",kind="task",id=t["id"],expected_rev=t["rev"],fields={"status":"in_progress"})
                call("update",kind="connector",id="b",expected_rev=c["rev"],fields={"bugly_outbound":False})
                assert call("bugly.outbox",id="b")["items"][0]["state"]=="cancelled"
                call("bugly.revoke",id="b")
                assert call("bugly.credentials",id="b",clear=True)=={"saved":False}
                assert call("zoho.credential",id="zo",clear=True)=={"saved":False}
                assert wire(url,submission)[0]==401
                call("access.revoke",id=viewer_credential["id"])
                assert call("imports.history",id="zo",token=reader,ok=False)["code"]=="unauthorized"
            finally:
                stop(p)
    finally:
        upstream.shutdown()
        upstream.server_close()
        thread.join(timeout=3)
    print("Verified native Zoho discovery, pagination, incremental merge and batch rollback, and signed Bugly writeback using loopback HTTP.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    team=import_module("verify-team")
    verify(parser.parse_args().directory.resolve(),team.start,team.stop)
