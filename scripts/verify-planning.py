"""Verify task planning and intake from independently authenticated CLI profiles."""
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
    with tempfile.TemporaryDirectory(prefix="shipforge-planning-") as temp:
        root = Path(temp)
        actors = ("owner", "visitor", "member")
        tokens = {}
        for actor in actors:
            issued = subprocess.check_output(
                [server, "--data-dir", root / "server", "token", "issue", "--actor", actor,
                 "--scope", "write"], encoding="utf-8",
            )
            tokens[actor] = json.loads(issued)["token"]
        process, address = team.start(server, root / "server", "127.0.0.1:0")

        def call(actor, op, ok=True, **args):
            return release.run_cli(cli, root / actor, "request", request={"op": op, **args}, ok=ok)

        try:
            for actor in actors:
                team.connect(cli, root / actor, address, tokens[actor])
            for actor, role in [("visitor", "guest"), ("member", "member")]:
                call("owner", "create", kind="member", id=actor, title=actor, fields={"role": role})
            for name, visibility in [("intake", "public"), ("team", "private")]:
                call("owner", "create", kind="space", id=name, title=name, fields={"visibility": visibility})
                call("owner", "triage.configure", space=name, enabled=True)
            call("owner", "create", kind="member", space="team", id="visitor", title="Visitor",
                 fields={"role": "guest"})
            target = call("visitor", "triage.submit", space="team", title="Application startup")
            blocked = call("member", "triage.submit", False, space="team", title="Cannot enter")
            assert blocked["code"] == "forbidden"
            ticket = call("member", "triage.submit", space="intake", title="Public submission")
            assert "fields" not in ticket
            assert call("member", "get", False, space="intake", kind="task", id=ticket["id"])["code"] == "forbidden"
            shared = call("owner", "task.share", space="team", id=target["id"])
            assert call("visitor", "task.open", link=shared["link"])["id"] == target["id"]
            assert call("member", "task.open", False, link=shared["link"])["code"] == "forbidden"
            call("owner", "create", kind="project", id="release", title="Release",
                 fields={"spaces": ["team"], "primary_space": "default"})
            task = call("owner", "triage", space="team", id=target["id"], decision="accept",
                        project="release", start_date="2026-11-01", due_date="2026-11-14")
            assert task["fields"]["status"] == "todo"
            assert call("owner", "project.timeline", id="release")["scheduled"][0]["task"]["id"] == target["id"]
            assert not call("member", "project.timeline", id="release")["scheduled"]
            call("owner", "delete", kind="member", space="team", id="visitor")
            assert call("visitor", "task.open", False, link=shared["link"])["code"] == "forbidden"
            call("owner", "triage", space="intake", id=ticket["id"], decision="reject", reason="Documented")
            assert not call("owner", "triage.queue", space="intake")["items"]
        finally:
            team.stop(process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("bin_dir", type=Path)
    verify(parser.parse_args().bin_dir.resolve())
    print("Task intake, review, permission-checked links and project timelines passed for three CLI profiles.")
