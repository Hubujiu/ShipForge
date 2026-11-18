"""Advance a synthetic offline team by a week and observe the normal server clock on restart."""
import argparse
from contextlib import closing
from datetime import datetime, timezone
from importlib import import_module
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time

release = import_module("verify-release")
team = import_module("verify-team")


def verify(bin_dir, start, stop):
    server, cli = (release.binary(bin_dir, name) for name in ("shipforge-server", "shipforge"))
    with tempfile.TemporaryDirectory(prefix="shipforge-iteration-") as directory:
        root = Path(directory)
        data, profile = root / "team", root / "admin"
        workspace = data / "workspace.sqlite3"
        token = json.loads(subprocess.check_output([server, "--data-dir", data, "token", "issue", "--actor","owner","--scope","write"], encoding="utf-8"))["token"]
        process, address = start(server, data, "127.0.0.1:0")
        try:
            team.connect(cli, profile, address, token)
            def call(op, **fields):
                return release.run_cli(cli, profile, "request", request={"op": op, **fields})
            day = datetime.now(timezone.utc).date().isoformat()
            call("iteration.configure", fields={"mode":"automatic","timezone":"UTC","anchor_date":day,"weeks":1,"future_count":1,"auto_join_in_progress":True})
            future, active = call("iteration.timeline")
            work = call("create", kind="task", id="ongoing", title="Work", fields={"status":"in_progress"})
            assert work["fields"]["iteration"] == active["id"]
            call("create", kind="task", id="done", title="Complete", fields={"status":"done","iteration":active["id"]})
            stop(process)
            process = None
            # The service is stopped. Shift only this temporary fixture and keep its real task IDs.
            with closing(sqlite3.connect(workspace)) as db, db:
                db.execute("""UPDATE records SET fields=json_set(fields,'$.start_date',date(json_extract(fields,'$.start_date'),'-7 days'),'$.end_date',date(json_extract(fields,'$.end_date'),'-7 days'))
                    WHERE kind='iteration' AND space='default' AND json_extract(fields,'$.cadence')=1""")
                db.execute("""UPDATE iteration_policies SET next_start=date(next_start,'-7 days'),maintained_on=date(maintained_on,'-7 days'),
                    config=json_set(config,'$.anchor_date',date(json_extract(config,'$.anchor_date'),'-7 days')) WHERE space='default'""")
            process, _ = start(server, data, address)
            # No command, worker or connected desktop triggers this transition.
            with closing(sqlite3.connect(workspace)) as db:
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    status = db.execute("SELECT json_extract(fields,'$.status') FROM records WHERE space='default' AND kind='iteration' AND id=?", (active["id"],)).fetchone()[0]
                    if status == "closed":
                        break
                    time.sleep(0.1)
                assert status == "closed", "the idle team service did not advance its iteration clock"
            assert call("get", kind="task", id="ongoing")["fields"]["iteration"] == future["id"]
            assert call("get", kind="task", id="done")["fields"]["iteration"] == active["id"]
            assert len(call("iteration.history", id=active["id"])["tasks"]) == 2
            assert len(call("iteration.timeline")) == 3
        finally:
            if process is not None:
                stop(process)
    print("Verified idle server catch-up, automatic placement, unfinished rollover and preserved iteration history.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve(), team.start, team.stop)
