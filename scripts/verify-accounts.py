"""Verify saved desktop identities, secure reauthentication, location restoration and legacy profiles."""
import argparse
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile

release = import_module("verify-release")


def verify(directory, start, stop, connect):
    cli = release.binary(directory, "shipforge")
    server = release.binary(directory, "shipforge-server")
    with tempfile.TemporaryDirectory(prefix="shipforge-switch-") as temporary:
        root = Path(temporary)
        team, admin, device = (root / name for name in ("team","admin","device"))
        owner = json.loads(subprocess.check_output([str(server),"--data-dir",str(team),"token","issue","--actor","owner","--scope","write"],encoding="utf-8"))["token"]
        process, address = start(server,team,"127.0.0.1:0")
        env = {k:v for k,v in os.environ.items() if k not in ("SHIPFORGE_TOKEN","SHIPFORGE_SERVER","SHIPFORGE_DATA_DIR")}
        def op(profile, action, ok=True, **kw):
            return release.run_cli(cli,profile,"request",request=dict(kw,op=action),ok=ok)
        def login_as(id, secret, okay):
            result = subprocess.run([str(cli),"--data-dir",str(device),"connect","http://"+address,"--account",id,"--token-stdin"],
                input=secret,encoding="utf-8",capture_output=True,env=env,timeout=10)
            assert (result.returncode==0)==okay, json.loads(result.stdout).get("error")
        try:
            connect(cli,admin,address,owner)
            for actor in ("writer","reader"):
                op(admin,"create",kind="member",id=actor,title=actor,fields={"role":"member"})
            op(admin,"create",kind="organization",id="studio",title="Studio")
            op(admin,"create",kind="member",space="studio",id="writer",title="Writer",fields={"role":"member"})
            op(admin,"create",kind="task",space="studio",id="drawing",title="Layout")
            a = op(admin,"access.issue",actor="writer",scope="write")
            b = op(admin,"access.issue",actor="reader",scope="read")
            connect(cli,device,address,a["token"])
            path = device / "connection.json"
            book = json.loads(path.read_text(encoding="utf-8"))
            # The earlier Windows client serialized one protected {url,token} object.
            path.write_text(json.dumps(book["accounts"][0]["connection"]),encoding="utf-8")
            connect(cli,device,address,b["token"])
            device_list = op(device,"profiles.list")
            assert len(device_list["accounts"])==2
            aid, bid = next(x["id"] for x in device_list["accounts"] if not x["active"]), device_list["active"]
            assert op(device,"profiles.use",id=aid)["id"]==aid
            assert op(device,"access.whoami")["actor"]=="writer"
            where = {"space":"studio","page":"tasks","kind":"task","id":"drawing"}
            op(device,"profiles.remember",id=aid,location=where)
            op(device,"profiles.use",id=bid)
            assert op(device,"access.whoami")["actor"]=="reader"
            assert op(device,"profiles.remember",id=aid,location=where,ok=False)["code"]=="conflict"
            op(device,"profiles.remember",id=bid,location={"space":"default","page":"inbox"})
            restored=op(device,"profiles.use",id=aid)["location"]
            assert restored["space"]=="studio" and restored["selected"]["id"]=="drawing"
            assert {x["actor"] for x in op(device,"profiles.list")["accounts"]}=={"writer","reader"}
            # Revocation of space membership, then restoration from a fresh CLI process.
            op(admin,"delete",kind="member",space="studio",id="writer")
            op(device,"profiles.use",id=bid)
            assert op(device,"profiles.use",id=aid)["location"]=={"space":"default","page":"tasks"}
            op(device,"profiles.use",id=bid)
            op(admin,"access.revoke",id=a["id"])
            assert op(device,"profiles.use",id=aid,ok=False)["code"]=="unauthorized"
            listed=op(device,"profiles.list")
            assert listed["active"]==bid and next(x for x in listed["accounts"] if x["id"]==aid)["needs_login"]
            login_as(aid,b["token"],False)
            fresh=op(admin,"access.issue",actor="writer",scope="write")["token"]
            login_as(aid,fresh,True)
            assert op(device,"access.whoami")["actor"]=="writer"
            assert len(op(device,"profiles.list")["accounts"])==2
            assert not any(t in json.dumps(op(device,"profiles.list")) for t in (fresh,a["token"],b["token"]))
            if os.name=="nt":
                assert all(t not in path.read_text(encoding="utf-8") for t in (fresh,a["token"],b["token"]))
            release.run_cli(cli,device,"disconnect")
            local=op(device,"profiles.list")
            assert local["active"] is None and [x["id"] for x in local["accounts"]]==[bid]
            assert op(device,"profiles.use",id=bid)["location"]["page"]=="inbox"
            op(device,"profiles.forget",id=bid)
            assert op(device,"profiles.list")=={"active":None,"accounts":[]}
            assert op(device,"access.whoami")["mode"]=="local"
        finally:
            stop(process)
    print("Verified two saved identities, organization and task restoration, reauthentication, revoked access, local logout and a pre-upgrade connection.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    team=import_module("verify-team")
    verify(parser.parse_args().directory.resolve(),team.start,team.stop,team.connect)
