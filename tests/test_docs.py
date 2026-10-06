"""The documentation: every example builds, the reference names every statement and option, and
the pages generated from the code are current."""

from __future__ import annotations

import re
import sys
from pathlib import Path

import pytest

import partscript as ps
from partscript.check import COMMON_OPTIONS, SHAPE_OPTIONS
from partscript.lang import ALIASES, SHAPES

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
EXAMPLES = sorted((ROOT / "examples").glob("*.parts"))
BLOCK = re.compile(r"^```parts[^\n]*\n(.*?)^```", re.S | re.M)


def blocks() -> list:
	out = []
	for page in sorted(DOCS.rglob("*.md")):
		for k, match in enumerate(BLOCK.finditer(page.read_text())):
			code = match.group(1)
			if re.search(r"^(prop|def|building|kit|set) ", code, re.M):
				out.append(pytest.param(page, code, id=f"{page.relative_to(DOCS)}#{k}"))
	return out


@pytest.mark.parametrize("page, code", blocks())
def test_every_example_in_the_docs_checks_and_builds(page: Path, code: str) -> None:
	name = f"{page.relative_to(DOCS)}"
	project = ps.Project([(str(p), p.read_text()) for p in EXAMPLES] + [(name, code)])
	errors = [e for e in project.errors if name in e]
	assert errors == [], errors
	report = project.check()
	assert [e for e in report["errors"] if name in e] == []
	for prop in project.props():
		if prop["file"] == name:
			assert project.build(prop["id"], glb=False).triangles > 0


def test_the_reference_names_every_statement_and_option() -> None:
	text = (DOCS / "reference" / "statements.md").read_text()
	words = set(re.findall(r"[a-z_]+", text))
	long = {short: name for name, short in ALIASES.items() if len(name) > len(short)}
	missing = [long.get(op, op) for op in SHAPES if long.get(op, op) not in words and op not in words]
	options = {"r": "turn", "s": "sides", "rt": "top_radius", "ax": "axis", "jit": "jitter", "capm": "cap_mat", "index": "as"}
	for keys in [*SHAPE_OPTIONS.values(), COMMON_OPTIONS]:
		missing += [options.get(k, k) for k in keys if options.get(k, k) not in words and k not in ("printed",)]
	assert sorted(set(missing)) == []


def test_generated_pages_are_current() -> None:
	sys.path.insert(0, str(ROOT / "site"))
	import docs as site_docs
	for path, make in site_docs.GENERATED.items():
		assert (DOCS / path).read_text() == make(), f"{path} is out of date: run uv run python site/docs.py"


def test_every_page_is_in_the_navigation() -> None:
	sys.path.insert(0, str(ROOT / "site"))
	import docs as site_docs
	listed = {path for _, pages in site_docs.NAV for path, _ in pages}
	on_disk = {str(p.relative_to(DOCS)) for p in DOCS.rglob("*.md")}
	assert on_disk == listed


def test_links_between_pages_lead_somewhere() -> None:
	for page in DOCS.rglob("*.md"):
		for target in re.findall(r"\]\(([^)#:]+\.md)(?:#[^)]*)?\)", page.read_text()):
			assert (page.parent / target).resolve().exists(), f"{page.relative_to(DOCS)} links to {target}"
