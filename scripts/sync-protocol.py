"""Regenerate or compare the schema emitted by the compiled CLI."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--check",action="store_true")
    parser.add_argument("--binary",type=Path)
    args=parser.parse_args()
    executable=args.binary or Path(os.environ.get("CARGO_TARGET_DIR",ROOT/"target"))/"debug"/("shipforge.exe" if os.name=="nt" else "shipforge")
    result=json.loads(subprocess.check_output([str(executable.resolve()),"schema"],encoding="utf-8"))
    if not result["ok"]: raise SystemExit("CLI did not return its schema")
    schema=result["data"]
    path=ROOT/"docs/protocol.schema.json"
    text=json.dumps(schema,ensure_ascii=False,indent=2)+"\n"
    if args.check:
        if json.loads(path.read_text(encoding="utf-8"))!=schema:raise SystemExit("Regenerate docs/protocol.schema.json with scripts/sync-protocol.py")
    else:
        path.write_text(text,encoding="utf-8",newline="\n")
    ops=schema["properties"]["op"]["enum"]
    assert len(ops)==len(set(ops))
    manual=(ROOT/"docs/cli.md").read_text(encoding="utf-8")
    missing=[op for op in ops if f"`{op}`" not in manual]
    if missing: raise SystemExit("The CLI manual has no entry for "+", ".join(missing))
    print(f"Validated {len(ops)} named protocol operations and their CLI manual entries.")


if __name__=="__main__":
    main()
