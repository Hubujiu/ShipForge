"""Verify conversation membership, revoked offline reads and frozen HTTP links using synthetic identities."""
import argparse
from importlib import import_module
import json
from pathlib import Path
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.request import Request, urlopen

team = import_module("verify-team")
release = import_module("verify-release")


def verify(bin_dir, start, stop):
    server, cli = (release.binary(bin_dir, name) for name in ("shipforge-server", "shipforge"))
    with tempfile.TemporaryDirectory(prefix="shipforge-chat-") as directory:
        root = Path(directory)
        data = root / "team"
        keys = {}
        for actor, scope in [("owner", "write"), ("alice", "write"), ("bob", "write"), ("outside", "read")]:
            issued = subprocess.check_output([server, "--data-dir", data, "token", "issue", "--actor", actor, "--scope", scope], encoding="utf-8")
            keys[actor] = json.loads(issued)["token"]
        process, address = start(server, data, "127.0.0.1:0")

        def command(actor, operation, ok=True, **fields):
            return release.run_cli(cli, root / actor, "request", request={"op": operation, **fields}, ok=ok)

        def get_share(link, actor=None):
            headers = {} if actor is None else {"Authorization": "Bearer " + keys[actor]}
            try:
                response = urlopen(Request(link, headers=headers), timeout=8)
            except HTTPError as error:
                response = error
            with response:
                return response.status, response.headers, response.read().decode()

        try:
            for actor, token in keys.items():
                team.connect(cli, root / actor, address, token)
            for actor in ("alice", "bob"):
                command("owner", "create", kind="member", id=actor, title=actor, fields={"actor":actor,"role":"member"})
            command("alice", "create", kind="chat", id="private", title="Example conversation")
            command("alice", "create", kind="message", id="hello", title="Message", fields={"chat":"private","body":"First version"})
            command("alice", "chat.manage", id="private", action="invite", target="bob")
            assert command("bob", "get", False, kind="message", id="hello")["code"] == "not_found"
            assert command("bob", "chat.invitations")[0]["id"] == "private"
            command("bob", "chat.manage", id="private", action="accept")
            assert command("bob", "get", kind="message", id="hello")["fields"]["body"] == "First version"
            public = command("alice", "chat.share.create", id="private", scope="link", message_ids=["hello"])
            org = command("alice", "chat.share.create", id="private", message_ids=["hello"])
            assert public["link"] == f"http://{address}/share/{public['share']}"
            assert get_share(org["link"])[0] == 401
            assert get_share(org["link"], "outside")[0] == 403
            status, headers, text = get_share(org["link"], "bob")
            assert status == 200 and headers.get_content_type() == "text/plain" and "First version" in text
            assert headers["Cache-Control"] == "no-store"
            command("alice", "update", kind="message", id="hello", fields={"body":"Second version"})
            assert get_share(public["link"])[2] == text
            command("alice", "chat.manage", id="private", action="revoke", target="bob")
            assert command("bob", "get", False, kind="message", id="hello")["code"] == "not_found"
            assert command("alice", "chat.share.revoke", id="private", share=org["share"])["revoked"] == 2
            assert get_share(org["link"], "bob")[0] == get_share(public["link"])[0] == 404
            command("alice", "chat.manage", id="private", action="invite", target="bob")
            command("bob", "chat.manage", id="private", action="accept")
            assert command("bob", "get", kind="message", id="hello")["fields"]["body"] == "Second version"
            stop(process)
            process = None
            assert command("bob", "get", False, kind="message", id="hello")["code"] == "offline"
        finally:
            if process is not None:
                stop(process)
    print("Verified two conversation identities, scoped frozen HTTP links, revocation and offline access.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve(), team.start, team.stop)
