//! A map that keeps the order keys were first added in (as a Python dict does).

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Ordered<V> {
	items: Vec<(String, V)>,
	index: HashMap<String, usize>,
}

impl<V> Default for Ordered<V> {
	fn default() -> Self {
		Ordered::new()
	}
}

impl<V> Ordered<V> {
	pub fn new() -> Self {
		Ordered { items: Vec::new(), index: HashMap::new() }
	}

	pub fn get(&self, key: &str) -> Option<&V> {
		self.index.get(key).map(|&i| &self.items[i].1)
	}

	pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
		self.index.get(key).map(|&i| &mut self.items[i].1)
	}

	pub fn contains(&self, key: &str) -> bool {
		self.index.contains_key(key)
	}

	/// Sets a key: replaced where it is, else added at the end.
	pub fn insert(&mut self, key: &str, value: V) {
		if let Some(&i) = self.index.get(key) {
			self.items[i].1 = value;
		} else {
			self.index.insert(key.to_string(), self.items.len());
			self.items.push((key.to_string(), value));
		}
	}

	pub fn remove(&mut self, key: &str) -> Option<V> {
		let i = self.index.remove(key)?;
		let (_, value) = self.items.remove(i);
		for (k, slot) in self.index.iter_mut() {
			let _ = k;
			if *slot > i {
				*slot -= 1;
			}
		}
		Some(value)
	}

	pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
		self.items.iter().map(|(k, v)| (k.as_str(), v))
	}

	pub fn keys(&self) -> impl Iterator<Item = &str> {
		self.items.iter().map(|(k, _)| k.as_str())
	}

	pub fn values(&self) -> impl Iterator<Item = &V> {
		self.items.iter().map(|(_, v)| v)
	}

	pub fn len(&self) -> usize {
		self.items.len()
	}

	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}
}

impl<V: Clone> Ordered<V> {
	pub fn entry_or(&mut self, key: &str, default: V) -> &mut V {
		if !self.index.contains_key(key) {
			self.insert(key, default);
		}
		self.get_mut(key).unwrap()
	}
}

impl<V> FromIterator<(String, V)> for Ordered<V> {
	fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
		let mut out = Ordered::new();
		for (k, v) in iter {
			out.insert(&k, v);
		}
		out
	}
}
