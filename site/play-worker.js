// The playground's builder: Pyodide (Python in WebAssembly) running partscript and kitlib, off the page's
// thread so typing never waits on a build. Messages in: {sources: [[name, text]...], prop}. Out: {ready},
// or {result, glb} where result says what was built (or what is wrong) and glb is the model's bytes.
importScripts("https://cdn.jsdelivr.net/pyodide/v0.26.4/full/pyodide.js");

const ready = (async () => {
	const pyodide = await loadPyodide({ indexURL: "https://cdn.jsdelivr.net/pyodide/v0.26.4/full/" });
	await pyodide.loadPackage("numpy");
	const zip = await (await fetch("py/partscript.zip")).arrayBuffer();
	pyodide.unpackArchive(zip, "zip", { extractDir: "/lib/partscript-src" });
	pyodide.runPython(`
import sys, json, time
sys.path.insert(0, "/lib/partscript-src")
import partscript as ps
from partscript.host import Host
HOST = Host()

LIBRARY = {}

def build(sources_json, wanted, library_json="{}"):
    sources = [tuple(s) for s in json.loads(sources_json)]
    LIBRARY.update(json.loads(library_json))
    started = time.perf_counter()
    try:
        project = ps.Project(sources, HOST, reader=lambda target: [(target, LIBRARY[target])] if target in LIBRARY else [])
    except Exception as error:
        return json.dumps({"errors": [str(error)], "props": []}), None
    edited = sources[-1][0]
    props = [p["id"] for p in project.props() if p["file"] == edited]
    errors = [str(e) for e in project.program.errors]
    if errors:
        return json.dumps({"errors": errors, "props": props}), None
    if not props:
        return json.dumps({"errors": ["nothing to show: start a prop with  prop NAME \\"Title\\""], "props": []}), None
    name = wanted if wanted in props else props[0]
    report = project.check()
    try:
        built = project.build(name)
    except ps.PartScriptError as error:
        return json.dumps({"errors": [str(error)], "props": props, "prop": name}), None
    return json.dumps({"errors": [e for e in report["errors"] if e.startswith(edited)], "props": props, "prop": name,
        "triangles": built.triangles, "ms": round((time.perf_counter() - started) * 1000),
        "warnings": [w for w in built.warnings + report["warnings"]][:12]}), built.glb
`);
	postMessage({ ready: true });
	return pyodide;
})();
ready.catch(error => postMessage({ failed: String(error) }));

onmessage = async event => {
	const pyodide = await ready;
	const build = pyodide.globals.get("build");
	const out = build(JSON.stringify(event.data.sources), event.data.prop || "", JSON.stringify(event.data.library || {}));
	const [result, glb] = out.toJs();
	out.destroy();
	const bytes = glb ? glb.slice() : null;
	postMessage({ result: JSON.parse(result), glb: bytes, id: event.data.id }, bytes ? [bytes.buffer] : []);
};
