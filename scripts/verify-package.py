"""Verify exactly the published ZIP, then run its contents from an isolated directory."""
import argparse
import hashlib
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
import tomllib
import zipfile

ROOT=Path(__file__).resolve().parents[1]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify(dist, version):
    base=f"shipforge-{version}-windows-x64"
    archive=dist/(base+".zip")
    assert (dist/(archive.name+".sha256")).read_text(encoding="utf-8").split()==[digest(archive),archive.name]
    with tempfile.TemporaryDirectory(prefix="shipforge-zip-") as directory:
        tmp=Path(directory)
        with zipfile.ZipFile(archive) as package:
            names=set()
            for item in package.infolist():
                path=PurePosixPath(item.filename.replace("\\","/"))
                assert path.parts and path.parts[0]==base and ".." not in path.parts and all(":" not in part for part in path.parts), path
                if item.is_dir(): continue
                assert str(path) not in names, path
                names.add(str(path))
                out=tmp.joinpath(*path.parts)
                out.parent.mkdir(parents=True,exist_ok=True)
                out.write_bytes(package.read(item))
            assert package.testzip() is None
        payload=tmp/base
        checksums=(payload/"SHA256SUMS.txt").read_text(encoding="utf-8").splitlines()
        checked=set()
        for line in checksums:
            checksum,name=line.split()
            assert name not in checked and len(PurePosixPath(name).parts)==1
            checked.add(name)
            assert digest(payload/name)==checksum
        assert checked=={"shipforge.exe","shipforge-desktop.exe","shipforge-server.exe","shipforge-worker.exe"}
        for file in ("LICENSE","docs/cli.md","docs/protocol.schema.json","skills/shipforge/SKILL.md","licenses/LUCIDE-LICENSE.txt"):
            assert (payload/file).is_file(), file
        subprocess.run([sys.executable,str(ROOT/"scripts/verify-release.py"),str(payload),"--expected-version",version],check=True)
        subprocess.run([sys.executable,str(ROOT/"scripts/verify-team.py"),str(payload)],check=True)
    print(f"Verified {archive.name} and SHA-256 {digest(archive)}.")


if __name__=="__main__":
    cli=argparse.ArgumentParser()
    cli.add_argument("directory",type=Path)
    cli.add_argument("--expected-version",default=tomllib.loads((ROOT/"Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"])
    args=cli.parse_args()
    verify(args.directory.resolve(),args.expected_version)
