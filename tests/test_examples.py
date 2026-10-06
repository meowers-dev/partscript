from __future__ import annotations

import re
from pathlib import Path

import partscript as ps

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples"


def test_examples_check_and_build_clean() -> None:
	project = ps.Project.from_paths([EXAMPLES])
	assert project.errors == []
	report = project.check()
	assert report["errors"] == [] and report["warnings"] == []
	for prop in project.props():
		built = project.build(prop["id"])
		assert built.triangles > 0 and built.warnings == [], (prop["id"], built.warnings)


def test_readme_snippets_come_from_the_examples() -> None:
	"""Every PartScript block in the README is lines of an example file, in order (it may leave lines out)."""
	from partscript.lang import _join_continued
	joined = lambda text: "\n".join(line for line in _join_continued(text.splitlines()) if line is not None)  # noqa: E731
	text = "\n".join(joined(p.read_text()) for p in sorted(EXAMPLES.glob("*.parts")))
	readme = (ROOT / "README.md").read_text()
	fenced = re.findall(r"^```(\w*)\n(.*?)^```$", readme, re.S | re.M)
	blocks = [body for lang, body in fenced if not lang and body.lstrip().startswith(("prop", "def"))]
	assert len(blocks) >= 4
	lines = text.splitlines()
	for block in blocks:
		at = 0
		for line in joined(block.strip("\n")).splitlines():
			if not line.strip():
				continue
			assert line in lines[at:], f"README line not in the examples (in order): {line!r}"
			at = lines.index(line, at) + 1
