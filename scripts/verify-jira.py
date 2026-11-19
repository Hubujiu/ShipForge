"""Exercise the packaged Jira REST site and native task API against one isolated database."""
import argparse
import base64
from contextlib import closing
from importlib import import_module
from importlib.metadata import version as package_version
import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import ProxyHandler, Request, build_opener
from unittest.mock import patch

release = import_module("verify-release")


def verify(directory, start, stop, *, sdk=False):
    server = release.binary(directory, "shipforge-server")
    cli = release.binary(directory, "shipforge")
    org = "jira-qa-org"
    project = "jira-qa-team"
    editor = "editor@example.test"
    with tempfile.TemporaryDirectory(prefix="shipforge-jira-") as temporary:
        root = Path(temporary)
        data = root / "server"
        def token(actor, scope, *args):
            output = subprocess.check_output([str(server), "--data-dir", str(data), "token", "issue",
                                              "--actor", actor, "--scope", scope, *args],
                                             encoding="utf-8", timeout=15)
            return json.loads(output)
        owner = token("owner", "write")["token"]
        member = token(editor, "write")["token"]
        reader = token(editor, "read")
        viewer = token("viewer", "write")["token"]
        outsider = token("outside", "write")["token"]
        hook = token("owner", "hook", "--space", "default", "--resource", "connector", "--binding", "test")["token"]
        worker = token("owner", "worker")["token"]
        process, address = start(server, data, "127.0.0.1:0", extra=("--jira-organization", org))
        base = "http://" + address
        opener = build_opener(ProxyHandler({}))
        def http(path, secret, method="GET", body=None, expected=200, params=None, basic=None):
            headers = {"Content-Type": "application/json"}
            if secret is not None:
                headers["Authorization"] = ("Bearer " + secret if basic is None else
                    "Basic " + base64.b64encode((basic + ":" + secret).encode()).decode())
            q = "?" + urlencode(params) if params else ""
            request = Request(base + path + q, headers=headers, method=method,
                              data=json.dumps(body).encode() if body is not None else None)
            try:
                response = opener.open(request, timeout=7)
            except HTTPError as exc:
                response = exc
            with response:
                raw = response.read()
                value = json.loads(raw) if raw else None
                assert response.status == expected, (method, path, response.status, value)
                if response.status >= 400 and path.startswith("/rest/"):
                    assert isinstance(value["errorMessages"], list) and isinstance(value["errors"], dict), value
                if response.status == 204:
                    assert raw == b""
                return value

        def native(op, **fields):
            return http("/v1/command", owner, "POST", {"op": op, **fields})["data"]
        def jira(resource, secret=member, *, version=2, **request):
            return http("/rest/api/" + str(version) + "/" + resource, secret, **request)
        def search(jql="", secret=member, **params):
            return jira("search", secret, params={"jql": jql, **params})
        def create(summary, **fields):
            return jira("issue", method="POST", expected=201, body={"fields": {
                "project": {"key": "QA"}, "issuetype": {"name": "缺陷"}, "summary": summary, **fields}})
        try:
            native("create", kind="organization", id=org, title="Synthetic Jira organization", fields={"key": "ORG"})
            native("create", kind="space", space=org, id=project, title="Synthetic Jira project",
                   fields={"key": "QA", "visibility": "private"})
            native("create", kind="space", space=org, id="jira-private", title="Hidden space",
                   fields={"key": "SECRET", "visibility": "private"})
            for person, role in ((editor, "member"), ("viewer", "viewer")):
                for space in (org, project):
                    native("create", space=space, kind="member", id=person, title=person,
                           fields={"actor": person, "role": role})
            native("create", space="default", kind="member", id="outside", title="Outside member",
                   fields={"actor": "outside", "role": "member"})
            native("create", space="jira-private", kind="task", title="Other space secret")
            native("create", kind="task", title="Other organization task")
            jira("myself", None, expected=401)
            jira("myself", outsider, expected=404)
            jira("myself", hook, expected=403)
            jira("serverInfo", worker, expected=403)
            jira("myself", member, basic="viewer", expected=401)
            assert jira("myself", member, basic=editor)["accountId"] == editor
            assert jira("serverInfo")["baseUrl"] == base
            assert {p["key"] for p in jira("project")} == {"ORG", "QA"}
            assert {p["key"] for p in jira("project", owner)} == {"ORG", "QA", "SECRET"}
            assert "SECRET" not in str(jira("issue/createmeta"))
            meta = jira("issue/createmeta", params={"projectKeys": "QA"})
            assert meta["projects"][0]["issuetypes"][0]["fields"]["summary"]["required"]
            assert jira("project/QA")["issueTypes"]
            assert {row["id"] for row in jira("priority")} == {"1", "2", "3", "4", "5"}
            assert all(jira(path) for path in ("status", "resolution", "issuetype", "field"))
            assert [a["accountId"] for a in jira("user/assignable/search", params={"project": "QA", "query": "editor"})] == [editor]
            private = native("create", space=project, kind="task", title="Invisible private task",
                             fields={"visibility": "private"})
            jira("issue/" + private["fields"]["number"], expected=404)
            jira("issue/SECRET-1", expected=404)
            jira("issue/SF-1", owner, expected=404)
            jira("issue/1", expected=400)

            adf = {"type": "doc", "version": 1, "content": [{"type": "paragraph", "content": [
                {"type": "text", "text": "Unicode 描述", "marks": [{"type": "strong"}]}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Second line"}]}]}
            first = create("SDK search α", priority={"name": "High"}, assignee={"accountId": editor},
                           description=adf, duedate="2027-01-31", labels=["qa", "unicode"])
            key = first["key"]
            issue = "issue/" + key
            assert first["self"] == base + "/rest/api/2/" + issue
            native("create", space=org, kind="space", id="duplicate-prefix", title="Independent QA project",
                   fields={"key": "QA", "visibility": "private"})
            for i in range(int(first["id"])):
                native("create", space="duplicate-prefix", kind="task", title="Colliding number " + str(i))
            jira(issue, owner, expected=409)
            jira("project/QA", owner, expected=409)
            assert jira("project/" + project, owner)["id"] == project
            assert jira(issue, version=3)["fields"]["description"] == adf
            assert jira(issue, reader["token"])["fields"]["description"] == "Unicode 描述\nSecond line"
            assert set(jira(issue, params={"fields": "summary,labels"})["fields"]) == {"summary", "labels"}
            assert native("organization.settings", space=project)["priority_levels"] == 4
            second = create("Second SDK task", priority={"id": "5"}, parent={"key": key})
            assert native("organization.settings", space=project)["priority_levels"] == 5
            assert jira("issue/" + second["key"])["fields"]["priority"]["name"] == "Lowest"
            assert jira("issue/" + second["key"])["fields"]["parent"]["key"] == key
            assert [row["key"] for row in search('project=QA AND (assignee=currentUser() OR summary~"Second") AND NOT statusCategory=done ORDER BY priority DESC')["issues"]] == [key, second["key"]]
            assert search('project IN ("QA", "OTHER") AND labels NOT IN ("hidden") AND description ~ 描述 AND updated >= -1M AND created >= -1y AND duedate <= "2027-01-31"')["total"] == 1
            assert search("assignee IS EMPTY AND resolution IS EMPTY AND summary !~ Missing")["total"] == 1
            assert search("created >= -7d AND created >= -2w AND id > 0")["total"] == 2
            assert search("project=QA AND resolution=Unresolved", maxResults=0) == {"startAt": 0, "maxResults": 0, "total": 2, "issues": []}
            assert search("project=QA ORDER BY key ASC", startAt=1, maxResults=1)["issues"][0]["key"] == second["key"]
            assert search("", maxResults=300)["maxResults"] == 100
            assert search("", startAt=999)["issues"] == []
            assert search("key=SECRET-1 OR key=" + private["fields"]["number"])["total"] == 0
            for bad in ("bogus=anything", "project IN (QA,", "created >= nonsense", "summary =", "a" * 8193):
                jira("search", params={"jql": bad}, expected=400)
            jira("search", params={"maxResults": "-1"}, expected=400)
            assert jira("search", reader["token"], method="POST", body={"jql": "project=QA", "fields": ["summary"]})["total"] == 2
            jira(issue, reader["token"], method="PUT", body={"fields": {"summary": "denied"}}, expected=403)
            jira(issue, viewer, method="PUT", body={"fields": {"summary": "also denied"}}, expected=403)
            jira(issue, method="PUT", body={"fields": {"labels": ["rollback-label"], "unsupported": "bad"}}, expected=400)
            assert all(row["title"] != "rollback-label" for row in native("list", space=project, kind="tag"))
            assert jira(issue)["fields"]["summary"] == "SDK search α"

            jira(issue, method="PUT", body={"fields": {"summary": "Changed from Jira", "assignee": None}}, expected=204)
            assert jira(issue)["fields"]["assignee"] is None
            row = native("get", space=project, kind="task", id=key)
            assert row["fields"]["assignees"] == [] and row["title"] == "Changed from Jira"
            assert any(e["id"] == row["id"] and e["action"] == "update" for e in native("events", space=project)["events"])
            choices = jira(issue + "/transitions")["transitions"]
            assert "done" in [t["id"] for t in choices] and "triage" not in [t["id"] for t in choices]
            assert jira(issue + "/transitions", viewer)["transitions"] == []
            jira(issue + "/transitions", method="POST", body={"transition": {"id": "unknown"}}, expected=400)
            jira(issue + "/transitions", method="POST", body={"transition": {"id": "done"}}, expected=204)
            assert native("get", space=project, kind="task", id=key)["fields"]["status"] == "done"
            assert search("statusCategory=done AND status=done AND resolution=Done")["total"] == 1
            comment = jira(issue + "/comment", version=3, method="POST", body={"body": adf}, expected=201)
            assert comment["author"]["accountId"] == editor and comment["body"] == adf
            assert jira(issue + "/comment", params={"maxResults": 0})["total"] == 1
            assert jira(issue + "/comment")["comments"][0]["body"] == "Unicode 描述\nSecond line"
            assert jira(issue + "/comment/" + comment["id"])["id"] == comment["id"]
            assert search("key="+key, fields="comment")["issues"][0]["fields"]["comment"]["total"] == 1
            assert native("list", space=project, kind="comment")[0]["fields"]["task"] == row["id"]

            if sdk:
                # Import the published client only when explicitly requested. CI's HTTP suite is offline.
                from jira import JIRA
                sdk_version = package_version("jira")
                with (patch.dict(os.environ, {"NO_PROXY": "127.0.0.1", "no_proxy": "127.0.0.1"}),
                      closing(JIRA(server=base, token_auth=member, max_retries=0, timeout=7)) as client):
                    assert client.myself()["accountId"] == editor
                    sdk_issue = client.create_issue(project={"key": "QA"}, summary="SDK fixture", issuetype={"id": "task"})
                    sdk_issue.update(summary="Updated with jira-python")
                    assert client.issue(sdk_issue.key).fields.summary == "Updated with jira-python"
                    transition = next(t for t in client.transitions(sdk_issue.key) if t["id"] == "done")
                    client.transition_issue(sdk_issue.key, transition["name"], comment="Transition comment", fields={"priority": {"name": "High"}})
                    assert client.add_comment(sdk_issue.key, "SDK comment").body == "SDK comment"
                    assert [c.body for c in client.comments(sdk_issue.key)] == ["Transition comment", "SDK comment"]
                    assert [i.key for i in client.search_issues('summary ~ "Updated with jira-python"')] == [sdk_issue.key]
                    assert client.issue(sdk_issue.key).fields.status.id == "done"
                    sdk_issue.delete()
            # Local CLI and both HTTP APIs read the same record under the token's current grants.
            team = import_module("verify-team")
            team.connect(cli, root / "reader", address, reader["token"])
            projection = release.run_cli(cli, root / "reader", "request", request={
                "op": "jira.read", "space": org, "api_version": 2, "resource": issue})
            assert projection["body"]["fields"]["summary"] == "Changed from Jira"
            jira("issue/" + second["key"], method="PUT", body={"fields":{"priority":None}}, expected=204)
            assert jira("issue/" + second["key"])["fields"]["priority"] is None
            jira("issue/" + second["key"], method="DELETE", expected=204)
            jira("issue/" + second["key"], expected=404)
            native("access.revoke", id=reader["id"])
            jira("myself", reader["token"], expected=401)
            native("delete", space=project, kind="member", id=editor)
            jira(issue, expected=404)
            assert search("project=QA")["total"] == 0
        finally:
            stop(process)
    print("Verified Jira v2/v3 issue CRUD, ADF, comments, workflows, JQL, pagination, personal token scopes and current organization/space permissions over loopback."
          + (f" Tested published jira-python SDK {sdk_version}." if sdk else ""))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument("--sdk", action="store_true")
    args = parser.parse_args()
    team = import_module("verify-team")
    verify(args.directory.resolve(), team.start, team.stop, sdk=args.sdk)
