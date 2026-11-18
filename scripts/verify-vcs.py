"""Replay GitHub and GitLab deliveries into the shipped HTTP server with synthetic credentials."""
from hashlib import sha256
import hmac
from importlib import import_module
import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener

release = import_module("verify-release")


def verify(bin_dir, start, stop):
    server = release.binary(bin_dir, "shipforge-server")
    cli = release.binary(bin_dir, "shipforge")
    with tempfile.TemporaryDirectory(prefix="shipforge-vcs-") as directory:
        root = Path(directory)
        data = root / "team"
        def command(op, **kw):
            return release.run_cli(cli, data, "request", request={"op": op, **kw})

        command("create", kind="agent", id="reviewer", title="Synthetic reviewer", fields={"runtime":"fixture"})
        command("create", kind="automation", id="review", title="Review new PR",
                fields={"handler_id":"reviewer", "instruction":"Read the review context.",
                        "enabled":True, "triggers":[{"type":"webhook"}]})
        hooks = {}
        for provider in ("github", "gitlab"):
            command("create", kind="connector", id=provider, title=provider,
                    fields={"provider":provider,"enabled":True,"repository":"qa/demo","link_tasks":True,"review_rule_id":"review"})
            hooks[provider] = json.loads(subprocess.check_output([str(server),"--data-dir",str(data),"token","issue",
                    "--actor","owner","--scope","hook","--space","default","--resource","connector","--binding",provider], encoding="utf-8"))
        task = command("create", kind="task", title="Linked task")
        number = task["fields"]["number"]
        config = root / "github.json"
        config.write_text(json.dumps([dict(space="default",connector="github",repository="qa/demo",
                    signing_secret_env="SF_TEST_GH_SIGN",hook_token_env="SF_TEST_GH_HOOK")]), encoding="utf-8")
        secret = "loopback-only-fixture"
        env = dict(os.environ, SF_TEST_GH_SIGN=secret, SF_TEST_GH_HOOK=hooks["github"]["token"])
        env.pop("SHIPFORGE_SLACK_CONFIG", None)
        proc, address = start(server,data,"127.0.0.1:0",extra=("--github-config",str(config)),env=env)
        opener = build_opener(ProxyHandler({}))
        def post(provider,event,delivery,data,credential=None):
            raw=json.dumps(data,ensure_ascii=False,separators=(",",":")).encode()
            if provider=="github":
                signature=hmac.new((credential or secret).encode(),raw,sha256).hexdigest()
                headers={"X-GitHub-Event":event,"X-GitHub-Delivery":delivery,"X-Hub-Signature-256":"sha256="+signature}
                route="/v1/github/default/github"
            else:
                headers={"X-Gitlab-Token":credential or hooks["gitlab"]["token"],"X-Gitlab-Event-UUID":delivery}
                route="/v1/integrations/default/gitlab"
            request=Request("http://"+address+route,data=raw,headers=dict(headers,**{"Content-Type":"application/json"}))
            try:
                response=opener.open(request,timeout=8)
            except HTTPError as error:
                response=error
            with response:
                return response.status,json.load(response)
        try:
            for provider in ("github","gitlab"):
                github = provider=="github"
                base="https://"+("github.com/" if github else "gitlab.com/")+"qa/demo"
                repo={"repository":{"full_name":"qa/demo","html_url":base}} if github else {
                    "project":{"id":7,"path_with_namespace":"qa/demo","web_url":base}}
                sha="1234"*10
                push=dict(repo,commits=[{"id":sha,"message":"修复 "+number}])
                if not github: push["object_kind"]="push"
                assert post(provider,"push","invalid-sig",push,"wrong")[0]==401
                wrong=dict(push)
                if github: wrong["repository"]={"full_name":"other/demo","html_url":"https://github.com/other/demo"}
                else: wrong["project"]={"id":8,"path_with_namespace":"other/demo","web_url":"https://gitlab.com/other/demo"}
                assert post(provider,"push","foreign-repo",wrong)[0] in (400,403)
                before=len(command("list",kind="comment"))
                for delivery in ("d1","d1","d2"):
                    assert post(provider,"push",delivery,push)[0]==200
                comments=command("list",kind="comment")
                assert len(comments)==before+1
                assert any(c["fields"]["vcs"]["url"]==base+("/commit/" if github else "/-/commit/")+sha
                           and c["fields"]["task"]==task["id"] for c in comments)
                altered=dict(push, commits=[{"id":sha,"message":"changed "+number}])
                assert post(provider,"push","d1",altered)[0]==409
                if github:
                    review=dict(repo,action="opened",number=12,pull_request={"title":"Review "+number,"author_association":"MEMBER","head":{"ref":"feature"},"base":{"repo":{"full_name":"qa/demo"}}})
                    event="pull_request"
                else:
                    review=dict(repo,object_kind="merge_request",object_attributes={"iid":12,"title":"Review "+number,"action":"open","target_project_id":7,"source_project_id":7})
                    event="merge_request"
                runs=len(command("list",kind="run"))
                for delivery in ("open","open-again"):
                    assert post(provider,event,delivery,review)[0]==200
                rows=command("list",kind="run")
                assert len(rows)==runs+1
                assert any(r["fields"].get("webhook_payload",{}).get("url")==base+("/pull/12" if github else "/-/merge_requests/12") and r["fields"]["task_id"]==task["id"] for r in rows)
                if github:
                    review["action"]="closed";review["pull_request"]["merged"]=True
                else:
                    review["object_attributes"]["action"]="merge"
                assert post(provider,event,"merge",review)[0]==200
                assert command("get",kind="task",id=task["id"])["fields"]["status"]==task["fields"]["status"]
                assert len(command("list",kind="run"))==runs+1
                subprocess.run([str(server),"--data-dir",str(data),"token","revoke",hooks[provider]["id"]],check=True,capture_output=True,timeout=10)
                assert post(provider,event,"revoked",review)[0]==401
        finally:
            stop(proc)
    print("Verified signed GitHub and token-bound GitLab push/review callbacks, task links, review runs, deduplication, repository isolation and revocation on loopback.")
