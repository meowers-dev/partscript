"""Per-copy variety: i, rand(), pick(), scatter (*N~), when= and fade=."""

from __future__ import annotations

import math
import textwrap

import pytest

import partscript as ps


def build(text: str, name: str):
	project = ps.Project.from_text(textwrap.dedent(text), "v.parts")
	assert project.errors == [] and project.check()["errors"] == [], project.check()["errors"]
	return project, project.build(name, glb=False)


def boxes(built) -> list[tuple]:
	"""(x centre, height) of each box in a part made only of boxes (6 faces each, in order)."""
	faces = built.parts[0].faces
	out = []
	for k in range(0, len(faces), 6):
		pts = [p for f in faces[k:k + 6] for p in f.points]
		out.append((round(sum(p[0] for p in pts) / len(pts), 4), round(max(p[2] for p in pts) - min(p[2] for p in pts), 4)))
	return out


def test_rand_differs_per_copy_and_repeats_per_build() -> None:
	text = """
	prop posts "Posts"
	  b 0,0,~ .1,.1,rand(.2,1) wood *8@.3,0,0
	"""
	_, first = build(text, "posts")
	heights = [h for _, h in boxes(first)]
	assert len(set(heights)) == 8 and all(.2 <= h <= 1 for h in heights)
	_, again = build(text, "posts")
	assert boxes(again) == boxes(first)


def test_comments_and_other_lines_leave_draws_alone() -> None:
	# A line's draws follow its own words, not its line number: tidying a file doesn't deal it again.
	posts = "  b 0,0,~ .1,.1,rand(.2,1) wood *6@.3,0,0\n"
	plain = build('prop aa "A"\n' + posts, "aa")[1]
	tidied = build('prop aa "A"\n  # posts\n\n  c 0,0,2 .1 .1 wood\n' + posts, "aa")[1]
	assert boxes_last(tidied, 6) == boxes(plain)


def boxes_last(built, count: int) -> list[tuple]:
	"""(x centre, height) of the last count boxes of a part."""
	faces = built.parts[0].faces[-6 * count:]
	out = []
	for k in range(0, len(faces), 6):
		pts = [p for f in faces[k:k + 6] for p in f.points]
		out.append((round(sum(p[0] for p in pts) / len(pts), 4), round(max(p[2] for p in pts) - min(p[2] for p in pts), 4)))
	return out


def test_seed_reshuffles_a_prop() -> None:
	one = build('prop aa "A" seed=1\n  b 0,0,~ .1,.1,rand(.2,1) wood *4@.3,0,0\n', "aa")[1]
	two = build('prop aa "A" seed=2\n  b 0,0,~ .1,.1,rand(.2,1) wood *4@.3,0,0\n', "aa")[1]
	assert boxes(one) != boxes(two)


def test_i_is_the_copy_number() -> None:
	_, built = build("""
	prop stairs "Stairs"
	  b 0,0,~ .3,.3,.1+i*.1 wood *5@.3,0,0
	""", "stairs")
	assert [h for _, h in boxes(built)] == [.1, .2, .3, .4, .5]


def test_pick_chooses_per_copy_and_every_choice_is_a_material() -> None:
	project, built = build("""
	def flower col=#ffffff/plastic
	  sph 0,0,.3 .04 col s=6 rings=3
	prop bed "Bed"
	  use flower *6x6@.12,.12 col=pick(#e84a6a,#f0d040,#f0f0f0)/plastic
	""", "bed")
	used = {f.material for f in built.parts[0].faces}
	assert used == {"xe84a6a_plastic", "xf0d040_plastic", "xf0f0f0_plastic"}
	assert all(key in project.host.materials for key in used)


def test_scatter_stays_in_its_area_and_apart() -> None:
	_, built = build("""
	prop stones "Stones"
	  b 0,0,~ .05 stone *20~2,1,.15
	""", "stones")
	centres = [(sum(p[0] for f in built.parts[0].faces[k:k + 6] for p in f.points) / 24,
		sum(p[1] for f in built.parts[0].faces[k:k + 6] for p in f.points) / 24) for k in range(0, 120, 6)]
	assert len(centres) == 20
	assert all(abs(x) <= 1 and abs(y) <= .5 for x, y in centres)
	closest = min(math.dist(a, b) for n, a in enumerate(centres) for b in centres[n + 1:])
	assert closest >= .15 - 1e-9


def test_when_skips_copies_and_ends_recursion() -> None:
	_, built = build("""
	prop gaps "Gaps"
	  b 0,0,~ .1 wood *6@.2,0,0 when=i%2==0
	""", "gaps")
	assert len(boxes(built)) == 3
	project, tree = build("""
	def branch len=1 depth=3
	  c 0,0,~ .03*len len wood s=5
	  use branch 0,0,len r=30,0,i*120 s=.7 len=len depth=depth-1 *3%0 when=depth>0
	prop tree "Tree"
	  use branch
	""", "tree")
	# 1 + 3 + 9 + 27 branches of 5 sides (10 side faces + 2 caps each).
	assert len(tree.parts[0].faces) == 40 * 7


def test_fade_darkens_toward_the_base() -> None:
	_, built = build("""
	prop tuft "Tuft"
	  b 0,0,~ .1,.1,1 wood fade=.5
	""", "tuft")
	side = next(f for f in built.parts[0].faces if len({round(p[2], 3) for p in f.points}) == 2)
	shades = {round(p[2], 3): s for p, s in zip(side.points, side.corner_shade)}
	assert shades[0.0] == pytest.approx(.5) and shades[1.0] == pytest.approx(1.0)


def test_check_counts_scatter_and_validates_picks() -> None:
	project = ps.Project.from_text(textwrap.dedent("""
	prop aa "A"
	  b 0,0,0 .1 wood *10~1 fade=2
	  b 0,0,0 .1 pick(wood,velvet_curtain)
	"""))
	report = project.check()
	assert report["props"][0]["triangles"] == 10 * 12 + 12
	assert any("fade=2" in e for e in report["errors"])
	assert any("velvet_curtain" in e for e in report["errors"])


def test_unquoted_spaces_in_an_expression_are_an_error() -> None:
	project = ps.Project.from_text('prop aa "A"\n  b 0 .1 wood *4@.2,0,0 when=i<1 or i>2\n')
	assert any("in quotes" in e for e in project.errors), project.errors
	_, built = build('prop aa "A"\n  b 0 .1 wood *4@.2,0,0 when="i<1 or i>2"\n', "aa")
	assert len(boxes(built)) == 2


def test_label_text_is_drawn_per_copy() -> None:
	project, built = build("""
	prop stones "Stones"
	  label 0,0,1 .5 .2 "pick(ADA,NELL,RUTH) {floor(rand(1800,1899))}" bg=#7c7c76 fg=#202020 *6@.6,0,0
	""", "stones")
	keys = {f.material for f in built.parts[0].faces}
	texts = {project.host.textures.recipes[project.host.materials[k].texture][1]["text"] for k in keys}
	assert len(texts) >= 4 and all(t.split()[0] in ("ADA", "NELL", "RUTH") and 1800 <= int(t.split()[1]) <= 1899 for t in texts)
	assert built.glb == b"" and project.build("stones").warnings == []


def test_wobble_moves_corners_and_keeps_shared_ones_together() -> None:
	_, plain = build('prop aa "A"\n  b 0,0,0 .4 wood\n', "aa")
	_, rough = build('prop aa "A"\n  b 0,0,0 .4 wood wobble=.03\n', "aa")
	corners = {(round(p[0], 6), round(p[1], 6), round(p[2], 6)) for f in rough.parts[0].faces for p in f.points}
	assert len(corners) == 8
	assert corners != {(round(p[0], 6), round(p[1], 6), round(p[2], 6)) for f in plain.parts[0].faces for p in f.points}


def test_odds_comes_up_as_often_as_its_weights() -> None:
	text = "def fated\n  set fate=odds(60,30,10)\n  b fate,0,~ .1,.1,.1 wood\n\nprop aa \"A\"\n  use fated *400@0,0,0\n"
	_, built = build(text, "aa")
	xs = [x for x, _ in boxes(built)]
	counts = [sum(1 for x in xs if abs(x - k) < .01) for k in range(3)]
	assert sum(counts) == 400
	assert 200 < counts[0] < 280 and 90 < counts[1] < 150 and 20 < counts[2] < 65


def test_odds_never_draws_a_weight_of_nothing_and_rejects_no_weights() -> None:
	_, built = build('prop aa "A"\n  b odds(0,5,0),0,~ .1,.1,.1 wood *50@0,0,0\n', "aa")
	assert {x for x, _ in boxes(built)} == {1.0}
	project = ps.Project.from_text('prop bb "B"\n  b odds(0,0),0,~ .1,.1,.1 wood\n', "v.parts")
	with pytest.raises(ps.PartScriptError, match="odds"):
		project.build("bb", glb=False)


def test_build_seed_deals_another_variant_the_same_every_time() -> None:
	project, plain = build('prop aa "A"\n  b 0,0,~ .1,.1,rand(.2,1) wood *6@.3,0,0\n', "aa")
	one = project.build("aa", glb=False, seed=4817)
	again = project.build("aa", glb=False, seed=4817)
	assert boxes(one) == boxes(again) != boxes(plain)
	assert boxes(project.build("aa", glb=False)) == boxes(plain)
