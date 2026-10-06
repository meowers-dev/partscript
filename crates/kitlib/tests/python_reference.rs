//! kitlib's Python-compatible pieces against what Python itself gave (tests/reference/basics.json.gz).

use kitlib::json::Json;
use kitlib::py::{self, PyRandom};

fn basics() -> Json {
	let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/reference/basics.json.gz");
	let text = String::from_utf8(kitlib::gunzip(&std::fs::read(path).unwrap()).unwrap()).unwrap();
	Json::parse(&text).unwrap()
}

fn seeded(seed: &Json) -> PyRandom {
	match seed {
		Json::Str(s) => PyRandom::from_str(s),
		Json::Int(n) => PyRandom::from_int(*n as i128),
		_ => panic!(),
	}
}

#[test]
fn random_streams() {
	let data = basics();
	for row in data.get("random").unwrap().as_list() {
		let mut rng = seeded(row.get("seed").unwrap());
		for v in row.get("random").unwrap().as_list() {
			assert_eq!(rng.random(), v.as_f64(), "{:?}", row.get("seed"));
		}
		for v in row.get("uniform").unwrap().as_list() {
			assert_eq!(rng.uniform(-2.5, 7.25), v.as_f64());
		}
		for v in row.get("choice").unwrap().as_list() {
			assert_eq!(*rng.choice(&[-1i64, 1]), v.as_i64());
		}
		let seven: Vec<i64> = (0..7).collect();
		for v in row.get("choice7").unwrap().as_list() {
			assert_eq!(*rng.choice(&seven), v.as_i64());
		}
		let mut values: Vec<i64> = (0..256).collect();
		rng.shuffle(&mut values);
		let want: Vec<i64> = row.get("shuffle").unwrap().as_list().iter().map(Json::as_i64).collect();
		assert_eq!(values, want);
		let sizes = [(10usize, 3usize), (30, 25), (100, 7), (400, 120), (5, 5)];
		for ((n, k), want) in sizes.iter().zip(row.get("sample").unwrap().as_list()) {
			let population: Vec<i64> = (0..*n as i64).collect();
			let got = rng.sample(&population, *k);
			let want: Vec<i64> = want.as_list().iter().map(Json::as_i64).collect();
			assert_eq!(got, want, "sample {n} {k}");
		}
	}
}

#[test]
fn float_formatting_and_rounding() {
	let data = basics();
	for row in data.get("floats").unwrap().as_list() {
		let x = row.get("x").unwrap().as_f64();
		assert_eq!(py::repr(x), row.get("repr").unwrap().as_str(), "repr");
		assert_eq!(py::g(x), row.get("g").unwrap().as_str(), "g {x}");
		assert_eq!(py::format_g(x, 6), row.get("g6").unwrap().as_str());
		assert_eq!(format!("{x:.2}"), row.get("f2").unwrap().as_str());
		assert_eq!(py::round(x), row.get("r0").unwrap().as_f64(), "round {x}");
		for (n, key) in [(4, "r4"), (5, "r5"), (6, "r6"), (12, "r12")] {
			let want = row.get(key).unwrap().as_f64();
			let got = py::round_to(x, n);
			assert!(got == want && got.is_sign_negative() == want.is_sign_negative() || got == want, "round({x}, {n}) {got} {want}");
		}
		assert_eq!(py::modulo(x, 3.0).unwrap(), row.get("mod3").unwrap().as_f64(), "{x} % 3");
		assert_eq!(py::modulo(x, -2.5).unwrap(), row.get("modn").unwrap().as_f64());
		assert_eq!(py::floordiv(x, 0.7).unwrap(), row.get("fdiv").unwrap().as_f64(), "{x} // .7");
		let tuple = format!("({}, {}, 0.0)", py::repr(py::round_to(x, 4)), py::repr(py::round_to(-x, 4)));
		assert_eq!(tuple, row.get("tuple").unwrap().as_str());
	}
}

#[test]
fn hypot_dist_and_sum() {
	let data = basics();
	for row in data.get("hypot").unwrap().as_list() {
		let (a, b) = (row.idx(0).as_f64(), row.idx(1).as_f64());
		assert_eq!(py::hypot2(a, b), row.idx(2).as_f64());
		assert_eq!(py::dist(&[a, b, 1.5], &[b, -a, 0.25]), row.idx(3).as_f64());
	}
	for row in data.get("sum").unwrap().as_list() {
		let xs: Vec<f64> = row.idx(0).as_list().iter().map(Json::as_f64).collect();
		assert_eq!(py::sum(xs.iter().copied()), row.idx(1).as_f64());
	}
}
