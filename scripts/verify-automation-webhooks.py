"""Exercise private automation URLs with a real server, CLI identities and an independent signer."""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import hashlib
import hmac
from http.client import HTTPConnection
from importlib import import_module
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

team = import_module("verify-team")
release = import_module("verify-release")


def post(url, body=b"{}", headers=None):
    request = Request(url, data=body, headers=headers or {}, method="POST")
    try:
        response = urlopen(request, timeout=6)
    except HTTPError as error:
        response = error
    with response:
        return response.status, json.load(response), response.headers


def signed(secret, body, timestamp):
    stamp = str(timestamp)
    data = stamp.encode() + b"\n" + body
    digest = hmac.new(secret.encode(), data, hashlib.sha256).digest()
    return {"X-Webhook-Timestamp": stamp, "X-Webhook-Signature": "v1=" + base64.b64encode(digest).decode()}


def verify(bin_dir):
    cli = release.binary(bin_dir, "shipforge")
    server = release.binary(bin_dir, "shipforge-server")
    with tempfile.TemporaryDirectory(prefix="shipforge-hooks-") as directory:
        root = Path(directory)
        env = dict(os.environ)
        env.pop("SHIPFORGE_PUBLIC_URL", None)
        token = json.loads(subprocess.check_output([server,"--data-dir",root/"team","token","issue","--actor","owner","--scope","write"], encoding="utf-8"))["token"]
        def seed(**fields):
            return release.run_cli(cli,root/"team","request",request=fields)
        seed(op="create",kind="agent",id="pre-upgrade",title="Earlier agent",fields={})
        seed(op="create",kind="automation",id="old-rule",title="Earlier rule",fields={"handler_id":"pre-upgrade","instruction":"Review the event","enabled":True,"triggers":[{"type":"webhook"}]})
        with closing(sqlite3.connect(root/"team"/"workspace.sqlite3")) as db:
            db.execute("UPDATE records SET fields=json_remove(fields,'$.triggers[0].id') WHERE kind='automation' AND id='old-rule'")
            db.commit()
        old_token=json.loads(subprocess.check_output([server,"--data-dir",root/"team","token","issue","--actor","owner","--scope","hook","--space","default","--resource","automation","--binding","old-rule"],encoding="utf-8"))["token"]
        service, address = team.start(server, root/"team", "127.0.0.1:0", env=env)
        try:
            team.connect(cli, root/"owner", address, token)

            def call(op, actor="owner", ok=True, **fields):
                return release.run_cli(cli, root/actor, "request", request={"op":op, **fields}, ok=ok)

            old_path="http://"+address+"/v1/hooks/default/old-rule"
            assert post(old_path,b'{"event_id":"before-upgrade","payload":{}}',{"Authorization":"Bearer "+old_token})[0]==200
            old_trigger=call("get",kind="automation",id="old-rule")["fields"]["triggers"][0]["id"]
            assert not call("automation.hook.settings",id="old-rule",trigger_id=old_trigger)["issued"]
            for actor, role in [("editor","member"), ("reviewer","viewer")]:
                call("create",kind="member",id=actor,title=actor,fields={"role":role})
                grant=release.run_cli(cli,root/"owner","request", request={"op":"access.issue","actor":actor,"scope":"write"})
                team.connect(cli,root/actor,address,grant["token"])

            call("create",kind="agent",id="helper",title="Hook agent",fields={"runtime":"test"})
            rule_fields={"handler_kind":"agent","handler_id":"helper","instruction":"Inspect the received event","enabled":True,
                         "triggers":[{"type":"webhook","id":"primary"},{"type":"webhook","id":"secondary","enabled":False}]}
            created=call("automation.hook.save",action="create",id="rule",title="API events",fields=rule_fields)
            links={entry["trigger_id"]:entry["url"] for entry in created["issued"]}
            assert len(links)==2 and urlsplit(links["primary"]).netloc==address
            common={"id":"rule","trigger_id":"primary"}
            url=links["primary"]
            assert post(url)[0]==202
            assert post(links["secondary"])[1]["data"]["reason"]=="trigger_disabled"
            call("update",kind="automation",id="rule",fields={"enabled":False})
            assert post(url)[1]["data"]["status"]=="skipped"
            call("update",kind="automation",id="rule",fields={"enabled":True})
            assert call("automation.hook.issue",ok=False,**common)["code"]=="conflict"
            assert call("automation.hook.configure","reviewer",False,signing=True,**common)["code"]=="forbidden"
            settings=call("automation.hook.configure","editor",signing=True,ip_filter=True,allowlist=["203.0.113.7/24","203.0.113.0/24","2001:db8::2/64"],**common)
            assert settings["allowlist"]==["203.0.113.0/24","2001:db8::/64"] and "secret" not in settings
            key=call("automation.hook.secret","editor",**common)["secret"]
            assert key.startswith("whsec_") and call("automation.hook.secret","reviewer",False,**common)["code"]=="forbidden"
            private=call("automation.hook.save",action="create",id="private",title="Private automation",fields=dict(rule_fields,visibility="private"))
            assert private["issued"]
            for op in ["automation.hook.settings","automation.hook.history","automation.hook.secret"]:
                assert call(op,"editor",False,id="private",trigger_id="primary")["code"] in {"not_found","forbidden"}
            for bad in [[],["127.0.0.1/33"],["0.0.0.0/0"],["::/0"],["::ffff:0:0/96"],["10.0.0.1"]*21]:
                assert call("automation.hook.configure",ok=False,ip_filter=True,allowlist=bad,**common)["code"]=="invalid"

            # Withhold the body to ensure both a short request and Expect are rejected on headers alone.
            target=urlsplit(url)
            for path,length,expect,wanted in [
                (target.path,100000,"Expect: 100-continue\r\n",b" 403 "),
                (target.path,16,"",b" 403 "),
                ("/v1/automation-webhooks/unknown",16,"",b" 404 "),
            ]:
                with socket.create_connection((target.hostname,target.port),timeout=4) as conn:
                    conn.sendall((f"POST {path} HTTP/1.1\r\nHost: {target.netloc}\r\n{expect}"
                                  f"X-Forwarded-For: 203.0.113.55\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").encode())
                    with conn.makefile("rb") as reply:
                        statusline=reply.readline()
                    assert wanted in statusline,statusline
            assert post(url,b'{"sensitive":"not-persisted-ip-denial"}',{"Forwarded":"for=203.0.113.7"})[1]["data"]["reason"]=="ip_not_allowed"
            assert call("automation.hook.settings",**common)["last_ip"]=="127.0.0.1"
            call("automation.hook.configure","editor",ip_filter=True,allowlist=["::ffff:127.0.0.1/128"],**common)
            ipv6, ipv6_address = team.start(server,root/"team","[::1]:0",env=env)
            try:
                def post_v6():
                    conn=HTTPConnection("::1",urlsplit("http://"+ipv6_address).port,timeout=5)
                    try:
                        conn.request("POST",target.path,b"{}")
                        response=conn.getresponse()
                        return response.status,json.loads(response.read())
                    finally:
                        conn.close()
                assert post_v6()[0]==403
                call("automation.hook.configure",allowlist=["::1","127.0.0.1"],**common)
                assert post_v6()[1]["data"]["reason"]=="missing_signature"
            finally:
                team.stop(ipv6)
            now=int(time.time())
            payload=b'{ "message": "raw", "n": 2 }\n'
            assert post(url,payload)[1]["data"]["reason"]=="missing_signature"
            assert post(url,payload,{"X-Webhook-Signature":"v1=broken"})[1]["data"]["reason"]=="missing_signature"
            assert post(url,payload,signed(key,payload,now-305))[1]["data"]["reason"]=="timestamp_expired"
            assert post(url,payload,signed(key,payload,now+305))[0]==401
            assert post(url,b'{"message":"raw","n":2}',signed(key,payload,now))[1]["data"]["reason"]=="invalid_signature"
            hdr=signed(key,payload,now*1000+123)
            with ThreadPoolExecutor(max_workers=2) as pool:
                attempts=list(pool.map(lambda _:post(url,payload,hdr),range(2)))
            assert sorted(v[0] for v in attempts)==[202,409]
            assert next(v for v in attempts if v[0]==409)[1]["data"]["reason"]=="replayed_signature"
            github={"X-Hub-Signature-256":"sha256="+hmac.new(key.encode(),payload,hashlib.sha256).hexdigest()}
            assert post(url,payload,github)[0]==202 and post(url,payload,github)[0]==202
            with closing(HTTPConnection(target.hostname,target.port,timeout=5)) as conn:
                chunked=b'{"message":"'+b"a"*2048+b'"}'
                conn.request("POST",target.path,iter([chunked[:32],chunked[32:]]),signed(key,chunked,now),encode_chunked=True)
                response=conn.getresponse()
                assert response.status==202 and json.loads(response.read())["data"]["status"]=="accepted"
            assert post(url,payload,dict(github,**{"X-Webhook-Timestamp":str(now),"X-Webhook-Signature":"v1=broken"}))[0]==401
            assert post(url,b"garbage",signed(key,b"garbage",now))[1]["data"]["reason"]=="invalid_json"
            call("automation.hook.configure",signing=False,**common)
            assert post(url,payload)[0]==202
            call("automation.hook.configure",signing=True,**common)
            assert call("automation.hook.secret",**common)["secret"]==key
            rotated=call("automation.hook.rotate","editor",request_id="rotation-1",**common)["url"]
            assert call("automation.hook.rotate","editor",False,request_id="rotation-1",**common)["code"]=="conflict"
            assert rotated!=url and post(url,payload)[0]==404
            assert call("automation.hook.secret",**common)["secret"]==key
            assert post(rotated,payload,signed(key,payload,now+1))[0]==202
            new_key=call("automation.hook.reset_secret","editor",**common)["secret"]
            assert new_key!=key and post(rotated,payload,signed(key,payload,now+2))[1]["data"]["reason"]=="invalid_signature"
            assert post(rotated,payload,signed(new_key,payload,now+2))[0]==202
            assert call("automation.hook.configure",ok=False,expected_version=0,signing=False,**common)["code"]=="conflict"
            all_entries=[]
            cursor={}
            while True:
                page=call("automation.hook.history","reviewer",limit=3,**common,**cursor)
                all_entries.extend(page["items"])
                if page["next_before"] is None:
                    break
                cursor={"before":page["next_before"]}
            assert len({v["id"] for v in all_entries})==len(all_entries)>10
            assert {"valid","invalid","not_required","missing","expired","ip_not_allowed"} <= {v["signature_state"] for v in all_entries}
            assert {v["source_ip"] for v in all_entries}=={"127.0.0.1","::1"}
            generic=[call("get",id="rule",kind="automation"),call("list",kind="automation"),call("events"),call("export"),all_entries]
            dumped=json.dumps(generic)
            assert all(v not in dumped for v in (urlsplit(url).path.split("/")[-1],key,new_key,"not-persisted-ip-denial"))

            # GitHub-style deliveries have no replay nonce. The existing named bearer route still uses event IDs.
            grant=release.run_cli(cli,root/"owner","request",request={"op":"access.issue","scope":"hook","actor":"owner","space":"default","resource":"automation","binding":"rule"})
            legacy="http://"+address+"/v1/hooks/default/rule"
            wrapped=b'{"event_id":"historical","payload":{"build":1}}'
            auth={"Authorization":"Bearer "+grant["token"]}
            old=post(legacy,wrapped,auth)
            assert old[0]==200 and post(legacy,wrapped,auth)[1]["data"]["id"]==old[1]["data"]["id"]
            call("automation.hook.revoke","editor",**common)
            assert post(rotated)[0]==404 and post(legacy,wrapped,auth)[0]==200
            fresh=call("automation.hook.issue",**common)["url"]
            assert post(fresh,payload,signed(new_key,payload,now+3))[0]==202
            with closing(sqlite3.connect(root/"team"/"workspace.sqlite3")) as db:
                persisted="\n".join(db.iterdump())
                assert "not-persisted-ip-denial" not in persisted and urlsplit(url).path.split("/")[-1] not in persisted
            # The per-URL counter starts fresh after rotation; rate-limited attempts do not enter history.
            rate=links["secondary"]
            outcomes=[post(rate) for _ in range(61)]
            assert outcomes[-1][0]==429 and int(outcomes[-1][2]["Retry-After"])>0
            assert len(call("automation.hook.history",id="rule",trigger_id="secondary",limit=100)["items"])==60
            call("update",kind="automation",id="rule",fields={"triggers":[{"type":"webhook","id":"secondary"}]})
            assert post(fresh)[0]==404
            call("delete",kind="automation",id="rule")
            assert post(rate)[0]==404
        finally:
            team.stop(service)
    print("Automation webhooks passed private URL lifecycle, two HMAC modes, CIDRs, pre-body IP checks, concurrency, history, scoped access and legacy callbacks.")


if __name__=="__main__":
    p=argparse.ArgumentParser()
    p.add_argument("bin_dir",type=Path)
    verify(p.parse_args().bin_dir.resolve())
