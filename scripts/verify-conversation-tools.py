"""Exercise conversation commands, review, frozen PNGs and current document permissions over HTTP."""
import argparse
import base64
from importlib import import_module
import json
from pathlib import Path
import struct
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.request import Request, urlopen
import zlib

team = import_module("verify-team")
release = import_module("verify-release")


def red_pixel():
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", 1, 1, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes([0, 255, 0, 0]))) + chunk(b"IEND", b""))


def verify(bin_dir):
    server, cli = (release.binary(bin_dir, name) for name in ("shipforge-server", "shipforge"))
    with tempfile.TemporaryDirectory(prefix="shipforge-commands-") as directory:
        root = Path(directory)
        keys = {}
        for actor in ("owner", "alice", "bob", "outside"):
            issued = subprocess.check_output(
                [server, "--data-dir", root / "team", "token", "issue", "--actor", actor,
                 "--scope", "write" if actor != "outside" else "read"], encoding="utf-8")
            keys[actor] = json.loads(issued)["token"]
        process, addr = team.start(server, root / "team", "127.0.0.1:0")

        def call(actor, op, ok=True, **fields):
            return release.run_cli(cli, root / actor, "request", request={"op": op, **fields}, ok=ok)

        def get(url, actor=None):
            headers = {"Authorization": "Bearer " + keys[actor]} if actor else {}
            try:
                res = urlopen(Request(url, headers=headers), timeout=30)
            except HTTPError as err:
                res = err
            with res:
                return res.status, res.headers, res.read()

        try:
            for actor in keys:
                team.connect(cli, root / actor, addr, keys[actor])
            for actor in ("alice", "bob"):
                call("owner", "create", kind="member", id=actor, title=actor, fields={"role": "member"})
            call("alice", "create", kind="agent", id="test-agent", title="Synthetic", fields={"runtime": "test"})
            call("alice", "create", kind="task", id="review", title="Synthetic review", fields={"status": "todo"})
            machine = dict(machine_id="test-box", machine_key="synthetic-machine-key-for-conversation-tools")
            call("alice", "machine.register", **machine, title="Synthetic", runtimes=["test"],
                 slash_commands={"test": ["/lint"]})
            shortcut = call("alice", "shortcut.save", title="Check results", handler_id="test-agent", prompt="/lint")
            assert call("bob", "shortcut.catalog") == []
            request = dict(id=shortcut["id"], task="review", request_id="launch-check")
            sent = call("alice", "shortcut.run", **request)
            assert call("alice", "shortcut.run", **request) == sent
            assert call("alice", "shortcut.history", task="review")["selected"]["chat"] == sent["chat"]
            assert call("alice", "chat.slash", id=sent["chat"])["items"][0]["token"] == "/lint"
            assert call("bob", "chat.slash", False, id=sent["chat"])["code"] in ("forbidden", "not_found")
            call("alice", "attachment.upload", kind="message", id=sent["message"], filename="proof.png",
                 content_base64=base64.b64encode(red_pixel()).decode())
            job = call("alice", "run.claim", **machine)
            call("alice", "run.report", **machine, id=sent["run"], lease=job["lease"], status="done", result="Checks passed")
            later = call("alice", "chat.slash.send", id=sent["chat"], body="/lint again", request_id="one-more")
            assert later["run"] != sent["run"]
            assert call("bob", "task.review.state", id="review")["awaiting_review"] is False
            denied = call("alice", "task.review", False, id="review", run=sent["run"], expected_rev=9999)
            assert denied["code"] == "conflict"
            assert call("alice", "task.review.state", id="review")["awaiting_review"]
            checked = call("alice", "task.review", id="review", run=sent["run"], request_id="send-review")
            assert checked["cleared"] and checked["status"] == "todo"
            assert call("alice", "task.review", id="review", run=sent["run"], request_id="send-review") == checked
            assert not call("alice", "get", kind="task", id="review")["fields"]["awaiting_review"]
            answers = call("alice", "list", kind="message", filter={"chat": sent["chat"]})
            answer = next(m["id"] for m in answers if m["fields"]["body"] == "Checks passed")
            selected = dict(id=sent["chat"], message_ids=[answer], include_questions=True)
            public = call("alice", "chat.share.create", **selected, scope="link")
            org = call("alice", "chat.share.create", **selected)
            assert get(org["link"] + "/image.png")[0] == 401
            assert get(org["link"] + "/image.png", "outside")[0] == 403
            status, headers, png = get(org["link"] + "/image.png", "bob")
            assert status == 200 and headers.get_content_type() == "image/png" and "no-store" in headers["Cache-Control"]
            assert png.startswith(b"\x89PNG\r\n\x1a\n") and png == get(public["link"] + "/image.png")[2]
            snapshot = call("bob", "chat.share.read", share=org["share"])
            assert [m["body"] for m in snapshot["messages"]] == ["/lint", "Checks passed"]
            assert snapshot["messages"][0]["images"][0]["filename"] == "proof.png"
            dest = root / "snapshot.png"
            release.run_cli(cli, root / "bob", "share-image", org["link"], str(dest))
            assert dest.read_bytes() == png
            call("alice", "update", kind="message", id=sent["message"], fields={"body": "Revised after sharing"})
            assert get(public["link"] + "/image.png")[2] == png
            call("alice", "chat.share.revoke", id=sent["chat"], share=public["share"])
            assert get(org["link"] + "/image.png", "bob")[0] == get(public["link"] + "/image.png")[0] == 404

            call("owner", "create", kind="space", id="team-docs", title="Docs", fields={"visibility": "private"})
            for actor in ("alice", "bob"):
                call("owner", "create", space="team-docs", kind="member", id=actor, title=actor, fields={"role":"member"})
            call("alice", "create", space="team-docs", kind="document", id="notes", title="Team notes",
                 fields={"visibility":"space", "body":"# Initial heading"})
            document = call("alice", "document.share", space="team-docs", id="notes")
            url = document["http_url"]
            assert get(url)[0] == 401 and get(url, "outside")[0] == 403 and get(url, "bob")[0] == 200
            call("alice", "create", kind="comment", id="reference", title="Reference",
                 fields={"task":"review", "body":"[Notes](" + document["link"] + ")"})
            def embeds():
                return call("bob", "document.embeds", kind="comment", id="reference")["items"][0]
            assert embeds()["preview"]["outline"][0]["text"] == "Initial heading"
            call("alice", "update", space="team-docs", kind="document", id="notes", fields={"body":"# Latest"})
            assert embeds()["preview"]["outline"][0]["text"] == "Latest" and b"Latest" in get(url, "bob")[2]
            call("owner", "delete", space="team-docs", kind="member", id="bob")
            assert get(url, "bob")[0] == 403 and embeds() == {"available":False, "link":document["link"]}
            assert call("bob", "document.open", False, link=document["link"])["code"] == "forbidden"
        finally:
            team.stop(process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("bin_dir", type=Path)
    verify(parser.parse_args().bin_dir.resolve())
    print("Passed authenticated command, review, PNG snapshot and live document access scenarios.")
