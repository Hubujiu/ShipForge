"""Verify two independent team profiles, live variants, grants, and archival over real HTTP."""
import argparse
import json
import subprocess
import tempfile
from importlib import import_module
from pathlib import Path

team = import_module("verify-team")
release = import_module("verify-release")


def verify(directory):
    server, cli = [release.binary(directory, name) for name in ("shipforge-server", "shipforge")]
    with tempfile.TemporaryDirectory(prefix="shipforge-agent-directory-") as scratch:
        root = Path(scratch)
        data = root / "server"
        keys = {}
        for actor in ("owner", "alice", "bob"):
            issued = subprocess.check_output(
                [server, "--data-dir", data, "token", "issue", "--actor", actor, "--scope", "write"],
                encoding="utf-8",
            )
            keys[actor] = json.loads(issued)["token"]
        process, address = team.start(server, data, "127.0.0.1:0")

        def command(principal, operation, ok=True, **fields):
            request = {"op": operation, **fields}
            return release.run_cli(cli, root / principal, "request", request=request, ok=ok)

        try:
            for actor, key in keys.items():
                team.connect(cli, root / actor, address, key)
            for actor in ("alice", "bob"):
                command("owner", "create", kind="member", id=actor, title=actor, fields={"role": "member"})
            command("owner", "create", kind="space", id="second", title="Second", fields={"visibility": "public"})
            for actor in ("alice", "bob"):
                command(actor, "space.join", space="second")
            source = {"instructions": "Initial prompt", "model": "mini", "runtime": "local-tool", "visibility": "private"}
            command("alice", "create", kind="agent", id="review", title="Reviewer", fields=source)
            assert command("bob", "agent.directory") == []
            command("alice", "agent.access", id="review", actor="bob", permission="use")
            branch = command("bob", "agent.variant", id="review", target_space="second", title="Reviewer", visibility="space")
            dest = {"id": branch["id"], "space": "second"}
            assert {r["space"] for r in command("bob", "agent.directory", scope="organization", query="Reviewer")} == {"default", "second"}
            command("bob", "agent.organize", action="favorite", **dest)
            assert command("bob", "agent.directory", space="second", view="favorite")[0]["id"] == branch["id"]
            assert command("alice", "agent.directory", space="second", view="favorite") == []
            command("bob", "agent.override", field="model", value="large", **dest)
            old = command("bob", "run.enqueue", space="second", handler_id=branch["id"], instruction="Sample run")
            command("alice", "update", kind="agent", id="review", fields={"instructions": "Updated prompt", "model": "medium"})
            effective = command("bob", "agent.resolve", **dest)
            assert (effective["instructions"], effective["model"]) == ("Updated prompt", "large")
            command("bob", "agent.override", field="model", value=None, **dest)
            assert command("bob", "agent.resolve", **dest)["model"] == "medium"
            assert command("bob", "get", space="second", kind="run", id=old["id"])["fields"]["config"]["instructions"] == "Initial prompt"
            command("bob", "agent.archive", **dest)
            assert command("bob", "get", space="second", kind="run", id=old["id"])["fields"]["status"] == "cancelled"
            assert command("bob", "agent.directory", space="second") == []
            assert command("bob", "agent.directory", space="second", view="archived")[0]["id"] == branch["id"]
            command("bob", "agent.restore", **dest)
            command("alice", "agent.access", id="review", actor="bob", permission="remove")
            assert command("bob", "agent.resolve", False, **dest)["code"] in ("not_found", "forbidden")
            assert command("bob", "get", False, kind="run", space="second", id=old["id"])["code"] in ("not_found", "forbidden")
            assert command("bob", "agent.directory", space="second") == []
            team.stop(process)
            process = None
            assert command("bob", "get", False, kind="agent", **dest)["code"] == "offline"
        finally:
            if process is not None:
                team.stop(process)
    print("Verified independent HTTP identities, isolated directory preferences, cross-space live settings, retained snapshots, archive/restore, revocation and offline restrictions.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
