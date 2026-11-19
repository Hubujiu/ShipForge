"""Run mailbox/device confirmation, profile and account closure against isolated real processes."""
import argparse
from contextlib import closing
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import queue
import re
import socket
import sqlite3
import tempfile
import threading
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlencode, urlsplit
from urllib.request import Request, urlopen

release=import_module("verify-release")
team=import_module("verify-team")


def verify(directory):
    mail=queue.Queue()
    class Mailbox(BaseHTTPRequestHandler):
        def log_message(self,*_): pass
        def do_POST(self):
            assert self.path=="/emails" and self.headers["Authorization"]=="Bearer loopback-key"
            message=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert self.headers["Idempotency-Key"] and len(message["to"])==1
            link=re.search(r"http://[^\s]+",message["text"]).group()
            if urlsplit(link).path == "/identity/approve":
                mail.put((message["to"][0],link))
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"id":"local-mail-receipt"}')

    with tempfile.TemporaryDirectory(prefix="shipforge-account-") as temporary, ThreadingHTTPServer(("127.0.0.1",0),Mailbox) as gateway:
        threading.Thread(target=gateway.serve_forever,daemon=True).start()
        with socket.socket() as port:
            port.bind(("127.0.0.1",0))
            port=port.getsockname()[1]
        address=f"127.0.0.1:{port}"
        url=f"http://{address}"
        env=dict(os.environ,SHIPFORGE_IDENTITY_BASE_URL=url,SHIPFORGE_IDENTITY_MAIL_URL=f"http://127.0.0.1:{gateway.server_port}/emails",
                 SHIPFORGE_IDENTITY_MAIL_FROM="team@example.test",SHIPFORGE_IDENTITY_MAIL_TOKEN="loopback-key")
        root=Path(temporary)
        cli=release.binary(directory,"shipforge")
        service,addr=team.start(release.binary(directory,"shipforge-server"),root/"team",address,env=env)
        assert addr==address
        a,b,other=[root/p for p in ("laptop","desktop","colleague")]
        email="synthetic@example.test"
        def op(device,action,ok=True,**fields):
            return release.run_cli(cli,device,"request",request=dict(fields,op=action),ok=ok)
        def authorize(challenge,recipient,check_pending=None):
            assert "device" not in challenge and len(challenge["pair"])==8
            to,link=mail.get(timeout=5)
            assert to==recipient
            with urlopen(link,timeout=5) as r:
                assert r.headers["Cache-Control"]=="no-store" and "配对码" in r.read().decode()
            if check_pending: check_pending()
            params={k:v[0] for k,v in parse_qs(urlsplit(link).query).items()}
            route=url+"/identity/approve"
            def post(pair):
                return urlopen(Request(route,data=urlencode(dict(params,pair=pair)).encode()),timeout=5)
            try:
                post("WRONG")
                raise AssertionError("incorrect pairing code accepted")
            except HTTPError as error:
                assert error.code==401
            with post(challenge["pair"]) as response:
                assert response.status==200 and ("shipforge://identity/"+challenge["id"]) in response.read().decode()
        def login(device,address):
            step=op(device,"profiles.login.start",url=url,email=address)
            authorize(step,address,lambda: op(device,"profiles.login.finish",ok=False,id=step["id"]))
            assert op(device,"profiles.verification",id=step["id"])["pair"]==step["pair"]
            return op(device,"profiles.login.finish",id=step["id"])["actor"]
        try:
            actor=login(a,email)
            assert login(b,email)==actor
            home=op(a,"organization.list")[0]["id"]
            assert home!="default"
            assert op(a,"get",ok=False,kind="space",id="default")["code"]=="forbidden"
            before=op(a,"access.account.profile")
            edited=op(a,"access.account.update",expected_rev=before["rev"],name="Test member",bio="example",avatar="https://example.test/image.png")
            assert edited["name"]==op(b,"access.account.profile")["name"]=="Test member"
            assert op(b,"access.account.update",ok=False,expected_rev=before["rev"],name="Stale")["code"]=="conflict"
            assert op(a,"onboarding.get",space=home)["setup_required"] is True
            op(a,"onboarding.setup",space=home,organization_name="Example workspace",organization_slug="example-team",work_type="software",task_types=["task"])
            initial_guide=op(b,"onboarding.get",space=home)
            assert not initial_guide["setup_required"] and initial_guide["max_points"]==560
            other_actor=login(other,"colleague@example.test")
            profile_id=op(other,"profiles.list")["active"]
            session_id=op(other,"access.whoami")["token_id"]
            with closing(sqlite3.connect(root/"team"/"identities.sqlite3")) as db, db:
                db.execute("UPDATE identity_sessions SET expires=unixepoch()-1 WHERE token_id=?",(session_id,))
            assert op(other,"profiles.use",ok=False,id=profile_id)["code"]=="unauthorized"
            wrong=op(other,"profiles.login.start",account=profile_id,url=url,email="unrelated@example.test")
            authorize(wrong,"unrelated@example.test")
            assert op(other,"profiles.login.finish",ok=False,id=wrong["id"])["code"]=="unauthorized"
            relogin=op(other,"profiles.login.start",account=profile_id,url=url,email="colleague@example.test")
            authorize(relogin,"colleague@example.test")
            assert op(other,"profiles.login.finish",id=relogin["id"])["actor"]==other_actor
            assert op(other,"profiles.list")["active"]==profile_id
            colleague_home=op(other,"organization.list")[0]["id"]
            invitation=op(other,"invitation.issue",space=colleague_home,channel="email",email=email,role="member")
            op(a,"invitation.accept",link=invitation["link"])
            op(a,"create",space=home,kind="member",id=other_actor,title="Colleague",fields={"role":"member"})
            task=op(a,"create",space=home,kind="task",title="Dissolved with example organization")
            guide=op(b,"onboarding.get",space=home)
            assert "create_task" in guide["completed"] and guide["points"]>initial_guide["points"]
            op(b,"create",space=home,kind="task",title="A second task from the other device")
            assert op(a,"onboarding.get",space=home)["points"]==guide["points"]
            assert "create_task" not in op(other,"onboarding.get",space=home)["completed"]
            retained=op(other,"create",space=colleague_home,kind="task",title="Retained organization task")
            pat=op(b,"access.personal.issue",space=home,label="Example",lifetime="30d")
            team.connect(cli,root/"pat",addr,pat["token"])
            impact=op(a,"access.account.impact")
            assert {o["action"] for o in impact["organizations"]}=={"dissolve_with_members","leave"}
            challenge=op(a,"profiles.close.start")
            authorize(challenge,email)
            machine={"space":home,"machine_id":"shared-box","machine_key":"example-machine-key-of-thirty-two-bytes","title":"Example machine","runtimes":["inspect"]}
            op(a,"machine.register",**machine,shared=True)
            assert op(a,"access.account.impact")["shared_machines"][0]["id"]=="shared-box"
            assert op(a,"profiles.close.finish",ok=False,id=challenge["id"],confirmation="注销",dissolve=True)["code"]=="conflict"
            op(a,"machine.register",**machine,shared=False)
            assert op(b,"profiles.close.finish",ok=False,id=challenge["id"],confirmation="注销")["code"]=="not_found"
            assert op(a,"profiles.close.finish",ok=False,id=challenge["id"],confirmation="错",dissolve=True)["code"]=="invalid"
            assert op(a,"profiles.close.finish",ok=False,id=challenge["id"],confirmation="注销",dissolve=False)["code"]=="conflict"
            assert op(a,"profiles.close.finish",id=challenge["id"],confirmation="注销",dissolve=True)["closed"]
            assert op(b,"access.whoami",ok=False)["code"]=="unauthorized"
            assert op(root/"pat","access.whoami",ok=False)["code"]=="unauthorized"
            assert op(other,"get",space=home,kind="task",id=task["id"],ok=False)["code"] in ("forbidden","not_found")
            assert op(other,"get",space=colleague_home,kind="task",id=retained["id"])["id"]==retained["id"]
            new=login(a,email)
            assert new!=actor and op(a,"access.account.profile")["bio"]==""
            assert {o["id"] for o in op(a,"organization.list")}.isdisjoint({home,colleague_home})
            assert op(a,"profiles.logout")["logged_out"]
            assert op(a,"access.whoami")["mode"]=="local"
        finally:
            team.stop(service)
            gateway.shutdown()
    print("Verified email/browser device pairing, two sessions, editable identity, addressed invitation, organization dissolution, fresh registration and immediate token revocation.")

if __name__=="__main__":
    args=argparse.ArgumentParser()
    args.add_argument("directory",type=Path)
    verify(args.parse_args().directory.resolve())
