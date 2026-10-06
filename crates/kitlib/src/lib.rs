//! kitlib: low-poly geometry, baking and a .glb writer. PartScript compiles to it.

pub mod hash;
pub mod py;
pub mod json;

/// The bytes of a gzip file (one member, as Python's gzip.compress writes it).
pub fn gunzip(data: &[u8]) -> Result<Vec<u8>, String> {
	if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
		return Err("not gzip".into());
	}
	let flags = data[3];
	let mut pos = 10;
	if flags & 4 != 0 {
		let extra = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
		pos += 2 + extra;
	}
	for bit in [8u8, 16] {
		if flags & bit != 0 {
			while data[pos] != 0 {
				pos += 1;
			}
			pos += 1;
		}
	}
	if flags & 2 != 0 {
		pos += 2;
	}
	miniz_oxide::inflate::decompress_to_vec(&data[pos..data.len() - 8]).map_err(|e| format!("{e:?}"))
}
