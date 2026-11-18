"""Check the public product inventory and optionally compare the current official navigation."""

import argparse
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
from urllib.parse import urlsplit
from urllib.request import Request, urlopen
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs" / "parity-inventory.md"
JUDGMENTS = {"有对应", "部分", "缺口", "待核", "预告", "跨端"}
STRENGTH = ("实机 ·", "测试 ·", "实现 ·", "未证 ·")
REFERENCE = re.compile(r"\[[^\]]+\]\[([a-z0-9-]+)\]")
DEFINITION = re.compile(r"^\[([a-z0-9-]+)\]: (\S+)$", re.MULTILINE)


class Navigation(HTMLParser):
    def __init__(self):
        super().__init__()
        self.docs = set()

    def handle_starttag(self, tag, attrs):
        if tag != "a":
            return
        url = dict(attrs).get("href", "")
        if url.startswith("/docs/") and "#" not in url and "?" not in url:
            self.docs.add(url.rstrip("/"))


def get(url):
    req = Request(url, headers={"User-Agent": "ShipForge public documentation inventory"})
    with urlopen(req, timeout=20) as response:
        return response.read()


def difference(label, current, documented):
    errors = [f"{label} not mapped: {item}" for item in sorted(current - documented)]
    return errors + [f"{label} no longer listed: {item}" for item in sorted(documented - current)]


def compare_navigation(sources):
    nav = Navigation()
    nav.feed(get(sources["welcome"]).decode("utf-8"))
    help_paths = {
        urlsplit(url).path.rstrip("/")
        for url in sources.values()
        if url.startswith("https://help.hifox.com/docs/")
    }
    sitemap = ElementTree.fromstring(get(sources["sitemap"]))
    site_paths = {urlsplit(node.text).path for node in sitemap.findall(".//{*}loc") if node.text}
    canonical = {path.removeprefix("/en") or "/" if path.startswith("/en/") else path for path in site_paths}
    documented = {
        urlsplit(url).path
        for url in sources.values()
        if url.startswith("https://hifox.com/") and url != sources["sitemap"]
    }
    print(f"Official navigation: {len(nav.docs)} help pages, {len(site_paths)} sitemap URLs, {len(canonical)} language pairs.")
    return difference("help page", nav.docs, help_paths) + difference("product page", canonical, documented)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--online", action="store_true", help="compare public help navigation and the product sitemap")
    args = parser.parse_args()
    text = INVENTORY.read_text(encoding="utf-8")
    errors = []
    items = DEFINITION.findall(text)
    sources = dict(items)
    if len(items) != len(sources):
        errors.append("Duplicate source definition.")
    used = set(REFERENCE.findall(text))
    errors.extend(difference("reference", used, set(sources)))
    for name, path in sources.items():
        source_prefix = "https://github.com/Hubujiu/ShipForge/blob/main/"
        if path.startswith(source_prefix):
            target = (ROOT / path.removeprefix(source_prefix)).resolve()
        elif path.startswith("https://"):
            continue
        else:
            target = (INVENTORY.parent / path).resolve()
        if not target.is_relative_to(ROOT) or not target.is_file():
            errors.append(f"{name}: missing repository source {path}")

    ids = set()
    counts = Counter()
    screen_refs = set()
    supplements = set()
    for line_no, line in enumerate(text.splitlines(), 1):
        if not line.startswith("| ") or not re.match(r"\| [A-Z]\d", line):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) != 7 or any(not cell for cell in cells):
            errors.append(f"line {line_no}: expected seven populated cells")
            continue
        key, _, reference, judgment, evidence, dimensions, _ = cells
        labels = set(dimensions.split("、"))
        if "行为" not in labels or labels - {"行为", "数据", "权限", "待联调"}:
            errors.append(f"{key}: unrecognized acceptance dimensions {dimensions}")
        if not re.fullmatch(r"[A-Z]\d{2,}", key) or key in ids:
            errors.append(f"line {line_no}: malformed or duplicate ID {key}")
        ids.add(key)
        if judgment not in JUDGMENTS:
            errors.append(f"{key}: unknown comparison {judgment}")
        counts[judgment] += 1
        if not (REFERENCE.search(reference) or "图 " in reference or "菜单 " in reference):
            errors.append(f"{key}: missing reference-side evidence")
        if not evidence.startswith(STRENGTH) or not REFERENCE.search(evidence):
            errors.append(f"{key}: missing ShipForge evidence type or pointer")
        if evidence.startswith("实机 ·") and "native" not in REFERENCE.findall(evidence):
            errors.append(f"{key}: label for desktop observation needs its Windows record")
        screen_refs.update(re.findall(r"UI\d{2,}", reference))
        supplements.update(re.findall(r"补\d{2,}", reference))
    if not ids:
        errors.append("No inventory rows found.")
    ui_index = (ROOT / "docs" / "interfaces.md").read_text(encoding="utf-8")
    captured = set(re.findall(r"^\| (UI\d{2,}) ", ui_index, re.MULTILINE))
    errors.extend(difference("published screenshot entry", captured, screen_refs))
    errors.extend(difference("supplemental setting", {f"补{i:02}" for i in range(1, 8)}, supplements))
    if args.online:
        try:
            errors.extend(compare_navigation(sources))
        except (OSError, ValueError, ElementTree.ParseError) as exc:
            errors.append(f"Official navigation could not be checked: {exc}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Checked {len(ids)} behavior entries, {len(sources)} sources, {len(counts)} comparison classes.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
