"""Check organization-wide notification routing with separate CLI processes."""
import argparse
from importlib import import_module
import json
from pathlib import Path
import subprocess
import tempfile
import time

team = import_module("verify-team")
release = import_module("verify-release")


def verify(bin_dir):
    server = release.binary(bin_dir, "shipforge-server")
    cli = release.binary(bin_dir, "shipforge")
    with tempfile.TemporaryDirectory(prefix="shipforge-inbox-") as folder:
        root = Path(folder)
        credentials = {}
        for actor in ("owner", "reader", "author"):
            issued = subprocess.check_output([server, "--data-dir", root / "server", "token", "issue",
                                              "--actor", actor, "--scope", "write"], encoding="utf-8")
            credentials[actor] = json.loads(issued)["token"]
        process, address = team.start(server, root / "server", "127.0.0.1:0")
        def call(actor, op, ok=True, **args):
            return release.run_cli(cli, root / actor, "request", request={"op": op, **args}, ok=ok)
        try:
            for actor, token in credentials.items():
                team.connect(cli, root / actor, address, token)
            for actor in ("reader", "author"):
                call("owner", "create", kind="member", id=actor, title=actor, fields={"role":"member"})
            call("owner", "create", kind="space", id="labs", title="Labs", fields={"visibility":"private","key":"LAB"})
            for actor in ("reader", "author"):
                call("owner", "create", kind="member", id=actor, title=actor, space="labs", fields={"role":"member"})
            cursor = call("reader","inbox.feed")["cursor"]
            call("reader","inbox.configure",channel="desktop",key="assignment",enabled=False)
            for space in ("default","labs"):
                call("author","create",kind="task",space=space,id="work",title=f"Task in {space}",fields={"assignee":"reader"})
            assert call("reader","inbox.feed",after=cursor)["items"] == []
            assert call("reader","inbox.query",section="important")["total"] == 2
            call("author","create",kind="comment",space="labs",id="signoff",title="Please check the latch",fields={"task":"work","mentions":["reader"]})
            items = call("reader","inbox.feed",after=cursor)["items"]
            assert len(items) == 1 and items[0]["link"].startswith("shipforge://inbox/")
            assert call("reader","inbox.query",query="latch",exclude_spaces=["labs"])["total"] == 0
            notice = call("reader","inbox.query",section="important",include_spaces=["labs"])["rows"][0]
            assert call("reader","inbox.open",space="labs",id=notice["id"])["comment"] == "signoff"
            assert call("reader","inbox.query",section="important")["unread"]["important"] == 1
            until = int(time.time() * 1000) + 1800
            call("reader","notification.act",space="labs",id=notice["id"],action="snooze",until=until)
            assert call("reader","inbox.query",section="snoozed")["total"] == 1
            time.sleep(max(0, (until + 5) / 1000 - time.time()))
            assert call("reader","inbox.query",section="snoozed")["total"] == 0
            assert call("reader","inbox.bulk",section="important",action="clear",exclude_spaces=["labs"])["changed"] == 1
            assert call("reader","inbox.query",section="important")["rows"][0]["space"] == "labs"
            call("owner","delete",kind="member",space="labs",id="reader")
            assert call("reader","inbox.query",section="important")["total"] == 0
            assert call("reader","inbox.feed",after=cursor)["items"] == []
            assert call("reader","inbox.open",False,space="labs",id=notice["id"])["code"] == "forbidden"
        finally:
            team.stop(process)
    print("Verified actor-specific channels, multi-space inbox filters, timed reminders, notification links and revocation.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("bin_dir", type=Path)
    verify(parser.parse_args().bin_dir.resolve())
