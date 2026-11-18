"""Exercise Slack's native HTTP contract using synthetic payloads on loopback."""
from hashlib import sha256
import hmac
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener

release = import_module("verify-release")


def verify(bin_dir, start, stop):
    server = release.binary(bin_dir, "shipforge-server")
    cli = release.binary(bin_dir, "shipforge")
    with tempfile.TemporaryDirectory(prefix="shipforge-slack-") as directory:
        root = Path(directory)
        data = root / "workspace"
        def command(op, **values):
            return release.run_cli(cli, data, "request", request={"op": op, **values})
        fields = dict(provider="slack", enabled=True, outbound=True, workspace_id="T1",
                      channel_id="C1", app_id="A1", bot_id="B1")
        command("create", kind="connector", id="support", title="Local Slack fixture", fields=fields)
        command("create", kind="space", id="another", title="Another QA space")
        command("create", space="another", kind="connector", id="support", title="Other channel", fields=dict(fields,channel_id="C2"))
        hook = json.loads(subprocess.check_output([str(server), "--data-dir", str(data), "token",
                           "issue", "--actor", "owner", "--scope", "hook", "--resource", "connector",
                           "--space", "default", "--binding", "support"], encoding="utf-8"))
        bindings = [dict(space="default", connector="support", workspace_id="T1", channel_id="C1",
                         app_id="A1", signing_secret_env="SF_TEST_SIGN", hook_token_env="SF_TEST_HOOK")]
        other = json.loads(subprocess.check_output([str(server), "--data-dir", str(data), "token",
                           "issue", "--actor", "owner", "--scope", "hook", "--resource", "connector",
                           "--space", "another", "--binding", "support"], encoding="utf-8"))
        bindings.append(dict(bindings[0], space="another", channel_id="C2", hook_token_env="SF_TEST_OTHER"))
        config = root / "slack.json"
        config.write_text(json.dumps(bindings), encoding="utf-8")
        secret = "synthetic-signing-secret"
        env = dict(os.environ, SF_TEST_SIGN=secret, SF_TEST_HOOK=hook["token"], SF_TEST_OTHER=other["token"])
        env.pop("SHIPFORGE_GITHUB_WEBHOOKS", None)
        process, address = start(server, data, "127.0.0.1:0", extra=("--slack-config", str(config)), env=env)
        opener = build_opener(ProxyHandler({}))
        def post(payload, *, offset=0, key=secret, route="default/support"):
            raw = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode()
            ts = str(int(time.time()) + offset)
            sig = hmac.new(key.encode(), b"v0:" + ts.encode() + b":" + raw, sha256).hexdigest()
            request = Request("http://" + address + "/v1/slack/" + route, data=raw,
                              headers={"X-Slack-Request-Timestamp": ts, "X-Slack-Signature": "v0=" + sig,
                                       "Content-Type": "application/json"})
            try:
                response = opener.open(request, timeout=8)
            except HTTPError as error:
                response = error
            with response:
                return response.status, json.load(response)
        try:
            challenge = {"type": "url_verification", "challenge": "test-challenge"}
            assert post(challenge) == (200, {"challenge": "test-challenge"})
            assert post(challenge, route="apps/A1/events") == (200, {"challenge": "test-challenge"})
            assert post(challenge, key="incorrect")[0] == 401
            assert post(challenge, offset=-360)[0] == 401 and post(challenge, offset=360)[0] == 401
            assert post(challenge, route="second/support")[0] == 404
            event = {"type": "event_callback", "team_id": "T1", "api_app_id": "A1", "event_id": "Ev1",
                     "event": {"user": "U1", "channel": "C1", "type": "message", "ts": "3.000001", "text": "根消息"}}
            assert post(dict(event, team_id="T2"))[0] == 403
            assert post(dict(event, api_app_id="A2"))[0] == 403
            assert post(dict(event, event=dict(event["event"], channel="C2")))[0] == 200
            assert command("list", kind="task") == []
            assert post(dict(event, event=dict(event["event"], channel="C2")), route="apps/A1/events")[0] == 200
            assert len(command("list", kind="task", space="another")) == 1 and not command("list", kind="task")
            assert post(event) == (200, {"ok": True})
            assert post(event)[0] == 200
            [task] = command("list", kind="task")
            reply = dict(event, event_id="Ev2", event=dict(event["event"], ts="3.000002", thread_ts="3.000001", text="线程中的回复"))
            assert post(reply)[0] == 200 and post(reply)[0] == 200
            [comment] = command("list", kind="comment")
            assert comment["fields"]["task"] == task["id"] and comment["fields"]["body"] == "线程中的回复"
            assert command("im.outbox", id="support") == []
            command("create", kind="comment", title="local response", fields={"task": task["id"]})
            assert command("im.outbox", id="support") == []
            [thread] = command("im.threads", task_id=task["id"])
            reply_request=dict(id="support",task_id=task["id"],thread=thread["thread"],body="Thread response",request_id="first-reply")
            sent_reply=command("im.reply", **reply_request)
            assert command("im.reply", **reply_request)["id"] == sent_reply["id"]
            delivery = command("im.claim", id="support")
            assert delivery["thread"] == {"provider":"slack", "workspace":"T1", "channel":"C1", "thread":"3.000001"}
            echo = dict(reply, event_id="Bot1", event=dict(reply["event"], ts="3.000003", subtype="bot_message",
                        bot_id="B1", app_id="A1", metadata={"event_type": "shipforge_delivery",
                            "event_payload": {"delivery_id": delivery["delivery_id"]}}))
            assert post(dict(echo, event=dict(echo["event"], app_id="Aother")))[0] == 200
            assert command("im.outbox", id="support")[0]["state"] == "leased"
            receipt = dict(event, event_id="Meta1", event={"type":"message_metadata_posted","team_id":"T1","channel_id":"C1","app_id":"A1",
                "bot_id":"B1","message_ts":"3.000003","metadata":{"event_type":"shipforge_delivery",
                "event_payload":{"delivery_id":delivery["delivery_id"],"thread_ts":"3.000001"}}})
            assert post(receipt, route="apps/A1/events")[0] == 200
            assert post(echo)[0] == 200 and post(echo)[0] == 200
            [sent] = command("im.outbox", id="support")
            assert sent["state"] == "sent" and sent["remote_id"] == "3.000003"
            assert len(command("list", kind="comment")) == 3
            subprocess.run([str(server), "--data-dir", str(data), "token", "revoke", hook["id"]],
                           check=True, capture_output=True, timeout=10)
            assert post(challenge)[0] == 401
        finally:
            stop(process)
    print("Verified native Slack signatures, route isolation, replay, thread comments, signed bot receipts and revocation on loopback.")
