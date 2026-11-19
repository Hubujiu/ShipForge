"""Exercise real remote Git worktrees and Git author settings through the CLI and worker."""
import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from urllib.parse import urlsplit

release = import_module("verify-release")


@contextmanager
def git_http(repositories, env):
    class GitServer(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def invoke(self):
            url = urlsplit(self.path)
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            headers = {
                "GIT_PROJECT_ROOT": str(repositories),
                "GIT_HTTP_EXPORT_ALL": "1",
                "PATH_INFO": url.path,
                "QUERY_STRING": url.query,
                "REQUEST_METHOD": self.command,
                "CONTENT_TYPE": self.headers.get("Content-Type", ""),
                "CONTENT_LENGTH": str(len(body)),
                "HTTP_GIT_PROTOCOL": self.headers.get("Git-Protocol", ""),
            }
            result = subprocess.run(
                ["git", "http-backend"], env={**env, **headers}, input=body,
                capture_output=True, timeout=20, check=True,
            )
            header, separator, result = result.stdout.partition(b"\r\n\r\n")
            assert separator, header
            values = dict(line.decode("ascii").split(": ", 1) for line in header.splitlines())
            code = values.pop("Status", "200 OK").split()[0]
            self.send_response(int(code))
            for name, value in values.items():
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(result)

        do_GET = invoke
        do_POST = invoke

    with ThreadingHTTPServer(("127.0.0.1", 0), GitServer) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_address[1]}/app.git"
        finally:
            server.shutdown()
            thread.join(timeout=3)


def verify(directory):
    cli = release.binary(directory, "shipforge")
    worker = release.binary(directory, "shipforge-worker")
    with tempfile.TemporaryDirectory(prefix="shipforge-gitqa-") as tmp:
        root = Path(tmp)
        data, upstream, bare = (root / part for part in ("隔离工作区", "seed", "remote"))
        upstream.mkdir()
        bare.mkdir()
        config = root / "worker.json"
        script = root / "commit.py"
        hooks = root / "git-hooks"
        hooks.mkdir()
        env = {k: v for k, v in os.environ.items() if not k.startswith(("SHIPFORGE_", "GIT_"))}
        env.update(
            SHIPFORGE_DATA_DIR=str(data), GIT_CONFIG_NOSYSTEM="1",
            GIT_CONFIG_GLOBAL=str(root / "gitconfig"), GIT_TERMINAL_PROMPT="0",
        )

        def git(path, *args):
            result = subprocess.run(
                ["git", "-C", str(path), *args], env=env,
                text=True, encoding="utf-8", capture_output=True, timeout=20,
            )
            assert result.returncode == 0, result.stderr
            return result.stdout.strip()

        def run_worker(*args):
            result = subprocess.run(
                [str(worker), *args, "--config", str(config)], env=env,
                text=True, encoding="utf-8", capture_output=True, timeout=45,
            )
            assert result.returncode == 0, result.stderr
            return json.loads(result.stdout)

        git(upstream, "config", "--global", "user.name", "Machine Committer")
        git(upstream, "config", "--global", "user.email", "machine@example.test")
        git(upstream, "init", "--initial-branch=main")
        baseline = upstream / "baseline.txt"
        baseline.write_text("main", encoding="ascii")
        git(upstream, "add", "baseline.txt")
        git(upstream, "commit", "-m", "default")
        git(upstream, "switch", "-c", "feature/base")
        baseline.write_text("feature", encoding="ascii")
        git(upstream, "commit", "-am", "feature branch")
        git(upstream, "switch", "main")
        git(upstream, "clone", "--bare", str(upstream), str(bare / "app.git"))
        before = git(bare / "app.git", "show-ref")
        existing_hook = hooks / "pre-commit"
        existing_hook.write_bytes(b"#!/bin/sh\nprintf 'existing hook\\n' > user-hook-output\n")
        existing_hook.chmod(0o700)
        git(upstream, "config", "--global", "core.hooksPath", hooks.as_posix())

        script.write_text(
            """import json, subprocess, sys
from pathlib import Path
packet = json.load(sys.stdin)
assert "git" not in packet
if not Path(".git").exists():
    print(json.dumps({"status":"done","result":"temporary"}))
    sys.exit(0)
subprocess.run(["git","commit","--allow-empty","-m","QA worker commit"],check=True,stdout=sys.stderr)
print(json.dumps({"status":"done","result":Path("baseline.txt").read_text()}))
""",
            encoding="utf-8",
        )
        run_worker("init", "--workspace", str(upstream), "--runtime", "fixture", "--program", sys.executable, "--arg", str(script))
        settings = json.loads(config.read_text(encoding="utf-8"))
        settings["runtimes"]["fixture"]["inherit_env"] = ["GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL"]
        config.write_text(json.dumps(settings), encoding="utf-8")

        def call(op, **fields):
            return release.run_cli(cli, data, "request", request={"op": op, **fields})

        def save(level, settings, **fields):
            before = call("git.settings", level=level, **fields)
            return call("git.configure", level=level, expected_rev=before["rev"], settings=settings, **fields)

        def queued_run(instruction, **extra):
            row = call("run.enqueue", handler_id="build", instruction=instruction, **extra)
            result = run_worker("run", "--once")
            assert len(result["runs"]) == 1 and result["runs"][0]["id"] == row["id"], result
            row = call("get", kind="run", id=row["id"])
            fields = row["fields"]
            assert fields["status"] == "done", fields
            checkout = next(Path(item["location"]) for item in fields["artifacts"] if item["type"] == "workspace")
            return checkout, fields["result"]

        with git_http(bare, env) as url:
            save("organization", {
                "author": {"name": {"mode": "manual", "value": "组织作者"}, "email": {"mode": "manual", "value": "author@example.test"}},
            })
            save("space", {"repositories": [{"url": url, "branch": "feature/base"}]})
            call("create", kind="agent", id="build", title="Builder", fields={"runtime": "fixture"})
            first_title = "Remote feature branch"
            one, feature = queued_run(first_title)
            assert feature == "feature"
            assert git(one, "show", "-s", "--format=%an|%ae|%cn|%ce") == "组织作者|author@example.test|Machine Committer|machine@example.test"
            trailer = "Co-authored-by: ShipForge <shipforge@users.noreply.github.com>"
            assert git(one, "log", "-1", "--format=%B").splitlines().count(trailer) == 1
            assert (one / "user-hook-output").read_text(encoding="ascii").strip() == "existing hook"
            save("agent", {"author": {"coauthor": False}}, id="build")
            two, default = queued_run("Remote branch override", checkout_branch="main")
            assert default == "main" and one != two
            assert git(one, "rev-parse", "--git-common-dir") == git(two, "rev-parse", "--git-common-dir")
            assert trailer not in git(two, "log", "-1", "--format=%B")
            assert (two / "user-hook-output").is_file()
            assert git(bare / "app.git", "show-ref") == before
            git(upstream, "config", "user.name", "Local Author")
            git(upstream, "config", "user.email", "local@example.test")
            call("update", kind="agent", id="build", fields={"directory_mode": "isolated", "profile": "@workspace"})
            local, _ = queued_run("Explicit machine workspace")
            assert git(local, "show", "-s", "--format=%an|%ae") == "Local Author|local@example.test"
            assert git(upstream, "log", "-1", "--format=%s") == "default"
            call("update", kind="agent", id="build", fields={"directory_mode": "temporary"})
            empty, result = queued_run("Empty temporary workspace")
            assert result == "temporary" and not (empty / ".git").exists()
            machine = settings["machine_id"]
            call("machine.storage.request", id=machine, action="scan")
            run_worker("run", "--once")
            listing = call("machine.storage.preview", id=machine)
            item = next(entry for entry in listing["entries"] if entry["title"] == first_title)
            assert item["protection"] is None, item
            call("machine.storage.request", id=machine, action="clean", workspace=item["id"],
                 expected_snapshot=item["snapshot"], confirm=True)
            run_worker("run", "--once")
            assert call("machine.storage.preview", id=machine)["jobs"][0]["outcome"] == "success"
            assert not one.exists() and two.is_dir() and upstream.is_dir()
            assert git(two, "rev-parse", "--is-inside-work-tree") == "true"
    print("Verified actual Git HTTP fetch, shared cache, independent branches, author inheritance, Git committer, existing hooks, opt-out and verified cleanup.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
