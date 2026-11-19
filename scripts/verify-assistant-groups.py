"""Test the personal assistant worker and group avatars using four isolated team CLI profiles."""
import argparse
import base64
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
PNG = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAD0lEQVR4nGMQsyr5H2EIAAdbAk2AAYTqAAAAAElFTkSuQmCC"


def verify(directory):
    cli = release.binary(directory, "shipforge")
    server = release.binary(directory, "shipforge-server")
    worker = release.binary(directory, "shipforge-worker")
    with tempfile.TemporaryDirectory(prefix="shipforge-assistant-") as temporary:
        root = Path(temporary)
        data = root / "service"
        tokens = {}
        actors = {"root": ("owner", "write"), "a": ("fixture-alice", "write"),
                  "read": ("fixture-alice", "read"), "b": ("fixture-bob", "write"),
                  "outside": ("fixture-outsider", "write")}
        for profile, (actor, scope) in actors.items():
            raw = subprocess.check_output([server, "--data-dir", data, "token", "issue",
                                           "--actor", actor, "--scope", scope], encoding="utf-8", timeout=15)
            tokens[profile] = json.loads(raw)["token"]
        service, address = team.start(server, data, "127.0.0.1:0")
        host = None
        def call(profile, op, *, ok=True, **fields):
            return release.run_cli(cli, root / profile, "request", request={"op": op, **fields}, ok=ok)
        def until(query):
            deadline = time.monotonic() + 35
            while time.monotonic() < deadline:
                result = query()
                if result:
                    return result
                if host.poll() is not None:
                    raise AssertionError(host.communicate(timeout=3))
                time.sleep(.15)
            raise AssertionError("local worker timed out")
        try:
            for profile, token in tokens.items():
                team.connect(cli, root / profile, address, token)
            project = "assistant-project"
            call("root", "create", kind="space", id=project, title="Private project",
                 fields={"organization": "default", "visibility": "private"})
            for profile in ("a", "b"):
                actor = actors[profile][0]
                for scope in ("default", project):
                    call("root", "create", space=scope, kind="member", id=actor, title=actor, fields={"role":"member"})
            assert call("read", "assistant.history")["id"] is None
            call("read", "assistant.open", ok=False)
            first = call("a", "assistant.open", space=project)
            assert first["rooms"] == [] and first["id"] == call("a", "assistant.open")["id"]
            assert call("b", "assistant.open")["id"] != first["id"]
            call("outside", "assistant.open", ok=False)
            call("a", "assistant.send", body="Offline", ok=False)
            config = root / "worker.json"
            runtime = root / "answer.py"
            runtime.write_text(
                "import sys,json\nrun=json.load(sys.stdin)['fields']\n"
                "history=run['chat_context']['messages']\n"
                "assert run['config']['model']=='team-fixture' and run['handler_kind']=='assistant'\n"
                "print(json.dumps({'status':'done','result':'History messages: '+str(len(history))}))\n", encoding="utf-8")
            env = {k:v for k,v in os.environ.items() if not k.startswith("SHIPFORGE_")}
            env["SHIPFORGE_DATA_DIR"] = str(root / "a")
            subprocess.run([worker, "init", "--config", config, "--space", project,
                            "--program", sys.executable, "--runtime", "fixture", "--arg", str(runtime)],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, timeout=20)
            host = subprocess.Popen([worker, "run", "--config", config], env=env,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
            until(lambda: call("a","assistant.history")["runtime"] == "fixture")
            assert not call("b", "assistant.history")["runtimes"]
            payload = dict(body="First member-only request", runtime="fixture", model="team-fixture", request_id="first")
            sent = call("a", "assistant.send", **payload)
            assert sent["space"] == project and sent == call("a", "assistant.send", **payload)
            room = sent["id"]
            def finished(text):
                return any(r["fields"]["body"] == text for r in call("a","chat.history",space=project,id=room))
            until(lambda: finished("History messages: 1"))
            reply = call("a", "assistant.send", id=room, body="Continue")
            assert reply["id"] == room and reply["run"] != sent["run"]
            until(lambda: finished("History messages: 3"))
            assert [r["id"] for r in call("read","assistant.history")["rooms"]] == [room]
            assert not call("b","assistant.history")["rooms"]
            call("b","get", space=project, kind="chat", id=room, ok=False)
            call("b","get", space=project, kind="run", id=sent["run"], ok=False)
            call("read","assistant.send", id=room, body="Read only", ok=False)
            call("a","chat.manage",space=project,id=room,action="add",target=actors["b"][0],ok=False)
            group = call("a","chat.start",space=project,mode="group",title="Avatar fixture",members=[actors["b"][0]])
            avatar = dict(space=project, id=group["id"])
            before = call("a","chat.avatar",**avatar)
            assert before["avatar"] is None
            file = root / "avatar.png"
            file.write_bytes(base64.b64decode(PNG))
            call("b","chat.avatar.set",**avatar,expected_rev=before["rev"],content_base64=PNG,ok=False)
            call("read","chat.avatar.set",**avatar,expected_rev=before["rev"],content_base64=PNG,ok=False)
            image = release.run_cli(cli,root/"a","--space",project,"group-avatar",group["id"],str(file),"--rev",str(before["rev"]))
            assert call("b","chat.avatar",**avatar)["avatar"]["sha256"] == image["avatar"]["sha256"]
            assert len(base64.b64decode(image["avatar"]["icon"].split(",")[1])) > 20
            call("a","chat.manage",**avatar,action="transfer",target=actors["b"][0])
            current = call("b","chat.avatar",**avatar)
            call("a","chat.avatar.set",**avatar,expected_rev=current["rev"],remove=True,ok=False)
            call("b","chat.avatar.set",**avatar,expected_rev=current["rev"],remove=True)
            assert call("a","chat.avatar",**avatar)["avatar"] is None
            call("b","chat.manage",**avatar,action="remove",target=actors["a"][0])
            call("a","chat.avatar",**avatar,ok=False)
            call("b","chat.manage",**avatar,action="dissolve")
            call("b","chat.avatar",**avatar,ok=False)
            call("a","create",space=project,kind="agent",id="specialist",title="Specialist",
                 fields={"runtime":"unpaired-fixture"})
            call("a","create",space=project,kind="agent",id="personal",title="Personal agent",
                 fields={"visibility":"private","runtime":"unpaired-fixture"})
            call("b","chat.start",space=project,mode="agent",target="personal",ok=False)
            call("a","create",space=project,kind="task",id="work",title="Shared context",
                 fields={"assignee_kind":"agent","assignee":"specialist","status":"backlog"})
            for body, visibility in [("public note","space"),("private note","private")]:
                call("a","create",space=project,kind="comment",title=body,
                     fields={"task":"work","body":body,"visibility":visibility})
            mine = call("a","task.conversation",space=project,id="work")["id"]
            peer = call("b","task.conversation",space=project,id="work")["id"]
            assert peer != mine
            call("a","chat.history",space=project,id=peer,ok=False)
            req = call("b","chat.send",space=project,id=peer,body="Review the task")
            snapshot = call("b","get",space=project,kind="run",id=req["run"])["fields"]["task_context"]
            assert [c["body"] for c in snapshot["comments"]] == ["public note"]
            call("a","update",space=project,kind="task",id="work",fields={"visibility":"private"})
            call("b","chat.history",space=project,id=peer,ok=False)
            call("b","get",space=project,kind="run",id=req["run"],ok=False)
        finally:
            if host is not None:
                team.stop(host)
            team.stop(service)
    print("Private organization assistant, child-space worker, two turns, group avatars and ownership verified")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
