"""Exercise member-issued credentials using only an isolated local team service and profiles."""
import argparse
from contextlib import closing
import hashlib
from importlib import import_module
import json
from pathlib import Path
import secrets
import sqlite3
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener

release = import_module("verify-release")


def verify(directory, start, stop, connect):
    server = release.binary(directory, "shipforge-server")
    cli = release.binary(directory, "shipforge")
    http = build_opener(ProxyHandler({}))
    with tempfile.TemporaryDirectory(prefix="shipforge-personal-tokens-") as temp:
        root = Path(temp)
        data = root / "server"
        data.mkdir()
        # A real earlier installation has administrator-issued credentials and no personal_tokens table.
        owner = secrets.token_urlsafe(45)
        identities = data / "identities.sqlite3"
        with closing(sqlite3.connect(identities)) as db:
            db.execute("CREATE TABLE tokens(id TEXT PRIMARY KEY,digest BLOB NOT NULL UNIQUE,actor TEXT NOT NULL,scope TEXT NOT NULL,label TEXT NOT NULL,created INTEGER NOT NULL,revoked INTEGER)")
            db.execute("INSERT INTO tokens VALUES('bootstrap',?,'owner','write','old setup',unixepoch(),NULL)", (hashlib.sha256(owner.encode()).digest(),))
            db.commit()
        process, address = start(server, data, "127.0.0.1:0")

        def call(key, op, status=200, **fields):
            req = Request("http://"+address+"/v1/command", data=json.dumps(dict(fields, op=op)).encode(),
                          headers={"Authorization": "Bearer "+key, "Content-Type": "application/json"})
            try:
                response = http.open(req, timeout=8)
            except HTTPError as exc:
                response = exc
            with response:
                value = json.load(response)
                assert status == response.status, (op, status, response.status, value.get("error"))
            return value.get("data") if status == 200 else value["error"]

        try:
            for actor, role in (("lin", "member"), ("yan", "viewer")):
                call(owner, "create", kind="member", id=actor, title=actor, fields={"role": role})
            editor = call(owner, "access.issue", actor="lin", scope="write")
            reader = call(owner, "access.issue", actor="yan", scope="read")
            session = editor["token"]
            assert call(session, "access.personal.list") == []
            issued = {}
            for lifetime, days in (("30d", 30), ("90d", 90), ("1y", 365), ("never", None)):
                token = call(session, "access.personal.issue", label="build "+lifetime, lifetime=lifetime,
                             request_id="issue-"+lifetime)
                assert token["prefix"] == token["token"][:11]
                assert token["expires_at"] == (None if days is None else token["created_at"]+days*86400)
                issued[lifetime] = token
            repeat = call(session, "access.personal.issue", label="build 30d", lifetime="30d", request_id="issue-30d")
            assert repeat == {"id": issued["30d"]["id"], "already_issued": True}
            call(session, "access.personal.issue", 409, label="changed", request_id="issue-30d")
            call(session, "access.personal.issue", 400, label="bad", lifetime="5y")
            call(session, "access.personal.issue", 400, label="bad", actor="owner")
            call(session, "access.personal.issue", 400, label="bad", scope="write")
            personal = issued["30d"]["token"]
            profile = root / "member"
            connect(cli, profile, address, personal)
            identity = release.run_cli(cli, profile, "request", request={"op": "access.whoami"})
            assert identity["actor"] == "lin" and identity["personal"] and identity["prefix"] == personal[:11]
            assert personal not in json.dumps(identity)
            release.run_cli(cli, profile, "create", "task", "Member task")
            call(personal, "access.issue", 403, actor="owner", scope="write")
            call(personal, "access.personal.issue", 403, label="other")
            rows = call(session, "access.personal.list")
            assert len(rows) == 4 and all("token" not in row for row in rows)
            assert all(token["token"] not in json.dumps(rows) for token in issued.values())
            assert next(row for row in rows if row["id"] == issued["30d"]["id"])["last_used_at"] is not None
            assert next(row for row in rows if row["id"] == issued["90d"]["id"])["last_used_at"] is None
            assert call(reader["token"], "access.personal.list") == []
            call(reader["token"], "access.personal.revoke", 404, id=issued["30d"]["id"])
            read_pat = call(reader["token"], "access.personal.issue", label="Read reports")
            call(read_pat["token"], "list", kind="task")
            call(read_pat["token"], "create", 403, kind="task", title="Denied")
            machine = call(session, "access.personal.issue", label="QA machine", source="machine_setup")
            credentials = dict(machine_id="qa-machine", machine_key=secrets.token_hex(24))
            call(machine["token"], "machine.register", title="QA machine", runtimes=["inspect"], **credentials)
            linked = next(row for row in call(session, "access.personal.list") if row["id"] == machine["id"])
            assert linked["machine"]["name"] == "QA machine" and linked["machine"]["online"]
            call(session, "access.personal.revoke", id=machine["id"])
            call(machine["token"], "machine.heartbeat", 401, **credentials)
            call(personal, "access.personal.revoke", id=issued["30d"]["id"])
            call(personal, "access.whoami", 401)
            with closing(sqlite3.connect(identities)) as db:
                db.execute("UPDATE personal_tokens SET expires=unixepoch() WHERE token_id=?", (issued["90d"]["id"],))
                db.commit()
            call(issued["90d"]["token"], "access.whoami", 401)
            assert {row["id"] for row in call(session, "access.personal.list")} == {issued["1y"]["id"], issued["never"]["id"]}
            call(owner, "update", kind="member", id="lin", fields={"role":"viewer"})
            call(issued["1y"]["token"], "list", kind="task")
            call(issued["1y"]["token"], "create", 403, kind="task", title="Permissions changed")
            call(owner, "access.revoke", id=editor["id"])
            call(issued["never"]["token"], "access.whoami", 401)
            assert call(owner, "access.whoami")["actor"] == "owner"
        finally:
            stop(process)
    print("Verified personal token lifetime, one-time disclosure, two identities, CLI login, machine association, scope and immediate revocation.")


if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    team=import_module("verify-team")
    verify(parser.parse_args().directory.resolve(),team.start,team.stop,team.connect)
