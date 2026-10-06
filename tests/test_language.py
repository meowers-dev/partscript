from __future__ import annotations

import textwrap

import pytest

import partscript as ps
from partscript import lang
from partscript.host import Host


def check(text: str) -> dict:
	return ps.Project.from_text(textwrap.dedent(text), "<t>", Host(prefix="ld")).check()


class TestExpressions:
	def test_numbers_expressions_and_names(self) -> None:
		assert ps.evaluate("1.5", {}) == 1.5
		assert ps.evaluate("w/2+.1", {"w": 1.2}) == pytest.approx(0.7)
		assert ps.evaluate("sin(30)*2", {}) == pytest.approx(1.0)
		assert ps.evaluate("max(a,b)", {"a": 1, "b": "a+2"}) == 3.0
		with pytest.raises(ValueError):
			ps.evaluate("nope*2", {})
		with pytest.raises(ValueError):
			ps.evaluate("__import__('os')", {})

	def test_inverse_trig_answers_in_degrees(self) -> None:
		assert ps.evaluate("atan(1.5/6)", {}) == pytest.approx(14.036, abs=1e-3)
		assert ps.evaluate("asin(.5)", {}) == pytest.approx(30.0)

	def test_split_top_respects_parentheses(self) -> None:
		assert lang.split_top("max(1,2),0,w/2") == ["max(1,2)", "0", "w/2"]


class TestParse:
	def test_statements_blocks_and_placeholders(self) -> None:
		assert lang._split_statements("at 0,0,1 mx { b 0 1 wood ; c 0 .1 1 wood }") == ["at 0,0,1 mx {", "b 0 1 wood", "c 0 .1 1 wood", "}"]
		# {v} is a variant placeholder, not a block.
		assert lang._split_statements("prop cone_{c} for c=a,b ; b 0 1 #{c}/plastic") == ["prop cone_{c} for c=a,b", "b 0 1 #{c}/plastic"]
		assert lang._strip_comment("b 0 1 #aa3322 # a red box") == "b 0 1 #aa3322 "

	def test_copies_count_triangles(self) -> None:
		report = check("""
		prop legs_test "Legs"
		  b .4,.3,~ .05,.05,.7 wood mx my
		  c 0,0,.5 .1 .2 wood s=8 *3@.3,0,0
		  b 0,0,0 .1 wood *2x3@.2,.2
		""")
		assert report["errors"] == []
		# 4 boxes, 3 cylinders of 8 sides (16 + 12 cap tris), a 2x3 grid of boxes.
		assert report["props"][0]["triangles"] == 4 * 12 + 3 * 28 + 6 * 12

	def test_variants_expand_every_combination(self) -> None:
		report = check("""
		prop crate_{c}_{s} "Crate {c}" for c=red,blue for s=small,big
		  b 0,0,~ .5 #aa3322
		""")
		assert sorted(p["id"] for p in report["props"]) == ["ld_crate_blue_big", "ld_crate_blue_small", "ld_crate_red_big", "ld_crate_red_small"]

	def test_defs_params_and_std_parts(self) -> None:
		report = check("""
		def plank len=1 m=wood
		  b 0,0,~ len,.2,.03 m
		prop shelf_test "Shelf"
		  use plank len=2 m=#ffffff/paint
		  use table 0,0,0 w=1 top=#553322/wood
		  use crate 1,0,0 fill=none
		""")
		assert report["errors"] == []
		bad = check("""
		prop use_test
		  use table color=red
		""")
		assert any("no parameter 'color'" in e for e in bad["errors"]), bad["errors"]

	def test_reserved_parameter_and_duplicate_options(self) -> None:
		report = check("""
		def wheelish r=.3
		  c 0,0,0 r .1 wood
		prop dup_opts
		  b 0 1 wood r=0,0,1 r=0,0,2
		""")
		assert any("taken by use" in e for e in report["errors"]), report["errors"]
		assert any("r= given twice" in e for e in report["errors"]), report["errors"]

	def test_a_bad_line_does_not_sink_the_prop(self) -> None:
		report = check("""
		prop resilient "Resilient"
		  b 0 1 wood
		  frobnicate 1 2 3
		  b 0,0,1 1 wood
		""")
		assert [p["id"] for p in report["props"]] == ["ld_resilient"]
		assert len(report["errors"]) == 1, report["errors"]
		assert "unknown statement 'frobnicate'" in report["errors"][0]

	def test_materials_colours_and_unknowns(self) -> None:
		report = check("""
		prop mats_test
		  b 0 1 #aa3322/metal
		  b 0 1 wood|steel_dark
		  b 0 1 #aa3322/velvet
		  b 0 1 unobtainium
		""")
		errors = " ".join(report["errors"])
		assert "velvet" in errors
		assert "unobtainium" in errors
		assert len(report["errors"]) == 2, report["errors"]
		assert lang.colour_key("ld", "#AA3322/metal") == ("ld_xaa3322_metal", "aa3322", "metal")

	def test_uses_of_missing_parts_fail(self) -> None:
		report = check("""
		prop use_missing
		  use nothing_called_this
		""")
		assert any("nothing_called_this" in e for e in report["errors"])

	def test_styles_and_dressing(self) -> None:
		report = check("""
		prop fruit_stall "Stall"
		  use stall
		style test_market exterior=1
		  wall fruit_stall repeat=1,2 hero=1 gap=1.2
		  decals grime crack
		dressing clutter fruit_stall:0.4 missing_piece
		""")
		assert "test_market" in report["styles"]
		assert [e for e in report["errors"]] == ["dressing clutter: no piece 'missing_piece'"]
		program = ps.Program()
		ps.parse("style s\n  wall a repeat=1,3 hero=1 front=b\n", "<t>", program)
		assert program.styles["s"]["wall"] == [{"piece": "a", "repeat": [1, 3], "hero": True, "front": "b"}]

	def test_std_parts_check_clean(self) -> None:
		project = ps.Project([])
		assert project.errors == []
		assert project.check()["errors"] == []


class TestLanguage:
	def test_an_unindented_set_is_file_scope(self) -> None:
		report = check("""
		prop st_a "A"
		  b 0,0,0 .1 wood
		set k=3
		prop st_b "B"
		  b 0,0,0 .1 wood *(k)@.2,0,0
		""")
		assert report["errors"] == []
		assert [p["triangles"] for p in report["props"]] == [12, 36]

	def test_hang_bends_a_row_not_a_grid(self) -> None:
		assert check("""
		prop hg_a "Bunting"
		  b 0,0,2 .1 wood *9@.4,0,0 hang=.25
		""")["errors"] == []
		errors = check("""
		prop hg_b "Grid"
		  b 0,0,2 .1 wood *3x3@.4,.4 hang=.25
		""")["errors"]
		assert any("hang=" in e for e in errors), errors

	def test_placement_options_are_checked_before_building(self) -> None:
		errors = check("""
		prop ro_a "Ramp"
		  b 0,0,0 1 wood r=0,nope(1),0
		  b 0,0,0 1 wood r=0,atan(.25),0
		""")["errors"]
		assert len(errors) == 1, errors
		assert "nope" in errors[0]

	def test_sign_textures_are_powers_of_two(self) -> None:
		errors = check("""
		prop sg_a "Sign"
		  sign 0,0,1 1 .8 "HI" tex=128x96
		  sign 0,0,2 1 .8 "HO" tex=128x128
		""")["errors"]
		assert len(errors) == 1, errors
		assert "powers of two" in errors[0]

	def test_trim_needs_a_decal_sheet(self) -> None:
		errors = check("""
		prop tr_a "Trim"
		  trim 0,-.01,1 .2 .2 knob
		""")["errors"]
		assert any("no decal sheets" in e for e in errors), errors
