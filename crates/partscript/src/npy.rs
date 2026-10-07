//! numpy's default random generator (PCG64 seeded through SeedSequence), for textures that must come
//! out pixel for pixel as they always have.
//!
//! Upstream notices: licenses/NumPy.txt, licenses/SeedSequence.txt and licenses/PCG64.txt
//! at the repository root. Adapted to Rust for PartScript's texture generation.

const INIT_A: u32 = 0x43b0d7e5;
const MULT_A: u32 = 0x931e8875;
const INIT_B: u32 = 0x8b51f9dd;
const MULT_B: u32 = 0x58f38ded;
const MIX_MULT_L: u32 = 0xca01f9dd;
const MIX_MULT_R: u32 = 0x4973f715;
const XSHIFT: u32 = 16;
const POOL: usize = 4;

/// numpy.random.SeedSequence(seed).generate_state(4, uint64)
fn seed_state(seed: u64) -> [u64; 4] {
	let mut entropy: Vec<u32> = Vec::new();
	let mut n = seed;
	loop {
		entropy.push(n as u32);
		n >>= 32;
		if n == 0 {
			break;
		}
	}
	let mut hash_const = INIT_A;
	let hashmix = |value: u32, hash_const: &mut u32| -> u32 {
		let mut value = value ^ *hash_const;
		*hash_const = hash_const.wrapping_mul(MULT_A);
		value = value.wrapping_mul(*hash_const);
		value ^ (value >> XSHIFT)
	};
	let mix = |x: u32, y: u32| -> u32 {
		let result = MIX_MULT_L.wrapping_mul(x).wrapping_sub(MIX_MULT_R.wrapping_mul(y));
		result ^ (result >> XSHIFT)
	};
	let mut pool = [0u32; POOL];
	for (i, slot) in pool.iter_mut().enumerate() {
		*slot = hashmix(entropy.get(i).copied().unwrap_or(0), &mut hash_const);
	}
	for i_src in 0..POOL {
		for i_dst in 0..POOL {
			if i_src != i_dst {
				let h = hashmix(pool[i_src], &mut hash_const);
				pool[i_dst] = mix(pool[i_dst], h);
			}
		}
	}
	for &value in entropy.iter().skip(POOL) {
		for slot in pool.iter_mut() {
			let h = hashmix(value, &mut hash_const);
			*slot = mix(*slot, h);
		}
	}
	let mut hash_const = INIT_B;
	let mut words = [0u32; 8];
	for (i_dst, word) in words.iter_mut().enumerate() {
		let mut data = pool[i_dst % POOL];
		data ^= hash_const;
		hash_const = hash_const.wrapping_mul(MULT_B);
		data = data.wrapping_mul(hash_const);
		data ^= data >> XSHIFT;
		*word = data;
	}
	[0, 1, 2, 3].map(|i| words[2 * i] as u64 | ((words[2 * i + 1] as u64) << 32))
}

const MULTIPLIER: u128 = (2549297995355413924u128 << 64) | 4865540595714422341u128;

/// numpy.random.default_rng(seed): PCG64 (XSL RR).
pub struct Generator {
	state: u128,
	inc: u128,
	has_uint32: bool,
	uinteger: u32,
}

impl Generator {
	pub fn new(seed: u64) -> Generator {
		let v = seed_state(seed);
		let initstate = ((v[0] as u128) << 64) | v[1] as u128;
		let initseq = ((v[2] as u128) << 64) | v[3] as u128;
		let mut g = Generator { state: 0, inc: (initseq << 1) | 1, has_uint32: false, uinteger: 0 };
		g.step();
		g.state = g.state.wrapping_add(initstate);
		g.step();
		g
	}

	fn step(&mut self) {
		self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
	}

	pub fn next_u64(&mut self) -> u64 {
		self.step();
		let s = self.state;
		let x = ((s >> 64) as u64) ^ (s as u64);
		x.rotate_right((s >> 122) as u32)
	}

	pub fn next_u32(&mut self) -> u32 {
		if self.has_uint32 {
			self.has_uint32 = false;
			return self.uinteger;
		}
		let next = self.next_u64();
		self.has_uint32 = true;
		self.uinteger = (next >> 32) as u32;
		next as u32
	}

	/// random(dtype=float64)
	pub fn random_f64(&mut self) -> f64 {
		(self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0)
	}

	/// random(dtype=float32)
	pub fn random_f32(&mut self) -> f32 {
		(self.next_u32() >> 8) as f32 * (1.0f32 / 16777216.0f32)
	}
}
