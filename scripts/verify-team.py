"""Black-box verification of the packaged server and two independent desktop/CLI profiles."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
from urllib.error import HTTPError
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).parent))
from importlib import import_module
release = import_module("verify-release")


def start(server, data, address, *, extra=(), env=None):
    if env is None:
        env = {k: v for k, v in os.environ.items() if k not in ("SHIPFORGE_SLACK_CONFIG","SHIPFORGE_GITHUB_WEBHOOKS","SHIPFORGE_JIRA_ORGANIZATION","SHIPFORGE_JIRA_BASE_URL")}
    p = subprocess.Popen([str(server), "--data-dir", str(data), "serve", "--listen", address, *extra],
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", env=env)
    ready = queue.Queue()
    threading.Thread(target=lambda: ready.put(p.stdout.readline()), daemon=True).start()
    try:
        event = json.loads(ready.get(timeout=15))
        return p, event["listen"]
    except BaseException:
        p.kill()
        _,err = p.communicate(timeout=8)
        if err:
            print(err, file=sys.stderr)
        raise


def stop(p):
    p.terminate()
    try:
        p.communicate(timeout=8)
    except subprocess.TimeoutExpired:
        p.kill()
        p.communicate(timeout=8)


def connect(cli, profile, address, token):
    env = {k:v for k,v in os.environ.items() if k not in ("SHIPFORGE_TOKEN","SHIPFORGE_SERVER","SHIPFORGE_DATA_DIR")}
    result = subprocess.run([str(cli),"--data-dir",str(profile),"connect","http://"+address,"--token-stdin"],
                      input=token,text=True,encoding="utf-8",capture_output=True,env=env,timeout=15)
    assert result.returncode==0, result.stdout + result.stderr
    if os.name == "nt":
        assert token not in (profile/"connection.json").read_text(encoding="utf-8")


def verify(bin_dir):
    cli=release.binary(bin_dir,"shipforge")
    server=release.binary(bin_dir,"shipforge-server")
    with tempfile.TemporaryDirectory(prefix="shipforge-team-") as directory:
        root=Path(directory)
        data,admin,reader=(root/k for k in ("server","admin","reader"))
        key=json.loads(subprocess.check_output([str(server),"--data-dir",str(data),"token","issue","--actor","owner","--scope","write"],encoding="utf-8"))["token"]
        p,address=start(server,data,"127.0.0.1:0")
        def command(profile,op,ok=True,**kw):
            return release.run_cli(cli,profile,"request",request={"op":op,**kw},ok=ok)
        try:
            connect(cli,admin,address,key)
            assert command(admin,"access.whoami")["actor"]=="owner"
            command(admin,"create",kind="connector",id="qa-gitlab",title="GitLab QA",fields={"provider":"gitlab","enabled":True})
            hook=command(admin,"access.issue",actor="owner",scope="hook",space="default",resource="connector",binding="qa-gitlab")
            fixtures=Path(__file__).resolve().parents[1]/"crates/shipforge-core/tests/fixtures/inbound.json"
            payload=next(case["payload"] for case in json.loads(fixtures.read_text(encoding="utf-8")) if case["provider"]=="gitlab")
            def webhook(route, data, headers):
                request=Request("http://"+address+route,data=json.dumps(data).encode(),headers=headers)
                try:
                    response=urlopen(request,timeout=10)
                except HTTPError as error:
                    response=error
                with response:
                    return response.status,json.load(response)
            headers={"X-Gitlab-Token":hook["token"],"X-Gitlab-Event-UUID":"packaged-hook-1"}
            route="/v1/integrations/default/qa-gitlab"
            code,body=webhook(route,payload,headers)
            assert code==200 and body["data"]["action"]=="created",body
            assert command(admin,"get",kind="task",id=body["data"]["task"])["title"]=="Fix desktop zoom"
            code,replayed=webhook(route,payload,headers)
            assert code==200 and replayed["data"]["task"]==body["data"]["task"],replayed
            assert webhook("/v1/integrations/default/other",payload,headers)[0]==403
            assert webhook("/v1/command",{"op":"list","kind":"task"},{"Authorization":"Bearer "+hook["token"]})[0]==403
            command(admin,"create",kind="agent",id="hook-agent",title="Hook agent",fields={})
            rule_fields={"handler_kind":"agent","handler_id":"hook-agent","instruction":"Record this event","enabled":True,
                         "triggers":[{"type":"webhook","enabled":False}]}
            command(admin,"create",kind="automation",id="qa-rule",title="Hook QA",fields=rule_fields)
            auth_hook=command(admin,"access.issue",actor="owner",scope="hook",space="default",resource="automation",binding="qa-rule")
            hook_headers={"Authorization":"Bearer "+auth_hook["token"]}
            event={"source":"manual","event_id":"delivery-a","payload":{"build":1}}
            assert webhook("/v1/hooks/default/qa-rule",event,hook_headers)[0]>=400
            assert webhook("/v1/command",dict(event,op="automation.fire",space="default",id="qa-rule"),hook_headers)[0]>=400
            command(admin,"update",kind="automation",id="qa-rule",fields={"triggers":[{"type":"webhook","enabled":True}]})
            code,started=webhook("/v1/hooks/default/qa-rule",event,hook_headers)
            assert code==200 and started["data"]["fields"]["source"]=="webhook",started
            assert webhook("/v1/hooks/default/qa-rule",event,hook_headers)[1]["data"]["id"]==started["data"]["id"]
            command(admin,"create",kind="member",id="qa-reader",title="QA reader",fields={"actor":"qa-reader","role":"viewer"})
            token=command(admin,"access.issue",actor="qa-reader",scope="read")
            connect(cli,reader,address,token["token"])
            shared={"kind":"task","title":"Shared task","fields":{"description":"Visible in both profiles"},"request_id":"verify-create-1"}
            row=command(admin,"create",**shared)
            assert command(admin,"create",**shared)["id"]==row["id"]
            assert command(reader,"get",kind="task",id=row["id"])["title"]==row["title"]
            assert command(reader,"create",False,kind="task",title="denied")["code"]=="forbidden"

            def competing(title):
                return subprocess.run([str(cli),"--data-dir",str(admin),"request"],input=json.dumps({"op":"update","kind":"task","id":row["id"],"expected_rev":row["rev"],"title":title}),
                             env={k:v for k,v in os.environ.items() if k not in ("SHIPFORGE_TOKEN","SHIPFORGE_SERVER","SHIPFORGE_DATA_DIR")},
                             text=True,encoding="utf-8",capture_output=True,timeout=20)
            with ThreadPoolExecutor(max_workers=2) as ex:
                responses=[json.loads(r.stdout) for r in ex.map(competing,["Concurrent A","Concurrent B"])]
            assert sorted(r["ok"] for r in responses)==[False,True], responses
            assert next(r for r in responses if not r["ok"])["error"]["code"]=="conflict"
            row=command(admin,"get",kind="task",id=row["id"])
            stop(p);p=None
            cached=command(admin,"get",kind="task",id=row["id"])
            assert cached["rev"]==row["rev"]
            draft=command(admin,"sync.enqueue",request={"op":"update","kind":"task","space":"default","id":row["id"],"expected_rev":row["rev"],"title":"Offline edit"})
            assert draft["queued"] and command(admin,"sync.status")["pending"]==1
            p,_=start(server,data,address)
            flushed=command(admin,"sync.flush")
            assert flushed["pending"]==0 and len(flushed["changes"])==1,flushed
            assert command(reader,"get",kind="task",id=row["id"])["title"]=="Offline edit"
            command(admin,"access.revoke",id=token["id"])
            assert command(reader,"get",False,kind="task",id=row["id"])["code"]=="unauthorized"
            stop(p);p=None
            assert command(reader,"get",False,kind="task",id=row["id"])["code"]=="offline"
        finally:
            if p is not None:stop(p)
    print("Verified member scopes, two clients, request replay, concurrent writes, offline drafts, reconnect and revocation.")
    import_module("verify-slack").verify(bin_dir, start, stop)
    import_module("verify-vcs").verify(bin_dir, start, stop)
    import_module("verify-chat").verify(bin_dir, start, stop)
    import_module("verify-chat-workflows").verify(bin_dir)
    import_module("verify-conversation-tools").verify(bin_dir)
    import_module("verify-work-collaboration").verify(bin_dir)
    import_module("verify-planning").verify(bin_dir)
    import_module("verify-iterations").verify(bin_dir, start, stop)
    import_module("verify-invitations").verify(bin_dir, start, stop, connect)
    import_module("verify-jira").verify(bin_dir, start, stop)
    import_module("verify-vendor-imports").verify(bin_dir, start, stop)
    import_module("verify-tapd").verify(bin_dir, start, stop)
    import_module("verify-trackers").verify(bin_dir, start, stop)
    import_module("verify-cloud-trackers").verify(bin_dir, start, stop)
    import_module("verify-personal-tokens").verify(bin_dir, start, stop, connect)
    import_module("verify-accounts").verify(bin_dir, start, stop, connect)
    import_module("verify-identity").verify(bin_dir)
    import_module("verify-inbox").verify(bin_dir)
    import_module("verify-agent-directory").verify(bin_dir)
    import_module("verify-skill-market").verify(bin_dir)


if __name__=="__main__":
    p=argparse.ArgumentParser()
    p.add_argument("directory",type=Path)
    verify(p.parse_args().directory.resolve())
