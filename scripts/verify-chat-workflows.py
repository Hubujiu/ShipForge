"""Verify real CLI, machine and child-runtime conversation handoff in an isolated workspace."""
import argparse
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

release = import_module("verify-release")


def verify(directory):
    cli = release.binary(directory, "shipforge")
    worker = release.binary(directory, "shipforge-worker")
    with tempfile.TemporaryDirectory(prefix="shipforge-chat-worker-") as temp:
        root = Path(temp)
        work = root / "runtime"
        work.mkdir()
        data = root / "data"
        config = root / "worker.json"
        env = {key: value for key, value in os.environ.items() if not key.startswith("SHIPFORGE_")}
        env["SHIPFORGE_DATA_DIR"] = str(data)

        def host(*args):
            result = subprocess.run([worker, *args], env=env, capture_output=True, timeout=40)
            assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
            return json.loads(result.stdout)

        def command(op, **fields):
            return release.run_cli(cli, data, "request", request={"op": op, **fields})

        host("init", "--config", config, "--workspace", work, "--example")
        script = root / "synthetic_runtime.py"
        script.write_text(
            "import json,sys\n"
            "run=json.load(sys.stdin)['fields']\n"
            "context=run['chat_context']\n"
            "assert context['id']==run['chat_id']\n"
            "result={'status':'done','result':'Worker saw: '+context['messages'][0]['body']}\n"
            "if run['chat_conversion']:\n"
            "    assert any(m['body']=='Worker saw: Investigate export latency' for m in context['messages'])\n"
            "    result['task_draft']={'title':'Investigate export latency','description':'Measure latency for large exports.'}\n"
            "print(json.dumps(result))\n",
            encoding="utf-8")
        settings = json.loads(config.read_text(encoding="utf-8"))
        settings.pop("workspace", None)
        settings["runtimes"]["synthetic"] = {"program": sys.executable, "args": ["-X", "utf8", str(script)]}
        config.write_text(json.dumps(settings), encoding="utf-8")
        command("create", kind="agent", id="worker", title="Workflow fixture", fields={"runtime":"synthetic"})
        command("create", kind="chat", id="room", title="Export analysis", fields={"agent":"worker"})
        sent = command("chat.send", id="room", body="Investigate export latency", request_id="initial")
        host("run", "--config", config, "--once")
        messages = command("chat.history", id="room")
        assert messages[-1]["fields"]["body"] == "Worker saw: Investigate export latency", messages[-1]["fields"]["body"]
        agent_fork = command("chat.fork", id="room", message=messages[-1]["id"])
        inherited = command("chat.history", id=agent_fork["id"])
        assert [m["fields"]["author_kind"] for m in inherited] == ["human", "agent"]
        assert [m["fields"]["body"] for m in inherited] == [m["fields"]["body"] for m in messages]
        fork = command("chat.fork", id="room", message=sent["message"])
        assert [m["fields"]["body"] for m in command("chat.history", id=fork["id"])] == ["Investigate export latency"]
        command("chat.send", id=fork["id"], body="Study another hypothesis.")
        host("run", "--config", config, "--once")
        assert command("chat.history", id=fork["id"])[-1]["fields"]["body"] == "Worker saw: Investigate export latency"
        assert len(command("chat.history", id="room")) == 2
        conversion = command("chat.convert", id="room", request_id="finish")
        assert command("list", kind="task") == []
        host("run", "--config", config, "--once")
        run = command("get", kind="run", id=conversion["run"])
        assert run["fields"]["status"] == "done", run
        task = command("get", kind="task", id=run["fields"]["created_task"])
        assert task["title"] == "Investigate export latency"
        assert command("get", kind="chat", id="room")["fields"]["task"] == task["id"]
        assert command("chat.convert", id="room", request_id="finish")["run"] == run["id"]
        assert len(command("list", kind="task")) == 1
        print("Verified real worker context on both branches and idempotent Agent-created task binding.")


if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--bin-dir",type=Path,required=True)
    verify(parser.parse_args().bin_dir.resolve())
