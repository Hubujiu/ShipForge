"""Exercise the skills market over HTTP using two separate CLI profiles and synthetic packages."""
import argparse
import json
import subprocess
import tempfile
import zipfile
from importlib import import_module
from pathlib import Path

release = import_module("verify-release")
team = import_module("verify-team")


def verify(directory):
    server, cli = [release.binary(directory, n) for n in ("shipforge-server", "shipforge")]
    with tempfile.TemporaryDirectory(prefix="shipforge-skills-") as temporary:
        root = Path(temporary)
        data = root / "server"
        keys = {
            actor: json.loads(subprocess.check_output(
                [server, "--data-dir", data, "token", "issue", "--actor", actor, "--scope", "write"],
                encoding="utf-8",
            ))["token"] for actor in ("owner", "reader")
        }
        process, url = team.start(server, data, "127.0.0.1:0")
        try:
            for actor, key in keys.items():
                team.connect(cli, root / actor, url, key)

            def run(actor, op, ok=True, **fields):
                return release.run_cli(cli, root / actor, "request", request={"op": op, **fields}, ok=ok)

            run("owner", "create", kind="member", id="reader", title="Reader", fields={"role": "member"})
            run("owner", "create", kind="space", id="studio", title="Studio", fields={"visibility": "public"})
            run("reader", "space.join", space="studio")
            package = root / "skill"
            package.mkdir()
            note = package / "SKILL.md"
            note.write_text("---\nname: Report\ndescription: Synthetic guide\n---\nVersion one", encoding="utf-8")
            (package / "template.md").write_text("Saved template", encoding="utf-8")
            source = run("owner", "skill.import", path=str(package))["results"][0]["id"]
            note.write_text("# Report\nChanged outside", encoding="utf-8")
            assert run("owner", "get", kind="skill", id=source)["fields"]["instructions"] == "Version one"
            zipped = root / "skill.zip"
            with zipfile.ZipFile(zipped, "w", compression=zipfile.ZIP_DEFLATED) as archive:
                archive.writestr("pack/SKILL.md", "# Reviewer\nRead the notes")
                archive.writestr("pack/examples/input.txt", "Example")
            other = run("owner", "skill.import", path=str(zipped))["results"][0]["id"]
            run("owner", "catalog.publish", id="private-entry", title="Space-only", kind="skill", sources=[source])
            assert not run("reader", "catalog.list", query="Space-only", space="studio")
            run("reader", "catalog.detail", False, space="studio", id="private-entry")
            release_args = dict(id="report-pack", title="QA report stages", kind="skill", category="office",
                                audience="organization", sources=[source, other])
            run("owner", "catalog.publish", **release_args)
            card = run("reader", "catalog.list", space="studio", category="office", query="STAGES")[0]
            assert card["fields"]["count"] == 2 and not card["fields"]["installed"]
            run("reader", "create", kind="agent", space="studio", id="helper", title="Helper")
            records = run("reader", "catalog.install", space="studio", id="report-pack", agent="helper")["records"]
            ids = [s["id"] for s in records]
            assert len(ids) == 2 and set(run("reader", "agent.resolve", space="studio", id="helper")["skills"]) == set(ids)
            assert run("owner", "catalog.detail", space="studio", id="report-pack")["fields"]["installed"]
            first = run("reader", "run.enqueue", space="studio", handler_id="helper", instruction="First snapshot")
            run("reader", "catalog.publish", False, expected_version=1, **release_args)
            run("owner", "update", kind="skill", id=source, fields={"instructions": "Version two"})
            run("owner", "catalog.publish", expected_version=1, **release_args)
            run("reader", "catalog.update", space="studio", id="report-pack")
            new = run("reader", "run.enqueue", space="studio", handler_id="helper", instruction="Second snapshot")
            def snapshot(run_row):
                return next(x for x in run_row["fields"]["config"]["skill_versions"] if x["title"] == "Report")
            assert snapshot(first)["instructions"] == "Version one"
            assert snapshot(new)["instructions"] == "Version two"
            assert any(f["path"] == "template.md" for f in snapshot(new)["files"])
            run("reader", "catalog.uninstall", space="studio", id="report-pack")
            assert not run("reader", "agent.resolve", space="studio", id="helper")["skills"]
            assert snapshot(run("reader", "get", space="studio", kind="run", id=first["id"]))["instructions"] == "Version one"
            instances = [run("reader", "catalog.instantiate", space="studio", id="delivery-agent", instance_title=n)
                         for n in ("First copy", "Second copy")]
            assert instances[0]["id"] != instances[1]["id"]
            run("reader", "update", space="studio", kind="agent", id=instances[0]["id"], fields={"instructions": "custom"})
            assert run("reader", "agent.resolve", space="studio", id=instances[1]["id"])["instructions"] != "custom"
        finally:
            team.stop(process)
    print("Verified HTTP space and organization publication, releases, bundles, independent templates, ZIP and directory copies, installed state, bindings and immutable run snapshots.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
