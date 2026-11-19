"""Exercise the released CLI, worker and team service against two synthetic Feishu/Lark clouds."""
import argparse
import base64
import gzip
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from unittest.mock import patch
from urllib.parse import urlsplit, parse_qs
from lark_fixture import Cloud

team, release = import_module("verify-team"), import_module("verify-release")


def verify(bin_dir):
    server, cli, worker = (release.binary(bin_dir, name) for name in ("shipforge-server", "shipforge", "shipforge-worker"))
    with Cloud("feishu") as cn, Cloud("lark") as intl, tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for upstream in [cn, intl]:
            threading.Thread(target=upstream.serve_forever, daemon=True).start()
        with patch.dict(os.environ, {"SHIPFORGE_FEISHU_LOOPBACK": cn.base, "SHIPFORGE_LARK_LOOPBACK": intl.base}):
            def call(op, *, actor="alice", ok=True, **kw):
                return release.run_cli(cli, root/actor, "request", request={"op": op, **kw}, ok=ok)

            keys = {a:json.loads(subprocess.check_output([server, "--data-dir", root/"team", "token", "issue", "--scope", "write", "--actor", a], encoding="utf-8"))["token"] for a in ["owner", "alice", "bob"]}
            process, addr = team.start(server, root/"team", "127.0.0.1:0")
            machine = dict(machine_id="lark-worker", machine_key="synthetic-local-machine-long-adapter-key")

            def finish(body, artifacts=None):
                job = call("run.claim", **machine)
                assert job and job["run"]["fields"]["chat"]
                call("run.report", **machine, id=job["run"]["id"], lease=job["lease"], status="done", result=body, artifacts=artifacts or [])
                return job

            def start(brand, actual):
                cn.next_brand = actual
                qr = call("bot.qr.start", provider=brand, agent="helper")
                addr = urlsplit(qr["qr_content"])
                assert addr.hostname == ("open.larksuite.com" if brand == "lark" else "open.feishu.cn")
                params = parse_qs(addr.query)
                addons = json.loads(gzip.decompress(base64.urlsafe_b64decode(params["addons"][0] + "===")))
                assert "im.message.receive_v1" in addons["events"]["items"]["tenant"] and qr["qr_matrix"]
                assert "device-" not in str(qr) and "client_secret" not in str(qr)
                return qr["id"]

            try:
                for a, key in keys.items():
                    team.connect(cli, root/a, addr, key)
                for a in ["alice", "bob"]:
                    call("create", actor="owner", kind="member", id=a, title=a, fields={"role":"member"})
                call("create", kind="agent", id="helper", title="Agent", fields={"runtime":"test"})
                call("machine.register", **machine, title="Synthetic machine", runtimes=["test"])
                assert call("bot.qr.check", id=start("feishu", "denied"))["state"] == "denied"
                assert call("bot.qr.check", id=start("feishu", "lark"))["state"] == "wrong_cloud"
                fid, lid = start("feishu", "feishu"), start("lark", "lark")
                assert call("bot.qr.check", id=fid)["account"] == "cli_sample"
                assert call("bot.qr.check", id=lid)["state"] == "wait"
                assert call("bot.qr.check", id=lid)["account"] == "cli_sample"
                assert call("bot.qr.start", provider="lark", agent="helper", ok=False)["code"] == "conflict"
                assert call("bot.bindings", actor="bob")["items"] == []
                assert all(a["id"]!="helper" for a in call("bot.bindings",actor="bob",available_for="feishu")["available_agents"])
                assert call("bot.lark.policy", actor="bob", id=lid, dm=False, ok=False)["code"] in ["not_found","forbidden"]
                assert call("bot.lark.event", id=fid, device_key="x"*32, ok=False)["code"] == "forbidden"
                for cloud in [cn, intl]:
                    cloud.push("same_id","private",room="oc_dm",chat_type="p2p")
                cn.events.append(cn.events[0])
                cn.push("ignore","group without mention")
                cn.push("wrong","foreign",room="oc_dm",chat_type="p2p",tenant="foreign_tenant")
                cn.push("wrong_app","foreign",room="oc_dm",chat_type="p2p",app="cli_other")
                env = {k:v for k,v in os.environ.items() if k not in ("SHIPFORGE_SERVER","SHIPFORGE_TOKEN","SHIPFORGE_DATA_DIR")}
                out = subprocess.check_output([worker, "bot-sync", "--directory", root/"alice", "--space","default","--binding",fid],env=env,encoding="utf-8", timeout=25)
                assert json.loads(out)["received"] == 1 and cn.acks == [200]*5, (out,cn.acks)
                finish("domestic result")
                assert call("bot.sync",id=fid)["outgoing"] == 1
                assert cn.replies[0][:4] == ("same_id","text",{"text":"domestic result"},False)
                assert call("bot.sync",id=lid)["received"] == 1
                finish("international **result**")
                call("bot.sync",id=lid)
                assert intl.replies[0][0:2] == ("same_id","post") and "international" in str(intl.replies[0][2])
                private = call("list",kind="chat")
                assert len(private)==2 and {r["fields"]["external_provider"] for r in private}=={"feishu","lark"}
                for r in private:
                    assert call("get",actor="bob",kind="chat",id=r["id"],ok=False)["code"]=="not_found"
                settings = call("bot.lark.policy",id=fid,plain_chats=["oc_group","oc_topic"],reactions=True)
                assert settings["settings"]["reactions"] and not next(b for b in call("bot.bindings")["items"] if b["id"]==lid)["settings"]["reactions"]
                cn.push("human","@_user_1 human",mention="human")
                cn.push("top","open topic",room="oc_topic",topic="thread_a")
                cn.push("silent","silent followup",room="oc_topic",root="top",topic="thread_a")
                cn.push("otherroot","@_user_1 second topic",mention="robot",room="oc_topic",root="other",topic="thread_b")
                cn.push("plain","plain group")
                cn.push("post","",kind="post",mention="robot",body={"zh_cn":{"title":"Input","content":[[{"tag":"at","user_id":"@_user_1"},{"tag":"text","text":"relevant "},{"tag":"code_block","language":"rs","text":"let a=3;"},{"tag":"img","image_key":"img_demo"}]]}})
                assert call("bot.sync",id=fid)["received"]==4
                media=next(r for r in call("list",kind="message") if r["fields"].get("attachments"))
                attachment=media["fields"]["attachments"][0]
                content=call("attachment.download",kind="message",id=media["id"],attachment_id=attachment["id"])
                assert base64.b64decode(content["content_base64"])==b"\x89PNG\r\n\x1a\nsynthetic-input"
                assert "let a=3;" in media["fields"]["body"] and "@_user_1" not in media["fields"]["body"] and "@relevant" not in media["fields"]["body"]
                assert call("attachment.download",actor="bob",kind="message",id=media["id"],attachment_id=attachment["id"],ok=False)["code"]=="not_found"
                file_bytes=b"private generated receipt"
                seen_input=False
                for i in range(4):
                    run=finish("~~~rust\nlet a=1;\n~~~" if i==0 else "done",
                           [{"name":"delivery.txt","filename":"delivery.txt","content_base64":base64.b64encode(file_bytes).decode()}] if i==2 else [])
                    for f in run["run"]["fields"]["chat_context"]["messages"][-1].get("input_files",[]):
                        seen_input=True
                        assert f["content_base64"]==content["content_base64"]
                assert seen_input
                assert call("bot.sync",id=fid)["outgoing"]==5
                assert any(k=="interactive" for _,k,_,_,_ in cn.replies)
                assert any(k=="file" and val["file_key"]=="file_return" for _,k,val,_,_ in cn.replies) and file_bytes in cn.uploads[0]
                assert all(thread==(source in ["top","otherroot"]) for source,_,_,thread,_ in cn.replies)
                assert len({uuid for *_,uuid in cn.replies})==len(cn.replies)
                assert len(call("list",kind="chat"))==5
                call("bot.lark.policy",id=fid,thread_followup=True)
                cn.push("continued","continue here",room="oc_topic",root="top",topic="thread_a")
                assert call("bot.sync",id=fid)["received"]==1 and len(call("list",kind="chat"))==5
                cn.react("continued","CrossMark","ev_stop")
                cn.react("continued","CrossMark","ev_stop")
                assert call("bot.sync",id=fid)["received"]==1
                assert cn.replies[-1][0]=="continued" and "取消" in cn.replies[-1][2]["text"]
                cn.react("human","THUMBSUP","ev_react")
                assert call("bot.sync",id=fid)["received"]==1
                finish("reacted")
                call("bot.sync",id=fid)
                assert cn.replies[-1][:4]==("human","text",{"text":"reacted"},True)
                cn.push("new","/new second prompt",room="oc_dm",chat_type="p2p")
                cn.push("fresh","/new",room="oc_dm",chat_type="p2p")
                assert call("bot.sync",id=fid)["received"]==2
                assert len(call("list",kind="chat"))==7 and call("list",kind="task")==[]
                assert any(r["fields"].get("body")=="second prompt" for r in call("list",kind="message"))
                assert cn.reactions and any(k=="removed" for k,_ in cn.reactions)
                assert cn.connects>=7 and intl.connects>=2 and all(code==200 for code in cn.acks+intl.acks)
                call("bot.disconnect",id=fid)
                assert call("bot.sync",id=fid,ok=False)["code"]=="conflict"
                assert next(b for b in call("bot.bindings")["items"] if b["id"]==lid)["state"]=="connected"
                intl.revoked=True
                call("bot.sync",id=lid,ok=False)
                assert next(b for b in call("bot.bindings")["items"] if b["id"]==lid)["state"]=="expired"
                call("bot.disconnect",id=lid)
                for db in (root/"team").glob("*.sqlite*"):
                    assert b"secret-feishu" not in db.read_bytes() and b"secret-lark" not in db.read_bytes()
            finally:
                team.stop(process)
                cn.shutdown()
                intl.shutdown()
    print("Feishu and Lark passed isolated dual-cloud QR, gateway, private conversations, media, routing, reactions and unlink checks.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("bin_dir",type=Path)
    verify(parser.parse_args().bin_dir)
