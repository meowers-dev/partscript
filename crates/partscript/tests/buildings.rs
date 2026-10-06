//! Buildings: rooms furnished by fill= defs, door positions, cutaways, roofs.
mod common;

use common::*;
use partscript::building::{self as pb, BaseKit, FillValue, Plan};
use partscript::{evaluate, Host, Project};

const KIT: &str = "
prop wall_a \"Wall\"
  box at=0,0,on size=3,.2,3 mat=brick
prop door_a \"Door wall\"
  box at=-1.05,0,on size=.9,.2,3 mat=brick mirror x
prop slab \"Floor\"
  box at=0,0,-.1 size=3,3,.2 mat=concrete
prop lid \"Roof\"
  box at=0,0,-.1 size=3,3,.2 mat=concrete
prop rail \"Parapet\"
  box at=0,0,on size=3,.2,.6 mat=brick
kit kk grid=3 storey=3 wall=.2 parapet_lift=0
  wall solid=wall_a door=door_a
  piece floor=slab roof=lid parapet=rail
def marker colour=#ff0000/paint
  box at=w/2,d/2,on size=w*.5,d*.5,.1+level mat=colour
  box at=door_s,.2,on size=.3 mat=wood when=door_s>=0
";

fn with_kit(building: &str) -> Project {
	Project::from_text(&format!("{KIT}{}", dedent(building)), "b.parts", Host::default())
}

fn plan(p: &Project, name: &str) -> Plan {
	let prop = p.prop(name).unwrap();
	let kit = pb::resolve_kit(&p.program, prop.opt("kit").unwrap(), &BaseKit::default(), "").unwrap();
	let env = p.program.env_of(&prop.file);
	pb::plan(&prop, kit, &mut |text: &str| evaluate(text, &env))
}

fn fill_value<'a>(env: &'a [(String, FillValue)], key: &str) -> &'a FillValue {
	&env.iter().find(|(k, _)| k == key).unwrap().1
}

fn num(v: &FillValue) -> f64 {
	match v {
		FillValue::Num(n) => *n,
		FillValue::Int(n) => *n as f64,
		FillValue::Text(t) => panic!("{t}"),
	}
}

#[test]
fn a_room_fill_gets_its_size_level_and_doors() {
	let mut p = with_kit(
		"
	building house \"House\" kit=kk
	  room 0,0 2,1 storeys=2 fill=marker colour=#00ff00/paint
	  open 1,0 s door
	",
	);
	assert!(p.check().errors.is_empty(), "{:?}", p.check().errors);
	let result = plan(&p, "house");
	let fills: Vec<&pb::Placement> = result.placements.iter().filter(|pl| pl.role == "fill").collect();
	let levels: Vec<f64> = fills.iter().map(|f| num(fill_value(&f.fill.as_ref().unwrap().env, "level"))).collect();
	assert_eq!(levels, vec![0.0, 1.0]);
	let env = &fills[0].fill.as_ref().unwrap().env;
	assert_eq!(num(fill_value(env, "w")), 5.8);
	assert_eq!(num(fill_value(env, "d")), 2.8);
	assert!(matches!(fill_value(env, "colour"), FillValue::Text(t) if t == "#00ff00/paint"));
	assert_eq!(num(fill_value(env, "door_s")), 4.4);
	assert_eq!(num(fill_value(env, "door_n")), -1.0);
	assert_eq!(num(fill_value(&fills[1].fill.as_ref().unwrap().env, "door_s")), -1.0); // the door is on the ground floor only
	let built = p.build("house", &no_glb()).unwrap();
	assert!(built.parts[0].faces.iter().any(|f| &*f.material == "x00ff00_paint"));
}

#[test]
fn a_cutaway_has_no_front_walls_and_no_roof() {
	let p = with_kit(
		"
	building house_{v} \"House\" kit=kk cutaway={v} for v=closed,open
	  room 0,0 2,1
	",
	);
	let (closed, opened) = (plan(&p, "house_closed"), plan(&p, "house_open"));
	let count = |r: &Plan, role: &str| r.placements.iter().filter(|pl| pl.role == role).count();
	assert_eq!(count(&closed, "roof"), 2);
	assert_eq!(count(&opened, "roof"), 0);
	assert_eq!(count(&closed, "wall:solid"), 6);
	assert_eq!(count(&opened, "wall:solid"), 4);
}

#[test]
fn neighbouring_rooms_share_one_roof_without_a_parapet_between() {
	let p = with_kit(
		"
	building pair \"Pair\" kit=kk
	  room 0,0 1,1
	  room 1,0 1,1
	",
	);
	let parapets = plan(&p, "pair").placements.iter().filter(|pl| pl.role == "parapet").count();
	assert_eq!(parapets, 6); // round the outside of the two cells, none between them
}

#[test]
fn the_example_apartment_builds() {
	let examples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
	let mut project = Project::from_paths(&[examples], Host::default()).unwrap();
	for name in ["cottage", "apartment_closed", "apartment_open"] {
		let built = project.build(name, &no_glb()).unwrap();
		assert!(built.triangles() > 1000 && built.warnings.is_empty(), "{name} {:?}", built.warnings);
	}
}

#[test]
fn furnishing_that_leaves_its_room_is_warned_about() {
	let mut p = with_kit(
		"
	def tower
	  box at=1,1,on(i*2) size=.2 mat=wood repeat 4 every 0,0,0
	building tall \"Tall\" kit=kk
	  room 0,0 1,1 fill=tower
	",
	);
	let warnings = p.build("tall", &no_glb()).unwrap().warnings;
	assert!(has(&warnings, "reaches outside the room: z 0.00..6.20"), "{warnings:?}");
}

#[test]
fn build_warnings_reach_the_result() {
	let mut p = Project::from_text("prop big \"Big\"\n  box at=0,0,0 size=.1 mat=wood repeat 300 every .2,0,0\n", "<text>", Host::default());
	assert!(has(&p.build("big", &no_glb()).unwrap().warnings, "over the 2500 budget"));
}

#[test]
fn rooms_nobody_can_reach_are_warned_about() {
	let p = with_kit(
		"
	building flats \"Flats\" kit=kk
	  room 0,0 1,1 as=front
	  room 1,0 1,1 as=back
	  room 0,0 1,1 storey=1 as=upstairs
	  open 0,0 s door
	",
	);
	let mut warnings: Vec<String> = plan(&p, "flats").warnings.iter().map(|w| w.split_once(": ").unwrap().1.to_string()).collect();
	warnings.sort();
	assert_eq!(
		warnings,
		vec![
			"back (storey 0) cannot be reached from outside (no door or stair leads to it)",
			"upstairs (storey 1) cannot be reached from outside (no door or stair leads to it)"
		]
	);
	let p = with_kit(
		"
	building flats \"Flats\" kit=kk
	  room 0,0 1,1 as=front
	  room 1,0 1,1 as=back
	  open 0,0 s door
	  open 0,0 e door
	",
	);
	assert!(plan(&p, "flats").warnings.is_empty());
}

#[test]
fn a_stair_needs_floor_at_its_foot_and_where_it_arrives() {
	let stair = "prop steps \"Steps\"\n  box at=0,1.5,on size=1,3,3 mat=concrete\n";
	let text = format!(
		"{}{stair}{}",
		KIT.replace("  piece floor=slab roof=lid parapet=rail", "  piece floor=slab roof=lid parapet=rail stair=steps"),
		dedent(
			"
	building tower \"Tower\" kit=kk
	  room 0,0 1,2 storeys=2 as=hall
	  open 0,0 s door
	  stair 0,0 n
	"
		)
	);
	let p = Project::from_text(&text, "b.parts", Host::default());
	let warnings = plan(&p, "tower").warnings;
	assert!(has(&warnings, "no floor at its foot (cell 0,-1"), "{warnings:?}");
	let p2 = Project::from_text(&text.replace("stair 0,0 n", "stair 0,1 n").replace("room 0,0 1,2", "room 0,0 1,4"), "b.parts", Host::default());
	assert!(plan(&p2, "tower").warnings.is_empty());
}

#[test]
fn furniture_in_a_doorway_is_warned_about_but_a_rug_is_not() {
	let mut p = with_kit(
		"
	def blocker
	  box at=door_s,.3,on size=.4 mat=wood
	def mat_only
	  box at=door_s,.3,.004 size=.8,.5,.008 mat=wood
	building shed_{v} \"Shed\" kit=kk for v=blocker,mat_only
	  room 0,0 1,1 fill={v}
	  open 0,0 s door
	",
	);
	assert!(has(&p.build("shed_blocker", &no_glb()).unwrap().warnings, "stands in the doorway on its s wall"));
	assert!(p.build("shed_mat_only", &no_glb()).unwrap().warnings.is_empty());
}
