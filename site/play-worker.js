// The playground's builder: PartScript compiled to WebAssembly, off the page's thread so typing never
// waits on a build. Messages in: {sources: [[name, text]...], prop, library}. Out: {ready}, or
// {result, glb} where result says what was built (or what is wrong) and glb is the model's bytes.

const ready = (async () => {
	const { instance } = await WebAssembly.instantiateStreaming(fetch("pkg/partscript.wasm"), {});
	postMessage({ ready: true });
	return instance.exports;
})();
ready.catch(error => postMessage({ failed: String(error) }));

const encoder = new TextEncoder();
const decoder = new TextDecoder();

onmessage = async event => {
	const wasm = await ready;
	const started = performance.now();
	const input = encoder.encode(JSON.stringify({ sources: event.data.sources, prop: event.data.prop || "", library: event.data.library || {} }));
	const ptr = wasm.alloc(input.length);
	new Uint8Array(wasm.memory.buffer, ptr, input.length).set(input);
	wasm.build(ptr, input.length);
	wasm.dealloc(ptr, input.length);
	const out = new Uint8Array(wasm.memory.buffer, wasm.out_ptr(), wasm.out_len()).slice();
	const length = new DataView(out.buffer).getUint32(0, true);
	const result = JSON.parse(decoder.decode(out.subarray(4, 4 + length)));
	const glb = out.length > 4 + length ? out.slice(4 + length) : null;
	if (result.triangles !== undefined) result.ms = Math.round(performance.now() - started);
	postMessage({ result, glb, id: event.data.id }, glb ? [glb.buffer] : []);
};
