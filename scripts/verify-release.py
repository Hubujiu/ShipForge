"""Exercise a distributed ShipForge client against disposable local and team data."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PROGRAMS = ("shipforge", "shipforge-desktop", "shipforge-server", "shipforge-worker")


def binary(directory, name):
    return directory / (name + (".exe" if os.name == "nt" else ""))


def run_cli(cli, directory, *args, request=None, ok=True):
    env = os.environ.copy()
    for key in ("SHIPFORGE_SERVER", "SHIPFORGE_TOKEN", "SHIPFORGE_DATA_DIR"):
        env.pop(key, None)
    result = subprocess.run([str(cli), "--data-dir", str(directory), *args],
                            input=json.dumps(request, ensure_ascii=False) if request is not None else None,
                            capture_output=True, text=True, encoding="utf-8", env=env, timeout=25)
    try:
        reply = json.loads(result.stdout)
    except ValueError as exc:
        raise AssertionError((args, result.returncode, result.stderr)) from exc
    assert (result.returncode == 0) == ok and reply["ok"] == ok, (args, request, reply)
    assert reply["protocol"] == 1
    return reply["data"] if ok else reply["error"]


def verify(directory, version):
    cli = binary(directory, "shipforge")
    for name in PROGRAMS:
        output = subprocess.run([str(binary(directory, name)), "--version"], check=True,
                                capture_output=True, text=True, encoding="utf-8", timeout=12)
        assert version in output.stdout, (name, output.stdout, output.stderr)
    with tempfile.TemporaryDirectory(prefix="shipforge-release-") as tmp:
        data = Path(tmp) / "用户工作区"
        def call(op, **kw):
            return run_cli(cli, data, "request", request={"op": op, **kw})
        task = call("create", kind="task", title="验收 · 看板与 CLI", fields={"description":"验收说明"})
        assert task["kind"] == "task" and task["title"] == "验收 · 看板与 CLI"
        saved = call("update", kind="task", id=task["id"], expected_rev=task["rev"], title="已编辑标题")
        assert saved["rev"] > task["rev"]
        error = run_cli(cli, data, "request", request={"op":"update","kind":"task","id":task["id"],"expected_rev":task["rev"],"title":"过期改动"},ok=False)
        assert error["code"]=="conflict", error
        listed = call("list",kind="task")
        assert [r["title"] for r in listed if r["id"]==task["id"]]==["已编辑标题"]
        assert call("get",kind="task",id=task["id"])["rev"]==saved["rev"]
        source=Path(tmp)/"附件.bin"
        source.write_bytes(bytes(range(256))*4)
        uploaded=run_cli(cli,data,"upload",str(source),"--kind","task","--id",task["id"])
        file=uploaded["fields"]["attachments"][0]
        target=Path(tmp)/"下载.bin"
        downloaded=run_cli(cli,data,"download","task",task["id"],file["id"],str(target))
        assert target.read_bytes()==source.read_bytes() and file["sha256"]==downloaded["sha256"]
        assert run_cli(cli,data,"download","task",task["id"],file["id"],str(target),ok=False)["code"]=="storage"
        assert (data/"workspace.sqlite3").is_file()
        schema = run_cli(cli, data, "schema")
        checked = json.loads((ROOT/"docs"/"protocol.schema.json").read_text(encoding="utf-8"))
        assert checked == schema
        for entry in ("iteration.close","document.restore","run.enqueue","sync.flush","access.whoami"):
            assert entry in schema["properties"]["op"]["enum"]
    print(f"Verified four executables and local persistence, conflicts and CLI schema for {version}.")


if __name__=="__main__":
    cli = argparse.ArgumentParser()
    cli.add_argument("directory",type=Path)
    cli.add_argument("--expected-version",default=tomllib.loads((ROOT/"Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"])
    options = cli.parse_args()
    verify(options.directory.resolve(),options.expected_version)
