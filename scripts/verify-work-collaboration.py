"""Exercise saved views, field-loss confirmation and task discussions through a local team server."""
import argparse
from importlib import import_module
import json
from pathlib import Path
import subprocess
import tempfile

team = import_module("verify-team")
release = import_module("verify-release")


def verify(bin_dir):
    server = release.binary(bin_dir, "shipforge-server")
    cli = release.binary(bin_dir, "shipforge")
    with tempfile.TemporaryDirectory(prefix="shipforge-work-collaboration-") as directory:
        root = Path(directory)
        actor_tokens = {}
        for actor in ("owner", "alice", "bob"):
            output = subprocess.check_output(
                [server, "--data-dir", root / "server", "token", "issue",
                 "--actor", actor, "--scope", "write"], encoding="utf-8"
            )
            actor_tokens[actor] = json.loads(output)["token"]
        process, address = team.start(server, root / "server", "127.0.0.1:0")
        def command(actor, operation, ok=True, **fields):
            return release.run_cli(cli, root / actor, "request",
                                   request={"op": operation, **fields}, ok=ok)
        try:
            for actor, token in actor_tokens.items():
                team.connect(cli, root / actor, address, token)
            for actor in ("alice", "bob"):
                command("owner", "create", kind="member", id=actor, title=actor, fields={"role":"member"})
            for field in ("preserve", "remove"):
                command("owner", "create", kind="field", id=field, title=field, fields={"value_type":"text"})
            command("owner", "create", kind="task_type", id="strict", title="Strict type",
                    fields={"field_bindings":[{"field":"preserve","required":True}]})
            command("alice", "create", kind="task", id="one", title="Shared",
                    fields={"status":"todo","custom":{"preserve":"keep","remove":"old"}})
            command("alice", "create", kind="task", id="secret", title="Confidential",
                    fields={"visibility":"private","status":"todo"})
            preview = command("alice", "task.type.preview", id="one", target="strict")
            assert preview["remove"] == ["remove"] and not preview["missing"]
            assert command("alice", "update", False, kind="task", id="one", fields={"type":"strict"})["code"] == "conflict"
            result = command("alice", "update", kind="task", id="one", fields={"type":"strict"},
                             expected_rev=preview["expected_rev"], expected_type_rev=preview["expected_type_rev"],
                             confirm_remove=preview["remove"])
            assert result["fields"]["custom"] == {"preserve":"keep"}
            command("alice", "create", kind="view", title="Todo", id="team-view",
                    fields={"visibility":"space","layout":"table","filter":{"status":"todo"}})
            link = command("alice", "view.share", id="team-view")["link"]
            assert {r["id"] for r in command("alice", "view.open", link=link)["tasks"]} == {"one","secret"}
            assert [r["id"] for r in command("bob", "view.open", link=link)["tasks"]] == ["one"]
            copy = command("bob", "view.save_as", id="team-view", title="Bob's view", changes={"layout":"list"})
            command("bob", "view.organize", id=copy["id"], pinned=True)
            assert command("bob", "view.catalog")[0]["id"] == copy["id"]
            assert len(command("alice", "view.catalog")) == 1
            command("alice", "create", kind="comment", id="thread", title="Discuss", fields={"task":"one"})
            command("bob", "create", kind="comment", id="reply", title="Approved", fields={"task":"one","parent":"thread"})
            found = command("alice", "comment.thread", task_id="one", id="reply")
            resolved = command("alice", "comment.resolve", task_id="one", id="reply", expected_rev=found["root"]["rev"])
            assert resolved["id"] == "thread" and resolved["fields"]["resolution_comment"] == "reply"
            assert command("bob", "comment.reopen", task_id="one", id="thread")["fields"]["resolved"] is False
            command("owner", "delete", kind="member", id="bob")
            for op, kwargs in [("view.open", {"link":link}), ("comment.thread", {"task_id":"one","id":"reply"})]:
                assert command("bob", op, False, **kwargs)["code"] in ("forbidden","not_found")
        finally:
            team.stop(process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("bin_dir", type=Path)
    verify(parser.parse_args().bin_dir.resolve())
    print("Task fields, saved views and discussion permissions passed with three CLI profiles.")
