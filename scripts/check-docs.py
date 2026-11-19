"""Check the feature and screen catalogue and local document links."""

import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def main():
    docs = sorted((ROOT / "docs").glob("*.md")) + sorted(ROOT.glob("*.md"))
    errors = []
    catalogue = (ROOT / "docs/features.md").read_text(encoding="utf-8")
    features = re.findall(r"^\| (F\d{2,}) ", catalogue, re.MULTILINE)
    if not features or len(set(features)) != len(features):
        errors.append("The feature catalogue must have unique feature IDs.")

    screen_ids = set()
    interfaces = (ROOT / "docs/interfaces.md").read_text(encoding="utf-8")
    in_catalogue = False
    for line in interfaces.splitlines():
        if line.startswith("| 界面 | 截图索引 |"):
            in_catalogue = True
            continue
        if not line.startswith("|"):
            in_catalogue = False
        if not in_catalogue or line.startswith("| ---"):
            continue
        name, image, references, _description = [cell.strip() for cell in line.strip("|").split("|")]
        screen = name.split()[0]
        if not re.fullmatch(r"UI\d{2,}", screen) or screen in screen_ids:
            errors.append(f"Unexpected or duplicate screen ID: {screen}")
        screen_ids.add(screen)
        if not re.fullmatch(rf"`{screen[2:]}-[a-z0-9-]+\.png`", image):
            errors.append(f"{screen} has an inconsistent image name: {image}")
        for feature in references.split("、"):
            if feature not in features:
                errors.append(f"{screen} refers to unknown feature {feature}")
    if not screen_ids:
        errors.append("The screen catalogue is empty.")

    for doc in docs:
        for number, line in enumerate(doc.read_text(encoding="utf-8").splitlines(), 1):
            links = re.findall(r"\]\(([^)]+)\)", line) + re.findall(r"^\[[^\]]+\]:\s+(\S+)", line)
            for destination in links:
                target = urlsplit(destination.strip("<>"))
                if target.scheme or target.netloc or not target.path:
                    continue
                path = (doc.parent / unquote(target.path)).resolve()
                if not path.is_relative_to(ROOT) or not path.is_file():
                    errors.append(f"{doc.relative_to(ROOT)}:{number} missing local file {destination}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Checked {len(docs)} documents, {len(features)} feature groups and {len(screen_ids)} initial UI captures.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
