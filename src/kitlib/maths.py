"""Vectors, 3x3/4x4 matrices and XYZ eulers in plain Python (64-bit floats).

The subset of Blender's mathutils the geometry needs, with its conventions: column vectors,
``matrix @ vector`` (a 3-vector through a 4x4 matrix is a point), right-handed rotations, and
Euler "XYZ" = Rz @ Ry @ Rx. The hot paths (4x4 products, points through 4x4 matrices) are
written out by hand: they run for every corner of every face.
"""

from __future__ import annotations

import math
import struct

_F32 = struct.Struct("<f")


def f32(value: float) -> float:
	"""The nearest 32-bit float, as a Python float (what a glTF float32 accessor stores)."""
	return _F32.unpack(_F32.pack(value))[0]


def _vector(values: list) -> "Vector":
	out = Vector.__new__(Vector)
	out._v = values
	return out


class Vector:
	__slots__ = ("_v",)

	def __init__(self, values=(0.0, 0.0, 0.0)):
		self._v = [float(v) for v in values]

	# ------------------------------------------------------------ access
	def __len__(self) -> int:
		return len(self._v)

	def __iter__(self):
		return iter(self._v)

	def __getitem__(self, index):
		return self._v[index]

	def __setitem__(self, index, value) -> None:
		self._v[index] = float(value)

	def __repr__(self) -> str:
		return "Vector((%s))" % ", ".join(f"{v:.6g}" for v in self._v)

	def __eq__(self, other) -> bool:
		try:
			return self._v == [float(v) for v in other]
		except TypeError:
			return NotImplemented

	def __hash__(self) -> int:
		return hash(tuple(self._v))

	x = property(lambda self: self._v[0], lambda self, value: self.__setitem__(0, value))
	y = property(lambda self: self._v[1], lambda self, value: self.__setitem__(1, value))
	z = property(lambda self: self._v[2], lambda self, value: self.__setitem__(2, value))
	w = property(lambda self: self._v[3], lambda self, value: self.__setitem__(3, value))

	def copy(self) -> "Vector":
		return _vector(list(self._v))

	# ------------------------------------------------------------ arithmetic
	def __add__(self, other) -> "Vector":
		b = other._v if isinstance(other, Vector) else list(other)
		if len(b) != len(self._v):
			raise ValueError("Vector + Vector: sizes differ")
		return _vector([x + y for x, y in zip(self._v, b)])

	__radd__ = __add__

	def __sub__(self, other) -> "Vector":
		b = other._v if isinstance(other, Vector) else list(other)
		if len(b) != len(self._v):
			raise ValueError("Vector - Vector: sizes differ")
		return _vector([x - y for x, y in zip(self._v, b)])

	def __rsub__(self, other) -> "Vector":
		return _vector([y - x for x, y in zip(self._v, list(other), strict=True)])

	def __neg__(self) -> "Vector":
		return _vector([-a for a in self._v])

	def __mul__(self, other):
		if isinstance(other, Vector):
			return _vector([a * b for a, b in zip(self._v, other._v, strict=True)])
		scalar = float(other)
		return _vector([a * scalar for a in self._v])

	__rmul__ = __mul__

	def __truediv__(self, scalar: float) -> "Vector":
		inverse = 1.0 / float(scalar)
		return _vector([a * inverse for a in self._v])

	def dot(self, other) -> float:
		b = other._v if isinstance(other, Vector) else list(other)
		return sum(x * y for x, y in zip(self._v, b, strict=True))

	def cross(self, other) -> "Vector":
		ax, ay, az = self._v
		bx, by, bz = other._v if isinstance(other, Vector) else other
		return _vector([ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx])

	@property
	def length(self) -> float:
		return math.sqrt(self.length_squared)

	@property
	def length_squared(self) -> float:
		return sum(a * a for a in self._v)

	def normalize(self) -> None:
		squared = self.length_squared
		# Like mathutils, a zero vector stays zero rather than dividing by it.
		if squared > 1.0e-35:
			inverse = 1.0 / math.sqrt(squared)
			self._v = [a * inverse for a in self._v]
		else:
			self._v = [0.0] * len(self._v)

	def normalized(self) -> "Vector":
		out = self.copy()
		out.normalize()
		return out


def _matrix(rows: list) -> "Matrix":
	out = Matrix.__new__(Matrix)
	out._m = rows
	return out


class Matrix:
	"""A square matrix (3x3 or 4x4), rows of floats."""

	__slots__ = ("_m",)

	def __init__(self, rows=None):
		self._m = [[float(v) for v in row] for row in rows] if rows is not None else Matrix.Identity(4)._m

	# ------------------------------------------------------------ constructors
	@staticmethod
	def Identity(size: int) -> "Matrix":
		return _matrix([[1.0 if r == c else 0.0 for c in range(size)] for r in range(size)])

	@staticmethod
	def Translation(vector) -> "Matrix":
		x, y, z = (float(v) for v in list(vector)[:3])
		return _matrix([[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z], [0.0, 0.0, 0.0, 1.0]])

	@staticmethod
	def Diagonal(vector) -> "Matrix":
		values = [float(v) for v in vector]
		return _matrix([[values[r] if r == c else 0.0 for c in range(len(values))] for r in range(len(values))])

	@staticmethod
	def Rotation(angle: float, size: int, axis) -> "Matrix":
		c, s = math.cos(angle), math.sin(angle)
		if isinstance(axis, str):
			rows = {
				"X": [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
				"Y": [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
				"Z": [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
			}[axis.upper()]
		else:
			x, y, z = Vector(axis).normalized()
			ico = 1.0 - c
			sx, sy, sz = x * s, y * s, z * s
			n00, n01, n11 = x * x * ico, x * y * ico, y * y * ico
			n02, n12, n22 = x * z * ico, y * z * ico, z * z * ico
			rows = [
				[n00 + c, n01 - sz, n02 + sy],
				[n01 + sz, n11 + c, n12 - sx],
				[n02 - sy, n12 + sx, n22 + c],
			]
		out = _matrix(rows)
		return out.to_4x4() if size == 4 else out

	# ------------------------------------------------------------ access
	def __len__(self) -> int:
		return len(self._m)

	def __getitem__(self, row: int) -> list:
		return self._m[row]

	def __repr__(self) -> str:
		return "Matrix(%r)" % (self._m,)

	def copy(self) -> "Matrix":
		return _matrix([list(row) for row in self._m])

	def to_4x4(self) -> "Matrix":
		if len(self._m) == 4:
			return self.copy()
		(a, b, c), (d, e, f), (g, h, i) = self._m
		return _matrix([[a, b, c, 0.0], [d, e, f, 0.0], [g, h, i, 0.0], [0.0, 0.0, 0.0, 1.0]])

	def to_3x3(self) -> "Matrix":
		return _matrix([row[:3] for row in self._m[:3]])

	def transposed(self) -> "Matrix":
		return _matrix([list(row) for row in zip(*self._m)])

	def inverted(self) -> "Matrix":
		"""The inverse (Gauss-Jordan with partial pivoting); raises ValueError when there is none."""
		size = len(self._m)
		rows = [list(row) + [1.0 if r == c else 0.0 for c in range(size)] for r, row in enumerate(self._m)]
		for col in range(size):
			pivot = max(range(col, size), key=lambda r: abs(rows[r][col]))
			if abs(rows[pivot][col]) < 1e-12:
				raise ValueError("matrix has no inverse")
			rows[col], rows[pivot] = rows[pivot], rows[col]
			scale = rows[col][col]
			rows[col] = [v / scale for v in rows[col]]
			for r in range(size):
				if r != col and rows[r][col]:
					factor = rows[r][col]
					rows[r] = [a - factor * b for a, b in zip(rows[r], rows[col])]
		return _matrix([row[size:] for row in rows])

	def to_translation(self) -> Vector:
		return _vector([self._m[0][3], self._m[1][3], self._m[2][3]])

	def determinant(self) -> float:
		m = self._m
		if len(m) == 4 and m[3] == [0.0, 0.0, 0.0, 1.0]:
			m = [row[:3] for row in m[:3]]
		if len(m) == 3:
			(a, b, c), (d, e, f), (g, h, i) = m
			return a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
		return _determinant(m)

	# ------------------------------------------------------------ products
	def __matmul__(self, other):
		m = self._m
		size = len(m)
		if isinstance(other, Matrix):
			o = other._m
			if len(o) != size:
				raise ValueError("Matrix @ Matrix: sizes differ")
			if size == 4:
				(b00, b01, b02, b03), (b10, b11, b12, b13), (b20, b21, b22, b23), (b30, b31, b32, b33) = o
				return _matrix([[
					a0 * b00 + a1 * b10 + a2 * b20 + a3 * b30, a0 * b01 + a1 * b11 + a2 * b21 + a3 * b31,
					a0 * b02 + a1 * b12 + a2 * b22 + a3 * b32, a0 * b03 + a1 * b13 + a2 * b23 + a3 * b33,
				] for a0, a1, a2, a3 in m])
			columns = list(zip(*o))
			return _matrix([[sum(a * b for a, b in zip(row, column)) for column in columns] for row in m])
		values = other._v if isinstance(other, Vector) else [float(v) for v in other]
		if size == 4 and len(values) == 3:
			# A point: transformed as (x, y, z, 1), like mathutils.
			x, y, z = values
			r0, r1, r2 = m[0], m[1], m[2]
			return _vector([r0[0] * x + r0[1] * y + r0[2] * z + r0[3], r1[0] * x + r1[1] * y + r1[2] * z + r1[3],
				r2[0] * x + r2[1] * y + r2[2] * z + r2[3]])
		if len(values) == size:
			return _vector([sum(a * b for a, b in zip(row, values)) for row in m])
		raise ValueError("Matrix @ vector: sizes differ")


def _determinant(rows: list) -> float:
	if len(rows) == 1:
		return rows[0][0]
	if len(rows) == 2:
		return rows[0][0] * rows[1][1] - rows[0][1] * rows[1][0]
	total = 0.0
	for column, value in enumerate(rows[0]):
		if value == 0.0:
			continue
		minor = [row[:column] + row[column + 1:] for row in rows[1:]]
		total += (-1.0 if column % 2 else 1.0) * value * _determinant(minor)
	return total


class Euler:
	__slots__ = ("_v", "order")

	def __init__(self, angles=(0.0, 0.0, 0.0), order: str = "XYZ"):
		if order != "XYZ":
			raise ValueError("Euler: only the XYZ order is implemented")
		self._v = [float(v) for v in angles]
		self.order = order

	def __iter__(self):
		return iter(self._v)

	x = property(lambda self: self._v[0])
	y = property(lambda self: self._v[1])
	z = property(lambda self: self._v[2])

	def to_matrix(self) -> Matrix:
		ci, cj, ch = (math.cos(v) for v in self._v)
		si, sj, sh = (math.sin(v) for v in self._v)
		cc, cs, sc, ss = ci * ch, ci * sh, si * ch, si * sh
		return _matrix([
			[cj * ch, sj * sc - cs, sj * cc + ss],
			[cj * sh, sj * ss + cc, sj * cs - sc],
			[-sj, cj * si, cj * ci],
		])
