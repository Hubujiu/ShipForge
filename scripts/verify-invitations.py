"""Verify organization invitations through the HTTP server using independent CLI profiles."""
import argparse
from importlib import import_module
import json
from pathlib import Path
import subprocess
import tempfile

release = import_module("verify-release")


def verify(directory, start, stop, connect):
    cli = release.binary(directory, "shipforge")
    server = release.binary(directory, "shipforge-server")
    with tempfile.TemporaryDirectory(prefix="shipforge-invitations-") as temporary:
        root = Path(temporary)
        data = root / "team"
        def token(actor, scope):
            result = subprocess.run([str(server), "--data-dir", str(data), "token", "issue",
                                     "--actor", actor, "--scope", scope],
                                    text=True, encoding="utf-8", capture_output=True, check=True, timeout=15)
            return json.loads(result.stdout)["token"]

        account = "qa.member@example.test"
        identities = {
            "admin": ("owner", "write"),
            "receiver": (account, "write"),
            "read-only": (account, "read"),
            "wrong": ("qa.other@example.test", "write"),
            "later": ("qa.later@example.test", "write"),
        }
        keys = {name: token(*credentials) for name, credentials in identities.items()}
        process, address = start(server, data, "127.0.0.1:0")
        def call(profile, op, *, ok=True, **fields):
            return release.run_cli(cli, root / profile, "request", request={"op": op, **fields}, ok=ok)

        try:
            for name, credential in keys.items():
                connect(cli, root / name, address, credential)
                assert call(name, "access.whoami")["actor"] == identities[name][0]
            org = "qa-invite-org"
            first, extra = "qa-first-space", "qa-extra-space"
            call("admin", "create", kind="organization", id=org, title="Temporary invitation organization")
            for space in (first, extra):
                call("admin", "create", space=org, kind="space", id=space, title=space,
                     fields={"organization": org, "visibility": "private"})
            assert call("receiver", "whoami", space=org, ok=False)["code"] == "forbidden"
            invitation = call("admin", "invitation.issue", space=org, channel="email",
                              email="QA.Member@example.test", role="member", spaces=[first])
            assert invitation["invitation"]["fields"]["email"] == account
            invite = {"link": invitation["link"]}
            assert call("read-only", "invitation.accept", **invite, ok=False)["code"] == "forbidden"
            assert call("wrong", "invitation.accept", **invite, email=account, actor=account,
                        ok=False)["code"] == "forbidden"
            assert call("receiver", "whoami", space=org, ok=False)["code"] == "forbidden"
            accepted = call("receiver", "invitation.accept", **invite)
            assert accepted == {"organization": org, "actor": account, "joined": True}
            assert call("receiver", "invitation.accept", **invite)["joined"] is False
            assert call("receiver", "whoami", space=org)["role"] == "member"
            assert call("receiver", "whoami", space=first)["role"] == "member"
            assert call("receiver", "whoami", space=extra, ok=False)["code"] == "forbidden"

            additional = call("admin", "invitation.issue", space=org, channel="link", role="guest", spaces=[extra])
            assert call("receiver", "invitation.accept", link=additional["link"])["joined"] is False
            assert call("receiver", "whoami", space=extra)["role"] == "guest"
            assert call("receiver", "whoami", space=org)["role"] == "member"
            assert call("receiver", "whoami", space=first)["role"] == "member"
            invite_id = additional["invitation"]["id"]
            assert call("admin", "invitation.copy", space=org, id=invite_id)["link"] == additional["link"]
            call("admin", "invitation.revoke", space=org, id=invite_id)
            assert call("later", "invitation.accept", link=additional["link"], ok=False)["code"] == "forbidden"
            assert call("later", "whoami", space=org, ok=False)["code"] == "forbidden"
            rotated = call("admin", "invitation.copy", space=org, id=invite_id)["link"]
            assert rotated != additional["link"]
            assert call("later", "invitation.accept", link=additional["link"], ok=False)["code"] == "forbidden"
            assert call("later", "invitation.accept", link=rotated)["joined"] is True
            assert call("later", "whoami", space=extra)["role"] == "guest"
            assert call("later", "whoami", space=first, ok=False)["code"] == "forbidden"
        finally:
            stop(process)
    print("Verified read-token denial, addressed email identity, first join, additional private spaces, role preservation, revoked and rotated links through the packaged CLI and HTTP service.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    team = import_module("verify-team")
    verify(parser.parse_args().directory.resolve(), team.start, team.stop, team.connect)
