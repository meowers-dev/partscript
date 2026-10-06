//! Python's number behaviour, where PartScript's results depend on it: the random module's Mersenne
//! Twister (seeded from strings and integers as Python seeds it), float repr and `:g` formatting,
//! round() to n digits, `%` and `//`, the compensated float sum() and math.hypot / math.dist.
//!
//! PartScript began in Python; these keep every random deal and every rounded key the same, so a
//! file builds the same model it always did.

use crate::hash::sha512;

// ------------------------------------------------------------------ random.Random
const N: usize = 624;
const M: usize = 397;

/// random.Random: MT19937 with Python's seeding, random(), uniform(), choice, shuffle and sample.
#[derive(Clone)]
pub struct PyRandom {
	mt: [u32; N],
	index: usize,
}

impl PyRandom {
	/// random.Random(text): the string's UTF-8 bytes and their SHA-512, read as one big-endian integer.
	pub fn from_str(seed: &str) -> Self {
		let mut bytes = seed.as_bytes().to_vec();
		bytes.extend_from_slice(&sha512(seed.as_bytes()));
		Self::from_be_bytes(&bytes)
	}

	/// random.Random(n) for a whole number n (its absolute value is the seed).
	pub fn from_int(seed: i128) -> Self {
		let n = seed.unsigned_abs();
		Self::from_be_bytes(&n.to_be_bytes())
	}

	fn from_be_bytes(bytes: &[u8]) -> Self {
		let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
		let bytes = &bytes[start..];
		// 32-bit words, least significant first.
		let mut key = Vec::with_capacity(bytes.len() / 4 + 1);
		let mut end = bytes.len();
		while end > 0 {
			let begin = end.saturating_sub(4);
			let mut word = 0u32;
			for &b in &bytes[begin..end] {
				word = (word << 8) | b as u32;
			}
			key.push(word);
			end = begin;
		}
		if key.is_empty() {
			key.push(0);
		}
		let mut rng = PyRandom { mt: [0; N], index: N };
		rng.init_by_array(&key);
		rng
	}

	fn init_genrand(&mut self, seed: u32) {
		self.mt[0] = seed;
		for i in 1..N {
			let prev = self.mt[i - 1];
			self.mt[i] = 1812433253u32.wrapping_mul(prev ^ (prev >> 30)).wrapping_add(i as u32);
		}
		self.index = N;
	}

	fn init_by_array(&mut self, key: &[u32]) {
		self.init_genrand(19650218);
		let (mut i, mut j) = (1usize, 0usize);
		let mut k = N.max(key.len());
		while k > 0 {
			let prev = self.mt[i - 1];
			self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1664525)).wrapping_add(key[j]).wrapping_add(j as u32);
			i += 1;
			j += 1;
			if i >= N {
				self.mt[0] = self.mt[N - 1];
				i = 1;
			}
			if j >= key.len() {
				j = 0;
			}
			k -= 1;
		}
		k = N - 1;
		while k > 0 {
			let prev = self.mt[i - 1];
			self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1566083941)).wrapping_sub(i as u32);
			i += 1;
			if i >= N {
				self.mt[0] = self.mt[N - 1];
				i = 1;
			}
			k -= 1;
		}
		self.mt[0] = 0x8000_0000;
		self.index = N;
	}

	fn next_u32(&mut self) -> u32 {
		if self.index >= N {
			for kk in 0..N {
				let y = (self.mt[kk] & 0x8000_0000) | (self.mt[(kk + 1) % N] & 0x7fff_ffff);
				let mut next = self.mt[(kk + M) % N] ^ (y >> 1);
				if y & 1 != 0 {
					next ^= 0x9908_b0df;
				}
				self.mt[kk] = next;
			}
			self.index = 0;
		}
		let mut y = self.mt[self.index];
		self.index += 1;
		y ^= y >> 11;
		y ^= (y << 7) & 0x9d2c_5680;
		y ^= (y << 15) & 0xefc6_0000;
		y ^= y >> 18;
		y
	}

	/// random(): a float in [0, 1) from 53 random bits.
	pub fn random(&mut self) -> f64 {
		let a = (self.next_u32() >> 5) as f64;
		let b = (self.next_u32() >> 6) as f64;
		(a * 67108864.0 + b) * (1.0 / 9007199254740992.0)
	}

	/// uniform(a, b) = a + (b - a) * random()
	pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
		a + (b - a) * self.random()
	}

	/// getrandbits(k) for k up to 32.
	pub fn getrandbits(&mut self, k: u32) -> u32 {
		if k == 0 {
			return 0;
		}
		self.next_u32() >> (32 - k)
	}

	/// _randbelow(n): uniform in [0, n), by rejection on getrandbits(n.bit_length()).
	pub fn randbelow(&mut self, n: usize) -> usize {
		let k = usize::BITS - n.leading_zeros();
		let mut r = self.getrandbits(k) as usize;
		while r >= n {
			r = self.getrandbits(k) as usize;
		}
		r
	}

	pub fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
		&items[self.randbelow(items.len())]
	}

	pub fn shuffle<T>(&mut self, items: &mut [T]) {
		for i in (1..items.len()).rev() {
			let j = self.randbelow(i + 1);
			items.swap(i, j);
		}
	}

	/// sample(population, k): k distinct items, by Python's two methods (a pool, or a set of picks).
	pub fn sample<T: Clone>(&mut self, population: &[T], k: usize) -> Vec<T> {
		let n = population.len();
		assert!(k <= n, "sample larger than population");
		let mut setsize = 21usize;
		if k > 5 {
			setsize += 4usize.pow(((k as f64 * 3.0).ln() / 4f64.ln()).ceil() as u32);
		}
		let mut out = Vec::with_capacity(k);
		if n <= setsize {
			let mut pool: Vec<T> = population.to_vec();
			for i in 0..k {
				let j = self.randbelow(n - i);
				out.push(pool[j].clone());
				pool[j] = pool[n - i - 1].clone();
			}
		} else {
			let mut selected = std::collections::HashSet::new();
			for _ in 0..k {
				let mut j = self.randbelow(n);
				while selected.contains(&j) {
					j = self.randbelow(n);
				}
				selected.insert(j);
				out.push(population[j].clone());
			}
		}
		out
	}
}

// ------------------------------------------------------------------ rounding and arithmetic
/// round(x): to the nearest whole number, halves to even.
pub fn round(x: f64) -> f64 {
	x.round_ties_even()
}

/// round(x, digits): correctly rounded to that many decimals, halves (of the exact value) to even.
pub fn round_to(x: f64, digits: usize) -> f64 {
	if !x.is_finite() {
		return x;
	}
	let text = format!("{x:.digits$}");
	let out: f64 = text.parse().unwrap_or(x);
	if out == 0.0 && x.is_sign_negative() {
		-0.0
	} else {
		out
	}
}

/// x % y with Python's sign rule (the result takes the divisor's sign). None for y = 0.
pub fn modulo(x: f64, y: f64) -> Option<f64> {
	if y == 0.0 {
		return None;
	}
	let mut m = x % y;
	if m != 0.0 {
		if (y < 0.0) != (m < 0.0) {
			m += y;
		}
	} else {
		m = 0.0f64.copysign(y);
	}
	Some(m)
}

/// x // y as Python works it out (floor of the true quotient, snapped). None for y = 0.
pub fn floordiv(x: f64, y: f64) -> Option<f64> {
	if y == 0.0 {
		return None;
	}
	let mut m = x % y;
	let mut div = (x - m) / y;
	if m != 0.0 {
		if (y < 0.0) != (m < 0.0) {
			m += y;
			div -= 1.0;
		}
	}
	let _ = m;
	if div != 0.0 {
		let mut floor = div.floor();
		if div - floor > 0.5 {
			floor += 1.0;
		}
		Some(floor)
	} else {
		Some(0.0f64.copysign(x / y))
	}
}

/// sum() of floats as Python 3.12+ adds them: Neumaier's compensated summation.
pub fn sum<I: IntoIterator<Item = f64>>(items: I) -> f64 {
	let mut total = 0.0f64;
	let mut c = 0.0f64;
	let mut first = true;
	for x in items {
		if first {
			// int 0 + the first float
			total = 0.0 + x;
			first = false;
			continue;
		}
		let t = total + x;
		if total.abs() >= x.abs() {
			c += (total - t) + x;
		} else {
			c += (x - t) + total;
		}
		total = t;
	}
	if c != 0.0 && c.is_finite() {
		total += c;
	}
	total
}

// ------------------------------------------------------------------ hypot and dist
fn frexp_exponent(x: f64) -> i32 {
	// The e of frexp: x = m * 2**e with 0.5 <= m < 1 (x normal and positive here).
	let bits = x.to_bits();
	let exp = ((bits >> 52) & 0x7ff) as i32;
	if exp == 0 {
		// subnormal
		let m = x * f64::from_bits(0x4350_0000_0000_0000); // 2**54
		return frexp_exponent(m) - 54;
	}
	exp - 1022
}

fn ldexp_one(e: i32) -> f64 {
	// 2**e for the range vector_norm uses.
	if (-1022..=1023).contains(&e) {
		f64::from_bits(((e + 1023) as u64) << 52)
	} else {
		2f64.powi(e)
	}
}

fn dl_mul(x: f64, y: f64) -> (f64, f64) {
	let z = x * y;
	(z, x.mul_add(y, -z))
}

fn dl_fast_sum(a: f64, b: f64) -> (f64, f64) {
	let x = a + b;
	let z = x - a;
	(x, b - z)
}

/// CPython's vector_norm: the length of a vector of absolute values, max the largest of them.
fn vector_norm(vec: &mut [f64], max: f64, found_nan: bool) -> f64 {
	if max.is_infinite() {
		return max;
	}
	if found_nan {
		return f64::NAN;
	}
	if max == 0.0 || vec.len() <= 1 {
		return max;
	}
	let max_e = frexp_exponent(max);
	if max_e < -1023 {
		for v in vec.iter_mut() {
			*v /= f64::MIN_POSITIVE;
		}
		return f64::MIN_POSITIVE * vector_norm(vec, max / f64::MIN_POSITIVE, found_nan);
	}
	let scale = ldexp_one(-max_e);
	let (mut csum, mut frac1, mut frac2) = (1.0f64, 0.0f64, 0.0f64);
	for &v in vec.iter() {
		let x = v * scale;
		let pr = dl_mul(x, x);
		let sm = dl_fast_sum(csum, pr.0);
		csum = sm.0;
		frac1 += pr.1;
		frac2 += sm.1;
	}
	let mut h = (csum - 1.0 + (frac1 + frac2)).sqrt();
	let pr = dl_mul(-h, h);
	let sm = dl_fast_sum(csum, pr.0);
	csum = sm.0;
	frac1 += pr.1;
	frac2 += sm.1;
	let x = csum - 1.0 + (frac1 + frac2);
	h += x / (2.0 * h);
	h / scale
}

/// math.hypot(*coordinates)
pub fn hypot(coordinates: &[f64]) -> f64 {
	let mut vec: Vec<f64> = coordinates.iter().map(|v| v.abs()).collect();
	let mut max = 0.0f64;
	let mut found_nan = false;
	for &v in &vec {
		found_nan |= v.is_nan();
		if v > max {
			max = v;
		}
	}
	vector_norm(&mut vec, max, found_nan)
}

pub fn hypot2(x: f64, y: f64) -> f64 {
	hypot(&[x, y])
}

/// math.dist(p, q)
pub fn dist(p: &[f64], q: &[f64]) -> f64 {
	let diffs: Vec<f64> = p.iter().zip(q).map(|(a, b)| a - b).collect();
	hypot(&diffs)
}

// ------------------------------------------------------------------ formatting
/// (digits, exponent) of a finite non-zero float's shortest repr: value = d.ddd x 10**exponent.
fn shortest(x: f64) -> (String, i32) {
	let text = format!("{:e}", x.abs());
	let (mantissa, exponent) = text.split_once('e').unwrap();
	(mantissa.replace('.', ""), exponent.parse().unwrap())
}

/// repr(x) for a float.
pub fn repr(x: f64) -> String {
	if x.is_nan() {
		return "nan".into();
	}
	if x.is_infinite() {
		return if x > 0.0 { "inf".into() } else { "-inf".into() };
	}
	let sign = if x.is_sign_negative() { "-" } else { "" };
	if x == 0.0 {
		return format!("{sign}0.0");
	}
	let (digits, exp) = shortest(x);
	let body = if (-4..16).contains(&exp) { fixed(&digits, exp, true) } else { exponential(&digits, exp) };
	format!("{sign}{body}")
}

/// digits placed with the decimal point after exp + 1 of them (keep_point: always show .0).
fn fixed(digits: &str, exp: i32, keep_point: bool) -> String {
	let n = digits.len() as i32;
	if exp < 0 {
		return format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits);
	}
	if exp + 1 >= n {
		let whole = format!("{}{}", digits, "0".repeat((exp + 1 - n) as usize));
		return if keep_point { format!("{whole}.0") } else { whole };
	}
	let (a, b) = digits.split_at((exp + 1) as usize);
	format!("{a}.{b}")
}

fn exponential(digits: &str, exp: i32) -> String {
	let mantissa = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.to_string() };
	let sign = if exp < 0 { "-" } else { "+" };
	format!("{mantissa}e{sign}{:02}", exp.abs())
}

/// format(x, ".{precision}g") / "%.{precision}g": significant digits, trailing zeros dropped.
pub fn format_g(x: f64, precision: usize) -> String {
	if x.is_nan() {
		return "nan".into();
	}
	if x.is_infinite() {
		return if x > 0.0 { "inf".into() } else { "-inf".into() };
	}
	let precision = precision.max(1);
	let sign = if x.is_sign_negative() { "-" } else { "" };
	if x == 0.0 {
		return format!("{sign}0");
	}
	let text = format!("{:.*e}", precision - 1, x.abs());
	let (mantissa, exponent) = text.split_once('e').unwrap();
	let exp: i32 = exponent.parse().unwrap();
	let digits = mantissa.replace('.', "");
	let digits = digits.trim_end_matches('0');
	let digits = if digits.is_empty() { "0" } else { digits };
	let body = if exp < -4 || exp >= precision as i32 { exponential(digits, exp) } else { fixed(digits, exp, false) };
	format!("{sign}{body}")
}

/// f"{x:g}"
pub fn g(x: f64) -> String {
	format_g(x, 6)
}

/// str(x) of a whole number held as a float (Python ints print without a point).
pub fn int_str(x: f64) -> String {
	format!("{}", x as i64)
}

/// str.title(): each run of letters starts upper case, the rest lower.
pub fn title(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut previous_cased = false;
	for ch in text.chars() {
		let cased = ch.is_lowercase() || ch.is_uppercase();
		if cased {
			if previous_cased {
				out.extend(ch.to_lowercase());
			} else {
				out.extend(ch.to_uppercase());
			}
		} else {
			out.push(ch);
		}
		previous_cased = cased;
	}
	out
}

/// repr of a str, as Python writes it in a tuple or a message ('text', or "text" when it holds a ').
pub fn repr_str(text: &str) -> String {
	let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
	let mut out = String::new();
	out.push(quote);
	for ch in text.chars() {
		match ch {
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if c == quote => {
				out.push('\\');
				out.push(c);
			}
			c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
			c => out.push(c),
		}
	}
	out.push(quote);
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_like_python() {
		assert_eq!(repr(1e-5), "1e-05");
		assert_eq!(repr(1e16), "1e+16");
		assert_eq!(repr(0.1 + 0.2), "0.30000000000000004");
		assert_eq!(repr(1.0), "1.0");
		assert_eq!(repr(-0.0), "-0.0");
		assert_eq!(repr(123456789012345678.0), "1.2345678901234568e+17");
		assert_eq!(g(0.5), "0.5");
		assert_eq!(g(1e-5), "1e-05");
		assert_eq!(g(1234567.0), "1.23457e+06");
		assert_eq!(g(100.0), "100");
		assert_eq!(round_to(2.675, 2), 2.67);
		assert_eq!(title("writers_desk 2b".replace('_', " ").as_str()), "Writers Desk 2B");
	}

	#[test]
	fn random_matches_python() {
		// random.Random("a").random() and random.Random(0).random()
		assert_eq!(PyRandom::from_str("a").random(), 0.2720295377534757);
		assert_eq!(PyRandom::from_int(0).random(), 0.8444218515250481);
	}
}
