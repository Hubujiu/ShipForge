"""Verify opt-in scheduled cleanup with real processes and an isolated Git HTTP remote."""
import argparse
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

git_fixture = import_module("verify-git-workspaces")
release = import_module("verify-release")


def verify(directory):
    cli = release.binary(directory, "shipforge")
    worker = release.binary(directory, "shipforge-worker")
    with tempfile.TemporaryDirectory(prefix="shipforge-storage-qa-") as fixture:
        root = Path(fixture)
        seed, remote, data = (root / name for name in ("seed", "git-http", "workspace"))
        seed.mkdir()
        remote.mkdir()
        config = root / "worker.json"
        script = root / "runtime.py"
        script.write_text("import json,sys\njson.load(sys.stdin)\nprint(json.dumps({'status':'done','result':'checked'}))\n", encoding="ascii")
        env = {k: v for k, v in os.environ.items() if not k.startswith(("SHIPFORGE_", "GIT_"))}
        env.update(SHIPFORGE_DATA_DIR=str(data), GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(root / "gitconfig"), GIT_TERMINAL_PROMPT="0")

        def run(*command):
            result = subprocess.run(command, env=env, capture_output=True, text=True, encoding="utf-8", timeout=50)
            assert result.returncode == 0, result.stderr
            return result.stdout.strip()

        def git(path, *args):
            return run("git", "-C", str(path), *args)

        def machine(*args):
            return json.loads(run(str(worker), *args, "--config", str(config)))

        def request(op, **fields):
            return release.run_cli(cli, data, "request", request={"op": op, **fields})

        git(seed, "init", "--initial-branch=main")
        (seed / "keep.txt").write_text("keep this repository", encoding="ascii")
        git(seed, "add", "keep.txt")
        git(seed, "-c", "user.name=QA", "-c", "user.email=qa@example.test", "commit", "-m", "baseline")
        git(seed, "clone", "--bare", str(seed), str(remote / "app.git"))
        original = git(remote / "app.git", "show-ref")
        machine("init", "--workspace", str(seed), "--runtime", "fixture", "--program", sys.executable, "--arg", str(script))
        machine_id = json.loads(config.read_text(encoding="utf-8"))["machine_id"]

        def preview():
            return request("machine.storage.preview", id=machine_id)

        def configure(**changes):
            return request("machine.storage.configure", id=machine_id, expected_rev=preview()["policy_rev"], policy=changes)

        def poll():
            return machine("run", "--once")

        with git_fixture.git_http(remote, env) as url:
            settings = request("git.settings", level="space")
            request("git.configure", level="space", expected_rev=settings["rev"], settings={"repositories": [{"url": url}]})
            request("create", kind="agent", id="qa", title="Storage QA", fields={"runtime": "fixture"})
            work = request("run.enqueue", handler_id="qa", instruction="Build the checkout")
            poll()
            record = request("get", kind="run", id=work["id"])
            assert record["fields"]["status"] == "done", record
            actual = next(Path(a["location"]) for a in record["fields"]["artifacts"] if a["type"] == "workspace")
            assert (actual / "keep.txt").exists()
            assert preview()["policy"]["enabled"] is False
            configure(min_free_gib=1048576, cache_retention_days=0)
            poll()
            inventory = preview()
            assert 0 < inventory["available_bytes"] < 1048576 * (1 << 30)
            assert inventory["health"]["disk_pressure"] is True
            cache = next(e for e in inventory["entries"] if e["category"] == "cache")
            assert cache["bytes"] > 0 and cache["protection"] == "attached_worktree", cache
            checkout = next(e for e in inventory["entries"] if e["category"] == "run")
            assert checkout["protection"] is None, checkout
            cache_dir = Path(git(actual, "rev-parse", "--path-format=absolute", "--git-common-dir"))
            assert cache_dir.resolve().is_relative_to(root.resolve())
            request("machine.storage.pin", id=machine_id, workspace=cache["id"], pinned=True)
            request("machine.storage.request", id=machine_id, action="clean", confirm=True, workspace=checkout["id"], expected_snapshot=checkout["snapshot"])
            poll()
            assert not actual.exists() and cache_dir.is_dir()
            pinned = next(e for e in preview()["entries"] if e["category"] == "cache")
            assert pinned["pinned"] is True and pinned["protection"] == "pinned", pinned
            configure(enabled=True)
            poll()
            assert cache_dir.is_dir()
            request("machine.storage.pin", id=machine_id, workspace=cache["id"], pinned=False)
            configure(interval_minutes=30)
            poll()
            poll()
            assert not cache_dir.exists(), preview()
            configure(enabled=False, min_free_gib=0)
            next_run = request("run.enqueue", handler_id="qa", instruction="Keep unpublished content")
            poll()
            record = request("get", kind="run", id=next_run["id"])
            checkout_path = next(Path(a["location"]) for a in record["fields"]["artifacts"] if a["type"] == "workspace")
            git(checkout_path, "-c", "user.name=QA", "-c", "user.email=qa@example.test", "commit", "--allow-empty", "-m", "unpublished work")
            unpublished = git(checkout_path, "rev-parse", "HEAD")
            request("machine.storage.request", id=machine_id, action="scan")
            poll()
            item = next(e for e in preview()["entries"] if e["category"] == "run")
            request("machine.storage.request", id=machine_id, action="clean", confirm=True, workspace=item["id"], expected_snapshot=item["snapshot"])
            poll()
            configure(enabled=True)
            poll()
            poll()
            protected = next(e for e in preview()["entries"] if e["category"] == "cache")
            assert protected["protection"] == "local_changes", protected
            assert git(cache_dir, "cat-file", "-t", unpublished) == "commit"
            assert git(remote / "app.git", "show-ref") == original
            assert (seed / "keep.txt").read_text(encoding="ascii") == "keep this repository"
    print("Verified disk pressure, checkout confirmation, cache pins, scheduled mirror reclamation and retention of unpublished Git commits.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
