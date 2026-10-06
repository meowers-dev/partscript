"""PartScript: low-poly props in a few words, compiled to .glb."""

from .check import check
from .compiler import Compiler
from .host import Host
from .lang import PartScriptError, Program, evaluate, parse, tokenize
from .project import Built, Project, load_program
from .reference import REFERENCE
from .textures import BasicProvider, TextureProvider, TextureStore

__version__ = "0.1.0.dev0"

__all__ = ["REFERENCE", "BasicProvider", "Built", "Compiler", "Host", "PartScriptError", "Program", "Project", "TextureProvider",
	"TextureStore", "check", "evaluate", "load_program", "parse", "tokenize"]
