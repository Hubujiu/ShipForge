"""Exercise history and unattended execution with a real service, CLI profiles and a child process."""
import argparse
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

release = import_module("verify-release")
team = import_module("verify-team")


def verify(directory):
    cli, server, worker = [release.binary(directory, name) for name in ("shipforge", "shipforge-server", "shipforge-worker")]
    with tempfile.TemporaryDirectory(prefix="shipforge-rule-history-") as tmp:
        root = Path(tmp)
        store = root / "service"
        keys = {actor: json.loads(subprocess.check_output([server, "--data-dir", store, "token", "issue",
                     "--actor", actor, "--scope", scope], encoding="utf-8", timeout=15))["token"]
                for actor, scope in (("owner","write"), ("alice","write"), ("observer","read"))}
        env = {k: v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
        proc, url = team.start(server, store, "127.0.0.1:0", env=env)

        def call(actor, op, ok=True, **fields):
            return release.run_cli(cli, root / actor, "request", request={"op": op, **fields}, ok=ok)

        def host(*args):
            result = subprocess.run([worker, *args], env={**env, "SHIPFORGE_DATA_DIR": str(root / "alice")},
                                    text=True, encoding="utf-8", capture_output=True, timeout=28)
            assert result.returncode==0, (result.stdout, result.stderr)

        try:
            for actor, token in keys.items():
                team.connect(cli, root / actor, url, token)
            for actor in ("alice", "observer"):
                call("owner","create",kind="member",id=actor,title=actor,fields={"role":"member"})
            call("owner","create",kind="agent",id="a",title="Verifier",fields={"runtime":"fixture"})
            fields = {"handler_id":"a","instruction":"Historical release check","enabled":True,"notify":["observer"],
                      "triggers":[{"id":"mention","type":"comment","keyword":"check"}]}
            call("owner","create",kind="automation",id="audit",title="Release check",fields=fields)
            call("owner","create",kind="task",id="t",title="Release",fields={})
            call("alice","create",kind="comment",id="c",title="check",fields={"task":"t","body":"check build 42"})
            [item] = call("alice","automation.history",id="audit")["items"]
            run=item["id"]
            assert (item["trigger_actor"], item["owner"], item["trigger_record"], item["source"])==("alice","alice","c","comment")
            room=call("alice","automation.conversation",id="audit",run=run)["id"]
            call("owner","automation.conversation",False,id="audit",run=run)
            call("observer","chat.history",False,id=room)
            program=root/"child.py"
            program.write_text(
                "import json,sys,time\n"
                "r=json.load(sys.stdin); f=r['fields']\n"
                "assert f['interaction']=={'mode':'unattended','ask_user':False,'request_approval':False}\n"
                "if f.get('automation_id'):\n"
                " assert f['instruction']=='Historical release check' and f['trigger_actor']=='alice' and f['trigger_record']=='c'\n"
                " if f['attempt']==1:\n"
                "  print(json.dumps({'event':'approval_request','question':'Ask before continuing'}),flush=True); time.sleep(120)\n"
                " print(json.dumps({'status':'done','result':'Completed historical check'}))\n"
                "else:\n"
                " assert f['automation_context']['trigger_record']=='c' and any(m['body']=='Completed historical check' for m in f['chat_context']['messages'])\n"
                " print(json.dumps({'status':'done','result':'Follow-up in the same room'}))\n", encoding="utf-8")
            config=root/"worker.json"
            host("init","--config",config,"--program",sys.executable,"--runtime","fixture","--arg",program)
            def run_once():
                host("run","--config",config,"--once")
            start=time.monotonic()
            run_once()
            assert time.monotonic()-start<25
            status=call("alice","automation.status",id="audit")
            assert status["health"]=="attention" and status["latest"]["failure"]["category"]=="unattended_input", status
            [notice]=[v for v in call("observer","list",kind="notification") if v["fields"].get("automation")=="audit"]
            assert notice["fields"]["importance"]=="important" and notice["fields"].get("run") is None
            revised=call("owner","update",kind="automation",id="audit",fields={"instruction":"New rules for future events"})
            call("observer","automation.retry",False,id="audit",run=run)
            call("alice","automation.retry",id="audit",run=run,request_id="same-retry")
            call("alice","automation.retry",id="audit",run=run,request_id="same-retry")
            run_once()
            assert call("alice","get",kind="run",id=run)["fields"]["result"]=="Completed historical check"
            call("alice","chat.send",id=room,body="Explain the check")
            run_once()
            assert any(m["fields"]["body"]=="Follow-up in the same room" for m in call("alice","chat.history",id=room))
            for i in range(4):
                call("alice","automation.fire",id="audit",event_id=f"manual-{i}")
            page=call("alice","automation.history",id="audit",limit=2)
            next_page=call("alice","automation.history",id="audit",limit=2,cursor=page["next_cursor"])
            assert not ({r["id"] for r in page["items"]} & {r["id"] for r in next_page["items"]})
            for action in ("pause","resume","archive"):
                revised=call("owner","automation.lifecycle",id="audit",action=action,expected_rev=revised["rev"])
                if action!="resume":
                    call("alice","automation.fire",False,id="audit",event_id=f"blocked-{action}")
            assert call("alice","automation.status",id="audit")["state"]=="archived"
        finally:
            team.stop(proc)
    print("Verified automation history, owner-scoped continuation, live unattended failure, historical retry and rule lifecycle")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
