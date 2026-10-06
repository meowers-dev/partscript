"""kitlib: low-poly geometry, baking and a .glb writer. Pure Python; PartScript compiles to it."""

from .bake import bake
from .geom import Atlas, Face, Mat, Part, geometry_stats
from .gltf import glb_bytes, write_glb

__all__ = ["Atlas", "Face", "Mat", "Part", "bake", "geometry_stats", "glb_bytes", "write_glb"]
