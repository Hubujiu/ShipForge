"""Install/reinstall the real per-user setup in an isolated Unicode directory."""
from __future__ import annotations

import argparse
import configparser
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import tomllib


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument("--expected-version")
    args = parser.parse_args()
    assert os.name == "nt", "Installer verification requires Windows"
    repo = Path(__file__).resolve().parents[1]
    version = args.expected_version or tomllib.loads((repo / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    name = f"shipforge-{version}-windows-x64"
    setup = args.directory.resolve() / f"{name}-setup.exe"
    manifest = Path(str(setup) + ".sha256").read_text(encoding="utf-8").split()
    assert manifest == [digest(setup), setup.name]
    payload = args.directory.resolve() / name
    assert (payload / "shipforge.exe").is_file()
    with tempfile.TemporaryDirectory(prefix="shipforge-install-") as temporary:
        base = Path(temporary).resolve()
        assert base.parent == Path(tempfile.gettempdir()).resolve()
        install = base / "自选 程序目录" / "ShipForge"
        data = base / "保留的 项目数据"
        data.mkdir()
        database = data / "workspace.sqlite3"
        clean_env = {k:v for k,v in os.environ.items() if k not in ("SHIPFORGE_DATA_DIR","SHIPFORGE_SERVER","SHIPFORGE_TOKEN")}
        subprocess.run([str(payload/"shipforge.exe"),"--data-dir",str(data),"create","task","existing task"],
                       env=clean_env, capture_output=True, check=True, timeout=20)
        with closing(sqlite3.connect(database)) as connection:
            connection.execute("CREATE TABLE preserved (content TEXT)")
            connection.execute("INSERT INTO preserved VALUES ('project/task/annotation sentinel')")
            connection.commit()
        before = digest(database)
        for attempt in range(2):
            setup_args = [str(setup), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/SP-", "/NORESTART",
                    "/NOICONS", "/NOCLOSEAPPLICATIONS", "/VERIFYINSTALL=1",
                    f"/DIR={install}", f"/LOG={base / f'install-{attempt}.log'}"]
            if attempt == 0:
                setup_args.append(f"/SHIPFORGEDATA={data}")
            subprocess.run(
                setup_args,
                check=True, timeout=120, env=os.environ | {"SHIPFORGE_DATA_DIR": str(data)},
                creationflags=subprocess.CREATE_NO_WINDOW,
            )
            assert digest(database) == before, "Installer opened or changed project data"
            for path in payload.rglob("*"):
                if path.is_file():
                    assert digest(install / path.relative_to(payload)) == digest(path), path
            marker = configparser.ConfigParser()
            marker.read(install / "shipforge-install.ini", encoding="utf-16")
            assert marker["Installation"]["format"] == "1"
            assert marker["Installation"]["data_dir"] == str(data)
            if attempt:
                assert (install / "user-notes.txt").read_text(encoding="utf-8") == "keep"
            (install / "user-notes.txt").write_text("keep", encoding="utf-8")
        subprocess.run([sys.executable, str(repo / "scripts/verify-release.py"), str(install), "--expected-version", version], check=True)
        assert digest(database) == before
        # Starting the installed CLI explicitly should use the same custom profile as the shortcut.
        reply = json.loads(subprocess.check_output([str(install/"shipforge.exe"),"create","task","installed-profile-check"],env=clean_env,encoding="utf-8"))
        assert reply["ok"]
        with closing(sqlite3.connect(database)) as connection:
            assert connection.execute("SELECT content FROM preserved").fetchone()[0] == "project/task/annotation sentinel"
            assert connection.execute("SELECT title FROM records WHERE kind='task' AND id=?", (reply["data"]["id"],)).fetchone()[0] == "installed-profile-check"
            assert connection.execute("SELECT count(*) FROM records WHERE kind='task' AND title='existing task'").fetchone()[0] == 1
    print(f"Verified {setup.name}: custom Unicode directory, saved profile across reinstall, payload hashes and untouched data")


if __name__ == "__main__":
    main()
