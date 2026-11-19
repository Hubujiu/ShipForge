"""Exercise the real team, two Windows-compatible CLI profiles and an isolated mail transport."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
import json
import os
from pathlib import Path
import re
import socket
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlencode, urlsplit
from urllib.request import Request, urlopen

release = import_module("verify-release")
team = import_module("verify-team")


def eventually(check):
    until = time.monotonic() + 18
    while time.monotonic() < until:
        value = check()
        if value:
            return value
        time.sleep(.12)
    raise AssertionError("Timed out waiting for mail")


def verify(directory):
    messages, attempts = {}, {}
    blocked = {"reject@example.test", "revoke@example.test"}
    lock = threading.Lock()

    class Mailbox(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            assert self.path == "/emails" and self.headers["Authorization"] == "Bearer isolated-mail"
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            key = self.headers["Idempotency-Key"]
            address = body["to"][0]
            assert len(body["to"]) == 1 and body["from"] == "team@example.test" and key
            with lock:
                attempts[key] = attempts.get(key, 0) + 1
                if address in blocked:
                    status = 422 if address.startswith("reject") else 429
                else:
                    assert messages.setdefault(key, body) == body
                    ambiguous = key.startswith("shipforge-") and address == "guest@example.test" and "invitation" in body["subject"].lower()
                    status = 503 if ambiguous and attempts[key] == 1 else 200
            self.send_response(status)
            if status == 429:
                self.send_header("Retry-After", "90")
            self.end_headers()
            self.wfile.write(json.dumps({"id": key, "status": status}).encode())

    def find_mail(address, path):
        with lock:
            for key, body in messages.items():
                if address == body["to"][0]:
                    url = re.search(r"https?://\S+", body["text"]).group()
                    if urlsplit(url).path.startswith(path):
                        return key, url, body
        return None

    def landing(url):
        with urlopen(url, timeout=5) as response:
            assert response.headers["Cache-Control"] == "no-store"
            assert response.headers["Referrer-Policy"] == "no-referrer"
            return re.search(r'href="(shipforge://[^"]+)"', response.read().decode()).group(1)

    with tempfile.TemporaryDirectory(prefix="shipforge-delivery-") as temporary, ThreadingHTTPServer(("127.0.0.1",0), Mailbox) as gateway:
        threading.Thread(target=gateway.serve_forever, daemon=True).start()
        root = Path(temporary)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1",0))
            port = reservation.getsockname()[1]
        listen = f"127.0.0.1:{port}"
        url = "http://" + listen
        env = dict(os.environ, SHIPFORGE_IDENTITY_BASE_URL=url, SHIPFORGE_IDENTITY_MAIL_URL=f"http://127.0.0.1:{gateway.server_port}/emails",
                   SHIPFORGE_IDENTITY_MAIL_FROM="team@example.test", SHIPFORGE_IDENTITY_MAIL_TOKEN="isolated-mail")
        service_exe = release.binary(directory,"shipforge-server")
        cli = release.binary(directory,"shipforge")
        def start():
            return team.start(service_exe,root/"team",listen,env=env)[0]
        def call(who, op, ok=True, **fields):
            return release.run_cli(cli,root/who,"request",request={"op":op, **fields},ok=ok)
        def login(who):
            address = who + "@example.test"
            challenge = call(who,"profiles.login.start",url=url,email=address)
            _, mail, _ = eventually(lambda: find_mail(address,"/identity/approve"))
            query = {k:v[0] for k,v in parse_qs(urlsplit(mail).query).items()}
            with urlopen(Request(url+"/identity/approve",data=urlencode(dict(query,pair=challenge["pair"])).encode()),timeout=6):
                pass
            return call(who,"profiles.login.finish",id=challenge["id"])["actor"]
        process = start()
        try:
            owner = login("lead")
            guest = login("guest")
            org = call("lead","organization.list")[0]["id"]
            def orgcall(op, **fields):
                return call("lead",op,space=org,**fields)
            settings = orgcall("organization.settings")
            orgcall("organization.configure",expected_rev=settings["rev"],default_language="en")
            room = orgcall("create",kind="space",id="private-team",title="Private team",fields={"visibility":"private"})["id"]
            def invite(who, key):
                return orgcall("invitation.issue",channel="email",email=who+"@example.test",role="member",spaces=[room],request_id=key)
            def status(inv, wanted):
                rows = orgcall("invitation.list")
                item = next(row["fields"]["mail"] for row in rows if row["id"] == inv["invitation"]["id"])
                return item if item["state"] == wanted else None
            inv = invite("guest","guest-invite")
            assert invite("guest","guest-invite") == inv
            accepted = eventually(lambda: status(inv,"accepted"))
            key, link, _ = find_mail("guest@example.test","/open/en/invite/")
            assert accepted["attempts"] == attempts[key] == 2
            deep = landing(link)
            assert deep == inv["link"]
            assert call("lead","invitation.accept",ok=False,link=deep)["code"] == "forbidden"
            assert call("guest","invitation.accept",link=deep)["joined"]
            assert call("guest","delivery.history",space=org,invitations=True,ok=False)["code"] == "forbidden"
            call("guest","inbox.configure",space=org,channel="inbox",key="all",enabled=False)
            task = call("lead","create",space=room,kind="task",title="Sensitive task title",fields={"assignees":[guest]})
            key, link, body = eventually(lambda: find_mail("guest@example.test","/open/en/activity/"))
            assert "Sensitive task title" not in body["text"]
            deep = landing(link)
            opened = call("guest","delivery.open",link=deep)
            assert opened["target"]["id"] == task["id"] and opened["space"] == room
            assert call("lead","delivery.open",space=org,link=deep,ok=False)["code"]=="forbidden"
            assert call("guest","inbox.query",space=org)["total"]==0
            def activity():
                return [r for r in call("guest","delivery.history",space=org) if r["target"]==task["id"]]
            assert eventually(lambda: any(r["state"]=="accepted" for r in activity()))
            call("guest","inbox.configure",space=org,channel="email",key="all",enabled=False)
            before = activity()
            call("lead","update",space=room,kind="task",id=task["id"],fields={"priority":"high"})
            assert activity()==before
            call("lead","delete",space=room,kind="member",id=guest)
            assert call("guest","delivery.open",link=deep,ok=False)["code"]=="forbidden"
            assert activity()==[]
            rejected = invite("reject","reject-retry")
            failed = eventually(lambda: status(rejected,"failed"))
            assert call("guest","delivery.retry",space=org,id=failed["id"],ok=False)["code"]=="forbidden"
            with lock:
                blocked.remove("reject@example.test")
            orgcall("delivery.retry",id=failed["id"])
            assert eventually(lambda: status(rejected,"accepted"))["attempts"]==2
            revoked = invite("revoke","revoke-before-retry")
            eventually(lambda: status(revoked,"retry"))
            orgcall("invitation.revoke",id=revoked["invitation"]["id"])
            team.stop(process)
            process=start()
            assert status(revoked,"canceled")["attempts"]==1
            assert call("lead","access.whoami")["actor"]==owner
            assert any(d["id"]==failed["id"] for d in orgcall("delivery.history",invitations=True))
            try:
                urlopen(url+"/open/en/https://example.test",timeout=5)
                raise AssertionError("invalid external link was accepted")
            except HTTPError as error:
                assert error.code==404
        finally:
            team.stop(process)
            gateway.shutdown()
    print("Verified addressed mail invitations, provider retries and receipts, independent email settings, desktop links, revocation and restart on loopback.")


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory",type=Path)
    verify(parser.parse_args().directory)
