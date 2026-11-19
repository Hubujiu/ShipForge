"""Local Slack signing, multi-profile team service and a real external Runtime subprocess."""
import argparse
from hashlib import sha256
import hmac
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener

release = import_module("verify-release")
team = import_module("verify-team")


def verify(directory):
    cli = release.binary(directory, "shipforge")
    server = release.binary(directory, "shipforge-server")
    worker = release.binary(directory, "shipforge-worker")
    with tempfile.TemporaryDirectory(prefix="shipforge-space-assistant-") as d:
        root = Path(d)
        data = root / "service"
        secret = "synthetic-signing-secret"
        def token(actor, scope, *extra):
            raw = subprocess.check_output([server, "--data-dir", data, "token", "issue", "--actor", actor,
                                           "--scope", scope, *extra], encoding="utf-8", timeout=15)
            return json.loads(raw)["token"]
        keys = {"admin":token("owner","write"), "member":token("alice","write"), "read":token("owner","read")}
        hook = token("owner","hook","--resource","connector","--space","default","--binding","slack-input")
        config = root / "slack.json"
        config.write_text(json.dumps([{"space":"default","connector":"slack-input","workspace_id":"T1","channel_id":"C1",
                                      "app_id":"A1","signing_secret_env":"SF_ASSISTANT_SIGN","hook_token_env":"SF_ASSISTANT_HOOK"}]),encoding="utf-8")
        env = {k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
        process,address = team.start(server,data,"127.0.0.1:0",extra=("--slack-config",str(config)),
                                     env={**env,"SF_ASSISTANT_SIGN":secret,"SF_ASSISTANT_HOOK":hook})
        def call(who,op,*,ok=True,**fields):
            return release.run_cli(cli,root/who,"request",request={"op":op,**fields},ok=ok)
        def post(event, key=secret):
            body=json.dumps(event,ensure_ascii=False,separators=(",",":")).encode()
            ts=str(int(time.time()))
            signature=hmac.new(key.encode(),b"v0:"+ts.encode()+b":"+body,sha256).hexdigest()
            req=Request("http://"+address+"/v1/slack/default/slack-input",data=body,
                        headers={"Content-Type":"application/json","X-Slack-Request-Timestamp":ts,"X-Slack-Signature":"v0="+signature})
            try:
                with build_opener(ProxyHandler({})).open(req,timeout=10) as response:
                    return response.status,json.load(response)
            except HTTPError as e:
                return e.code,None
        def event(i,text,sender="U1",thread=None):
            e={"type":"app_mention" if thread is None else "message","user":sender,"channel":"C1",
               "ts":f"8.{i:06}","text":text}
            if thread: e["thread_ts"]=thread
            return {"type":"event_callback","team_id":"T1","api_app_id":"A1","event_id":f"Ev{i}","event":e}
        def host(*args):
            result=subprocess.run([worker,*args],env={**env,"SHIPFORGE_DATA_DIR":str(root/"admin")},capture_output=True,
                                  encoding="utf-8",timeout=40)
            assert result.returncode==0,(result.stdout,result.stderr)
        try:
            for name,key in keys.items():team.connect(cli,root/name,address,key)
            call("admin","create",kind="member",id="alice",title="Alice",fields={"role":"member"})
            call("admin","create",kind="connector",id="slack-input",title="Local Slack",
                 fields={"provider":"slack","enabled":True,"outbound":True,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1"})
            options=dict(provider="slack",expected_rev=0,connector="slack-input",runtime="fixture",sender_map={"U1":"alice"})
            call("member","task_assistant.bind",ok=False,**options)
            call("read","task_assistant.bind",ok=False,**options)
            assistant=call("admin","task_assistant.bind",**options)
            assert len(call("member","task_assistant.catalog"))==5
            assert call("admin","list",kind="agent")==[]
            assert post(event(1,"create a task"),"incorrect-key")[0]==401
            assert post(event(1,"please do not create a task"))[0]==200
            assert post(event(2,"create a task","U-unknown"))[0]==200
            assert call("admin","task_assistant.history",provider="slack")==[]
            req=event(3,"<@Ubot> create a task to test Slack\n```rust\ncheck();\n```")
            assert post(req)==(200,{"ok":True}) and post(req)==(200,{"ok":True})
            assert len(call("admin","task_assistant.history",provider="slack"))==1
            runtime=root/"runtime.py"
            runtime.write_text("import json,sys\nf=json.load(sys.stdin)['fields']\n"
                               "a=f['external_request']['intent']\n"
                               "assert f['handler_kind']=='im_assistant' and f['external_request']['actor']=='alice'\n"
                               "if a=='create': assert 'check();' in f['external_body']\n"
                               "if a=='update': assert len(f['external_thread'])==1\n"
                               "print(json.dumps({'status':'done','result':'Completed','task_draft':"
                               "{'action':a,'title':'Prepared by runtime' if a=='create' else 'Revised by runtime','description':'Scoped draft'}}))\n",encoding="utf-8")
            cfg=root/"worker.json"
            host("init","--config",cfg,"--program",sys.executable,"--runtime","fixture","--arg",runtime)
            host("run","--config",cfg,"--once")
            [task]=call("member","list",kind="task")
            assert task["title"]=="Prepared by runtime" and task["fields"]["created_by_assistant"]["assistant"]==assistant["id"]
            thread=req["event"]["ts"]
            assert post(event(4,"update this task",thread=thread))[0]==200
            host("run","--config",cfg,"--once")
            assert call("member","get",kind="task",id=task["id"])["title"]=="Revised by runtime"
            for _ in range(2):
                item=call("admin","im.claim",id="slack-input")
                assert item["thread"]["channel"]=="C1" and item["thread"]["thread"]==thread
                call("admin","im.ack",id="slack-input",delivery_id=item["delivery_id"],lease=item["lease"],
                     outcome="sent",remote_id=f"9.00000{_+1}")
            call("admin","task_assistant.unbind",provider="slack",expected_rev=assistant["rev"])
            assert post(event(5,"create another task"))[0]==200
            assert len(call("member","list",kind="task"))==1
        finally:
            team.stop(process)
    print("Verified signed Slack task assistant, real Runtime create/update, original-thread receipts and revocation")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    verify(parser.parse_args().directory.resolve())
