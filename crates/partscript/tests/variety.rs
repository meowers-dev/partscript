//! Per-copy variety: i, rand(), pick(), scatter (*N~), when=, fade=, wobble=, odds() and seeds.
mod common;

use std::collections::HashSet;

use common::*;
use partscript::{BuildOptions, Host, Project, Recipe};

#[test]
fn rand_differs_per_copy_and_repeats_per_build() {
	let text = "
	prop posts \"Posts\"
	  b 0,0,~ .1,.1,rand(.2,1) wood *8@.3,0,0
	";
	let (_, first) = build(text, "posts");
	let heights: Vec<f64> = boxes(&first).iter().map(|(_, h)| *h).collect();
	let distinct: HashSet<u64> = heights.iter().map(|h| h.to_bits()).collect();
	assert!(distinct.len() == 8 && heights.iter().all(|h| (0.2..=1.0).contains(h)));
	let (_, again) = build(text, "posts");
	assert_eq!(boxes(&again), boxes(&first));
}

#[test]
fn comments_and_other_lines_leave_draws_alone() {
	// A line's draws follow its own words, not its line number: tidying a file doesn't deal it again.
	let posts = "  b 0,0,~ .1,.1,rand(.2,1) wood *6@.3,0,0\n";
	let plain = build(&format!("prop aa \"A\"\n{posts}"), "aa").1;
	let tidied = build(&format!("prop aa \"A\"\n  # posts\n\n  c 0,0,2 .1 .1 wood\n{posts}"), "aa").1;
	assert_eq!(boxes_last(&tidied, 6), boxes(&plain));
}

#[test]
fn seed_reshuffles_a_prop() {
	let one = build("prop aa \"A\" seed=1\n  b 0,0,~ .1,.1,rand(.2,1) wood *4@.3,0,0\n", "aa").1;
	let two = build("prop aa \"A\" seed=2\n  b 0,0,~ .1,.1,rand(.2,1) wood *4@.3,0,0\n", "aa").1;
	assert_ne!(boxes(&one), boxes(&two));
}

#[test]
fn i_is_the_copy_number() {
	let (_, built) = build(
		"
	prop stairs \"Stairs\"
	  b 0,0,~ .3,.3,.1+i*.1 wood *5@.3,0,0
	",
		"stairs",
	);
	assert_eq!(boxes(&built).iter().map(|(_, h)| *h).collect::<Vec<_>>(), vec![0.1, 0.2, 0.3, 0.4, 0.5]);
}

#[test]
fn pick_chooses_per_copy_and_every_choice_is_a_material() {
	let (project, built) = build(
		"
	def flower col=#ffffff/plastic
	  sph 0,0,.3 .04 col s=6 rings=3
	prop bed \"Bed\"
	  use flower *6x6@.12,.12 col=pick(#e84a6a,#f0d040,#f0f0f0)/plastic
	",
		"bed",
	);
	let used: HashSet<String> = built.parts[0].faces.iter().map(|f| f.material.to_string()).collect();
	assert_eq!(used, HashSet::from(["xe84a6a_plastic".to_string(), "xf0d040_plastic".to_string(), "xf0f0f0_plastic".to_string()]));
	assert!(used.iter().all(|k| project.host.has_material(k)));
}

#[test]
fn scatter_stays_in_its_area_and_apart() {
	let (_, built) = build(
		"
	prop stones \"Stones\"
	  b 0,0,~ .05 stone *20~2,1,.15
	",
		"stones",
	);
	let faces = &built.parts[0].faces;
	let centres: Vec<[f64; 2]> = (0..120)
		.step_by(6)
		.map(|k| {
			let pts: Vec<[f64; 3]> = faces[k..k + 6].iter().flat_map(|f| f.points.iter().copied()).collect();
			[pts.iter().map(|p| p[0]).sum::<f64>() / 24.0, pts.iter().map(|p| p[1]).sum::<f64>() / 24.0]
		})
		.collect();
	assert_eq!(centres.len(), 20);
	assert!(centres.iter().all(|c| c[0].abs() <= 1.0 && c[1].abs() <= 0.5));
	let mut closest = f64::INFINITY;
	for (n, a) in centres.iter().enumerate() {
		for b in &centres[n + 1..] {
			closest = closest.min(kitlib::py::dist(a, b));
		}
	}
	assert!(closest >= 0.15 - 1e-9);
}

#[test]
fn when_skips_copies_and_ends_recursion() {
	let (_, built) = build(
		"
	prop gaps \"Gaps\"
	  b 0,0,~ .1 wood *6@.2,0,0 when=i%2==0
	",
		"gaps",
	);
	assert_eq!(boxes(&built).len(), 3);
	let (_, tree) = build(
		"
	def branch len=1 depth=3
	  c 0,0,~ .03*len len wood s=5
	  use branch 0,0,len r=30,0,i*120 s=.7 len=len depth=depth-1 *3%0 when=depth>0
	prop tree \"Tree\"
	  use branch
	",
		"tree",
	);
	// 1 + 3 + 9 + 27 branches of 5 sides (10 side faces + 2 caps each).
	assert_eq!(tree.parts[0].faces.len(), 40 * 7);
}

#[test]
fn fade_darkens_toward_the_base() {
	let (_, built) = build(
		"
	prop tuft \"Tuft\"
	  b 0,0,~ .1,.1,1 wood fade=.5
	",
		"tuft",
	);
	let side = built.parts[0]
		.faces
		.iter()
		.find(|f| f.points.iter().map(|p| round(p[2], 3).to_bits()).collect::<HashSet<_>>().len() == 2)
		.unwrap();
	let shades = side.corner_shade.as_ref().unwrap();
	for (p, s) in side.points.iter().zip(shades) {
		let want = if round(p[2], 3) == 0.0 { 0.5 } else { 1.0 };
		assert!((s - want).abs() < 1e-9);
	}
}

#[test]
fn check_counts_scatter_and_validates_picks() {
	let project = project(
		"
	prop aa \"A\"
	  b 0,0,0 .1 wood *10~1 fade=2
	  b 0,0,0 .1 pick(wood,velvet_curtain)
	",
		"<text>",
	);
	let report = project.check();
	assert_eq!(report.props[0].triangles, 10 * 12 + 12);
	assert!(has(&report.errors, "fade=2"));
	assert!(has(&report.errors, "velvet_curtain"));
}

#[test]
fn unquoted_spaces_in_an_expression_are_an_error() {
	let p = Project::from_text("prop aa \"A\"\n  b 0 .1 wood *4@.2,0,0 when=i<1 or i>2\n", "<text>", Host::default());
	assert!(has(&p.errors(), "in quotes"), "{:?}", p.errors());
	let (_, built) = build("prop aa \"A\"\n  b 0 .1 wood *4@.2,0,0 when=\"i<1 or i>2\"\n", "aa");
	assert_eq!(boxes(&built).len(), 2);
}

#[test]
fn label_text_is_drawn_per_copy() {
	let (mut project, built) = build(
		"
	prop stones \"Stones\"
	  label 0,0,1 .5 .2 \"pick(ADA,NELL,RUTH) {floor(rand(1800,1899))}\" bg=#7c7c76 fg=#202020 *6@.6,0,0
	",
		"stones",
	);
	let keys: HashSet<String> = built.parts[0].faces.iter().map(|f| f.material.to_string()).collect();
	let texts: HashSet<String> = keys
		.iter()
		.map(|k| {
			let texture = project.host.materials.borrow()[k].texture.clone();
			match &project.host.textures.borrow().recipes[&texture] {
				Recipe::Sign(spec) => spec.get("text").unwrap().as_str().to_string(),
				_ => panic!(),
			}
		})
		.collect();
	assert!(texts.len() >= 4);
	for t in &texts {
		let words: Vec<&str> = t.split_whitespace().collect();
		assert!(["ADA", "NELL", "RUTH"].contains(&words[0]));
		assert!((1800..=1899).contains(&words[1].parse::<i64>().unwrap()));
	}
	assert!(built.glb.is_empty());
	assert!(project.build("stones", &BuildOptions::default()).unwrap().warnings.is_empty());
}

#[test]
fn wobble_moves_corners_and_keeps_shared_ones_together() {
	let (_, plain) = build("prop aa \"A\"\n  b 0,0,0 .4 wood\n", "aa");
	let (_, rough) = build("prop aa \"A\"\n  b 0,0,0 .4 wood wobble=.03\n", "aa");
	let corners = |b: &partscript::Built| -> HashSet<[u64; 3]> {
		b.parts[0].faces.iter().flat_map(|f| f.points.iter().map(|p| p.map(|v| round(v, 6).to_bits()))).collect()
	};
	assert_eq!(corners(&rough).len(), 8);
	assert_ne!(corners(&rough), corners(&plain));
}

#[test]
fn odds_comes_up_as_often_as_its_weights() {
	let text = "def fated\n  set fate=odds(60,30,10)\n  b fate,0,~ .1,.1,.1 wood\n\nprop aa \"A\"\n  use fated *400@0,0,0\n";
	let (_, built) = build(text, "aa");
	let xs: Vec<f64> = boxes(&built).iter().map(|(x, _)| *x).collect();
	let counts: Vec<usize> = (0..3).map(|k| xs.iter().filter(|x| (*x - k as f64).abs() < 0.01).count()).collect();
	assert_eq!(counts.iter().sum::<usize>(), 400);
	assert!(200 < counts[0] && counts[0] < 280 && 90 < counts[1] && counts[1] < 150 && 20 < counts[2] && counts[2] < 65, "{counts:?}");
}

#[test]
fn odds_never_draws_a_weight_of_nothing_and_rejects_no_weights() {
	let (_, built) = build("prop aa \"A\"\n  b odds(0,5,0),0,~ .1,.1,.1 wood *50@0,0,0\n", "aa");
	assert!(boxes(&built).iter().all(|(x, _)| *x == 1.0));
	let mut p = Project::from_text("prop bb \"B\"\n  b odds(0,0),0,~ .1,.1,.1 wood\n", "v.parts", Host::default());
	let error = p.build("bb", &no_glb()).err().unwrap();
	assert!(error.to_string().contains("odds"));
}

#[test]
fn build_seed_deals_another_variant_the_same_every_time() {
	let (mut p, plain) = build("prop aa \"A\"\n  b 0,0,~ .1,.1,rand(.2,1) wood *6@.3,0,0\n", "aa");
	let seeded = BuildOptions { glb: false, seed: Some("4817".into()), ..Default::default() };
	let one = p.build("aa", &seeded).unwrap();
	let again = p.build("aa", &seeded).unwrap();
	assert_eq!(boxes(&one), boxes(&again));
	assert_ne!(boxes(&one), boxes(&plain));
	assert_eq!(boxes(&p.build("aa", &no_glb()).unwrap()), boxes(&plain));
}
