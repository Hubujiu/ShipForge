"""Exercise organization statuses and type workflows through independent team CLI profiles."""
import argparse
from importlib import import_module
import json
from pathlib import Path
import subprocess
import tempfile

release = import_module("verify-release")
team = import_module("verify-team")


def verify(directory):
    cli = release.binary(directory, "shipforge")
    server = release.binary(directory, "shipforge-server")
    with tempfile.TemporaryDirectory(prefix="shipforge-status-workflows-") as temporary:
        root = Path(temporary)
        data = root / "server"
        actors = {"root": ("owner", "write"), "lead": ("team-lead", "write"),
                  "read": ("team-lead", "read"), "outside": ("unrelated", "write")}
        tokens = {}
        for profile, (identity, scope) in actors.items():
            raw = subprocess.check_output([str(server), "--data-dir", str(data), "token", "issue",
                                           "--actor", identity, "--scope", scope], encoding="utf-8", timeout=15)
            tokens[profile] = json.loads(raw)["token"]
        process, address = team.start(server, data, "127.0.0.1:0")
        def call(profile, operation, *, ok=True, **fields):
            return release.run_cli(cli, root / profile, "request",
                                   request={"op": operation, **fields}, ok=ok)
        try:
            for profile, token in tokens.items():
                team.connect(cli, root / profile, address, token)
            org, project = "workflow-org", "workflow-project"
            call("root", "create", kind="organization", id=org, title=org)
            call("root", "create", space=org, kind="space", id=project, title="Private project",
                 fields={"organization": org, "visibility": "private", "work_type": "software", "task_types": ["bug", "task"]})
            for scope, role in [(org, "member"), (project, "admin")]:
                call("root", "create", space=scope, kind="member", id="team-lead", title="Team lead", fields={"role": role})
            assert call("outside", "status.library", space=project, ok=False)["code"] == "forbidden"
            definition = dict(space=project, id="checking", title="Default caption",
                              fields={"category": "in_progress", "names": {"en": "Peer review"}, "collapsed": True})
            assert call("lead", "status.library.save", ok=False, **definition)["code"] == "forbidden"
            call("lead", "create", space=project, kind="status", id="checking", title="Old local caption", fields={"category": "backlog"})
            call("root", "status.library.save", **definition)
            config = call("read", "task_type.workflow", space=project, id="bug")
            ordered = dict(space=project, id="bug", expected_rev=config["rev"], statuses=["todo", {"library": "checking"}, "done"], default_status={"library": "checking"}, language="en")
            assert call("read", "task_type.workflow.save", ok=False, **ordered)["code"] == "forbidden"
            flow = call("lead", "task_type.workflow.save", **ordered)
            local = flow["default_status"]
            assert local != "checking" and flow["statuses"][1] == {"id": local, "name": "Peer review", "category": "in_progress", "collapsed": True}
            assert call("lead", "task_type.workflow.save", ok=False, **ordered)["code"] == "conflict"
            task = call("lead", "create", space=project, kind="task", id="normal", title="Normal task", fields={"type": "bug"})
            assert task["fields"]["status"] == local
            call("lead", "create", space=project, kind="task", id="legacy", title="Legacy task", fields={"type": "task", "status": "checking"})
            catalog = call("read", "status.library", space=project, language="fr")
            item = next(row for row in catalog["statuses"] if row["id"] == "checking")
            assert not catalog["can_manage"] and item["name"] == "Default caption" and item["usage"] == {"spaces": 1, "tasks": 1, "workflows": 1}
            destination = dict(space=org, id="checking", target="todo")
            preview = call("root", "status.library.removal", **destination)
            assert preview["can_remove"] and preview["usage"]["tasks"] == 1
            call("lead", "update", space=project, kind="task", id="normal", title="Changed before removal", expected_rev=task["rev"])
            assert call("root", "status.library.remove", **destination, preview=preview["preview"], ok=False)["code"] == "conflict"
            latest = call("root", "status.library.removal", **destination)
            remove = {**destination, "preview": latest["preview"], "request_id": "remove-once"}
            assert call("root", "status.library.remove", **remove)["removed"]
            assert call("root", "status.library.remove", **remove)["removed"]
            saved = call("read", "task_type.workflow", space=project, id="bug")
            assert saved["default_status"] == "todo" and [s["id"] for s in saved["statuses"]] == ["todo", "done"]
            assert call("read", "get", space=project, kind="task", id="legacy")["fields"]["status"] == "checking"
            assert call("read", "get", space=project, kind="task", id="normal")["fields"]["status"] == "todo"
            completed = dict(space=project, id="bug", expected_rev=saved["rev"], statuses=["todo", "done"],
                             default_status="todo", request_id="lead-workflow-once")
            first = call("lead", "task_type.workflow.save", **completed)
            assert call("lead", "task_type.workflow.save", **completed) == first
            member = call("root", "get", space=project, kind="member", id="team-lead")
            call("root", "update", space=project, kind="member", id="team-lead", expected_rev=member["rev"],
                 fields={"role": "viewer"})
            assert call("lead", "task_type.workflow.save", ok=False, **completed)["code"] == "forbidden"
        finally:
            team.stop(process)
    print("Verified protected organization statuses, scoped workflow order/default, multilingual fallback, local ID collision, stale preview and replay through CLI and team HTTP.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
