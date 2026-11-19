"""Run the shipped WeCom Agent adapter, worker and team service with synthetic users."""
import argparse
import base64
import json
import os
from importlib import import_module
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from unittest.mock import patch
from wecom_fixture import WeCom

team,release=import_module("verify-team"),import_module("verify-release")


def verify(bin_dir):
    server,cli,worker=(release.binary(bin_dir,n) for n in ("shipforge-server","shipforge","shipforge-worker"))
    with WeCom() as upstream, tempfile.TemporaryDirectory(prefix="sf-wecom-") as tmp, patch.dict(os.environ,{"SHIPFORGE_WECOM_LOOPBACK":upstream.base}):
        root=Path(tmp)
        threading.Thread(target=upstream.serve_forever,daemon=True).start()
        keys={a:json.loads(subprocess.check_output([server,"--data-dir",root/"team","token","issue","--scope","write","--actor",a],encoding="utf-8"))["token"] for a in ["owner","alice","bob"]}
        service,address=team.start(server,root/"team","127.0.0.1:0")
        try:
            for a,key in keys.items():team.connect(cli,root/a,address,key)
            def call(op,actor="alice",ok=True,**kwargs):
                return release.run_cli(cli,root/actor,"request",request={"op":op,**kwargs},ok=ok)
            for a in ["alice","bob"]:
                call("create","owner",kind="member",id=a,title=a,fields={"role":"member"})
            for a in ["helper","spare"]:
                call("create",kind="agent",id=a,title=a,fields={"runtime":"test"})
            machine=dict(machine_id="wecom-worker",machine_key="wecom-test-local-machine-key-long-enough",)
            call("machine.register",**machine,title="WeCom test",runtimes=["test"])
            creds=dict(agent="helper",bot_id="wecom-test",secret="isolated-wecom-test-secret")
            bad=call("bot.wecom.connect",ok=False,**dict(creds,secret="bad"))
            assert "authorization" in bad["message"] and not call("bot.bindings")["items"]
            # The gateway delivers waiting callbacks immediately after a successful subscription.
            upstream.push("direct","first private turn")
            binding=call("bot.wecom.connect",**creds)
            assert binding["state"]=="connected" and "secret" not in binding and binding["provider"]=="wecom"
            bid=binding["id"]
            assert call("bot.bindings","bob")["items"]==[]
            directory=call("bot.bindings","bob",available_for="wecom")
            assert directory["occupied_agents"]==[{"id":"helper","title":"helper"}]
            for op in ("bot.policy","bot.detach"):
                assert call(op,"bob",False,id=bid)["code"]=="not_found"
            assert [a["id"] for a in call("bot.bindings",available_for="wecom")["available_agents"]]==["spare"]
            before=len(upstream.auth)
            assert call("bot.wecom.connect","bob",False,**dict(creds,agent="spare"))["code"]=="conflict"
            assert len(upstream.auth)==before
            assert call("bot.wecom.event",ok=False,id=bid,device_key="x"*32,bot_id="wecom-test")["code"]=="forbidden"
            upstream.push("noise","foreign bot",bot="another")
            upstream.push("noncallback","other event",cmd="aibot_event_callback",group="group-ww")
            upstream.push("group","group mention",group="group-ww")
            def sync():
                return call("bot.sync",id=bid)
            out=subprocess.check_output([worker,"bot-sync","--directory",root/"alice","--space","default","--binding",bid],encoding="utf-8",timeout=25)
            assert json.loads(out)["received"]==2,out
            upstream.push("direct","first private turn")
            upstream.push("group","group mention",group="group-ww")
            assert sync()["received"]==0
            chats=call("list",kind="chat")
            assert len(chats)==2 and all(c["fields"]["visibility"]=="private" and c["fields"]["external_provider"]=="wecom" for c in chats)
            assert all(call("get","bob",False,kind="chat",id=c["id"])["code"]=="not_found" for c in chats)
            def finish(body,files=None):
                job=call("run.claim",**machine)
                assert job and job["run"]["fields"]["chat"]
                call("run.report",**machine,id=job["run"]["id"],lease=job["lease"],status="done",result=body,artifacts=files or [])
                return job
            returned=b"engine result"*50000
            finish("direct result")
            finish("group result",[{"name":"report.dat","filename":"report.dat","content_base64":base64.b64encode(returned).decode()}])
            assert sync()["outgoing"]==3
            assert ("aibot_respond_msg","incoming-direct") in [(r[0],r[1]) for r in upstream.replies]
            assert ("aibot_respond_msg","incoming-group") in [(r[0],r[1]) for r in upstream.replies]
            assert any(x[2].get("chatid")=="group-ww" and x[2]["msgtype"]=="file" for x in upstream.replies)
            uploaded=next(iter(upstream.uploads.values()))
            assert uploaded["meta"]["total_chunks"]==2 and uploaded["bytes"]==returned
            call("bot.policy",id=bid,dm=False,groups=True)
            upstream.push("dm-off","disabled")
            upstream.push("renew","/new fresh group context",group="group-ww")
            assert sync()["received"]==1 and len(call("list",kind="chat"))==3
            # Finish the only scheduled run and inspect the persisted prompt independently.
            job=finish("renewed")
            assert len(job["run"]["fields"]["chat_context"]["messages"])==1
            assert any(r["fields"].get("body")=="fresh group context" for r in call("list",kind="message"))
            assert sync()["outgoing"]==1
            call("bot.policy",id=bid,dm=True,groups=False)
            upstream.push("g-off","disabled",group="group-ww")
            plaintext=b"independent, encrypted source attachment"
            content=upstream.media("in",plaintext,"in.txt")
            upstream.push("media",kind="file",content=content)
            assert sync()["received"]==1
            upstream.push("badmedia",kind="file",content=dict(content,aeskey="invalid"))
            assert sync()["rejected"]==1
            inbound=next(x for x in call("list",kind="message") if x["fields"].get("attachments"))
            fid=inbound["fields"]["attachments"][0]["id"]
            assert call("attachment.download","bob",False,kind="message",id=inbound["id"],attachment_id=fid)["code"]=="not_found"
            job=finish("input accepted")
            file=job["run"]["fields"]["chat_context"]["messages"][-1]["input_files"][0]
            assert file["filename"]=="in.txt" and base64.b64decode(file["content_base64"])==plaintext
            upstream.push("media",kind="file",content=content)
            assert sync()["received"]==0 and upstream.read_media==["in","in"]
            upstream.push("uncertain","uncertain test")
            assert sync()["received"]==1
            finish("exact-once attempt")
            upstream.lose_ack=True
            assert sync()["outgoing"]==0
            pending=call("bot.bindings")["items"][0]["deliveries"]
            assert len(pending)==1 and pending[0]["state"]=="uncertain"
            sync()
            assert len([r for r in upstream.replies if r[1]=="incoming-uncertain"])==1
            call("bot.retry",id=bid,delivery=pending[0]["id"],decision="confirm")
            assert not call("bot.bindings")["items"][0]["deliveries"]
            upstream.push("reset","/new")
            assert sync()["outgoing"]==1 and call("run.claim",**machine) is None
            assert call("list",kind="task")==[]
            upstream.revoked=True
            call("bot.sync",ok=False,id=bid)
            assert call("bot.bindings")["items"][0]["state"]=="expired"
            upstream.revoked=False
            rebound=call("bot.wecom.connect",**creds)["id"]
            assert rebound!=bid
            upstream.push("direct","first private turn")
            assert call("bot.sync",id=rebound)["received"]==0
            before=len(upstream.auth)
            watcher=subprocess.Popen([worker,"bot-sync","--directory",root/"alice","--space","default","--binding",rebound,"--watch"],stdout=subprocess.DEVNULL)
            try:
                until=time.monotonic()+8
                while len(upstream.auth)==before and time.monotonic()<until:
                    time.sleep(.05)
                assert len(upstream.auth)>before
                synced=call("bot.sync",id=rebound)
                assert synced["background"] is True and synced["state"]=="connected" and synced["received"]==0
                assert len(upstream.auth)==before+1
            finally:
                watcher.kill()
                watcher.wait(timeout=5)
            upstream.push("displace",cmd="aibot_event_callback",kind="event",content={"eventtype":"disconnected_event"})
            assert "replaced" in call("bot.sync",ok=False,id=rebound)["message"]
            assert next(b for b in call("bot.bindings")["items"] if b["id"]==rebound)["state"]=="expired"
            rebound=call("bot.wecom.connect",**creds)["id"]
            call("bot.disconnect",id=rebound)
            assert call("bot.sync",ok=False,id=rebound)["code"]=="conflict"
            assert len(call("list",kind="chat"))==4 and not upstream.errors,upstream.errors
            for db in (root/"team").glob("*.sqlite*"):
                assert b"isolated-wecom-test-secret" not in db.read_bytes()
        finally:
            team.stop(service)
            upstream.shutdown()
    print("WeCom subscription, distinct DM/mentions, private runs, files, exact routing, uncertain sends, reconnect and revocation passed.")


if __name__=="__main__":
    p=argparse.ArgumentParser()
    p.add_argument("bin_dir",type=Path)
    verify(p.parse_args().bin_dir.resolve())
