"""Run conversation forks through two independent workers and a disposable Git service."""
import argparse
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

release = import_module("verify-release")
git_http = import_module("verify-git-workspaces").git_http


def verify(directory):
    cli = release.binary(directory, "shipforge")
    worker = release.binary(directory, "shipforge-worker")
    with tempfile.TemporaryDirectory(prefix="shipforge-fork-") as temp:
        root = Path(temp)
        data = root / "工作区"
        upstream, repositories = root / "seed", root / "remotes"
        upstream.mkdir()
        repositories.mkdir()
        env = {k: v for k, v in os.environ.items() if not k.startswith(("SHIPFORGE_", "GIT_"))}
        env.update(
            SHIPFORGE_DATA_DIR=str(data), GIT_CONFIG_GLOBAL=str(root / "isolated-gitconfig"),
            GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0",
        )

        def git(*args):
            out = subprocess.run(
                ["git", "-C", str(upstream), *args], env=env, capture_output=True, timeout=20,
            )
            assert out.returncode == 0, out.stderr

        git("config", "--global", "user.name", "Fixture")
        git("config", "--global", "user.email", "fixture@example.test")
        git("init", "--initial-branch=main")
        source = upstream / "branch.txt"
        source.write_text("main", encoding="ascii")
        git("add", "branch.txt")
        git("commit", "-m", "main")
        git("switch", "-c", "feature/base")
        source.write_text("feature", encoding="ascii")
        git("commit", "-am", "feature")
        git("switch", "main")
        git("clone", "--bare", str(upstream), str(repositories / "app.git"))
        runtime = root / "runtime.py"
        runtime.write_text(
            """import json,sys
from pathlib import Path
fields=json.load(sys.stdin)["fields"]
message=next(item for item in reversed(fields["chat_context"]["messages"]) if item["author_kind"]=="human")
marker=Path("workspace-marker.txt")
prior=marker.read_text() if marker.exists() else "empty"
marker.write_text(message["body"])
print(json.dumps({"status":"done","result":Path("branch.txt").read_text()+"|"+prior}))
""",
            encoding="utf-8",
        )

        def host(machine, *args):
            result = subprocess.run(
                [str(worker), *args, "--config", str(root / f"{machine}.json")],
                env=env, capture_output=True, timeout=50,
            )
            assert result.returncode == 0, result.stderr
            return json.loads(result.stdout)

        def call(op, **fields):
            return release.run_cli(cli, data, "request", request={"op": op, **fields})

        for machine in ("source", "target"):
            host(machine, "init", "--runtime", "fixture", "--program", sys.executable, "--arg", str(runtime))
            file = root / f"{machine}.json"
            cfg = json.loads(file.read_text(encoding="utf-8"))
            cfg["machine_id"] = machine
            file.write_text(json.dumps(cfg), encoding="utf-8")
            host(machine, "run", "--once")

        def send(room, body):
            return call("chat.send", id=room, body=body)

        def finish(machine, expected):
            batch = host(machine, "run", "--once")["runs"]
            assert len(batch) == 1, batch
            fields = call("get", kind="run", id=batch[0]["id"])["fields"]
            assert fields["status"] == "done" and fields["result"] == expected, fields
            return next(Path(item["location"]) for item in fields["artifacts"] if item["type"] == "workspace")

        with git_http(repositories, env) as remote:
            settings = call("git.settings", level="space")
            call("git.configure", level="space", expected_rev=settings["rev"],
                 settings={"repositories":[{"url":remote,"branch":"feature/base"}]})
            call("create", kind="agent", id="builder", title="Builder",
                 fields={"runtime":"fixture","machine_id":"source"})
            call("create", kind="chat", id="original", title="Original", fields={"agent":"builder"})
            anchor = send("original", "original")["message"]
            checkout = finish("source", "feature|empty")
            available = call("chat.workspace", id="original")
            assert available["ready"] and available["git"] and available["repository"] == remote
            assert [m["id"] for m in available["targets"]] == ["target"], available

            reused = call("chat.fork", id="original", message=anchor, workspace_mode="reuse")["id"]
            send(reused,"shared")
            assert host("target", "run", "--once")["runs"] == []
            assert finish("source", "feature|original") == checkout
            assert (checkout / "workspace-marker.txt").read_text() == "shared"

            branched = call("chat.fork", id="original", message=anchor, workspace_mode="branch", checkout_branch="main")["id"]
            send(branched, "isolated branch")
            send(branched, "preserve fork directory")
            independent = finish("source", "main|empty")
            assert independent != checkout
            assert finish("source", "main|isolated branch") == independent
            assert (checkout / "workspace-marker.txt").read_text() == "shared"

            moved = call("chat.fork", id="original", message=anchor, workspace_mode="branch",
                         checkout_branch="feature/base", target_machine_id="target")["id"]
            send(moved, "cross machine")
            assert host("source", "run", "--once")["runs"] == []
            target = finish("target", "feature|empty")
            assert target not in {checkout, independent}
            assert (target / "workspace-marker.txt").read_text() == "cross machine"
    print("Verified same-directory reuse, independent base branches, repeated turns and branch-only continuation between two workers.")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory",type=Path)
    verify(parser.parse_args().directory.resolve())
