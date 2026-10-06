// The PSX look for the preview: models drawn unlit (texture times their baked vertex colours, the way the
// PlayStation drew them: the baked shading is the only light), corners snapped to a coarse
// screen grid (the wobble), nearest texels, and distance fog in the background colour.
import * as THREE from "three";

const SNAP = { value: new THREE.Vector2(160, 120) };

function snapped(material) {
	material.onBeforeCompile = shader => {
		shader.uniforms.uSnap = SNAP;
		shader.vertexShader = "uniform vec2 uSnap;\n" + shader.vertexShader.replace("#include <project_vertex>",
			"#include <project_vertex>\n\tgl_Position.xy = floor(gl_Position.xy / gl_Position.w * uSnap + 0.5) / uSnap * gl_Position.w;");
	};
	material.customProgramCacheKey = () => "psx-snap";
	return material;
}

// Every mesh under root drawn the PSX way (on) or as loaded (off). The loaded materials are kept.
export function psx(root, on) {
	root.traverse(o => {
		if (!o.isMesh || o.userData.overlay) return;
		const own = o.userData.loaded || (o.userData.loaded = o.material);
		if (!on) { o.material = own; return; }
		o.material = o.userData.psx || (o.userData.psx = [own].flat().map(m => {
			const basic = new THREE.MeshBasicMaterial({ map: m.map || m.emissiveMap, vertexColors: true, transparent: m.transparent,
				opacity: m.opacity, alphaTest: m.alphaTest, side: m.side, wireframe: m.wireframe, fog: true });
			if (m.emissiveMap || (m.emissive && m.emissive.getHex() > 0)) {
				// lamps and signs: full bright
				basic.vertexColors = false;
				if (m.emissiveMap) basic.map = m.emissiveMap;
				else { basic.map = null; basic.color.copy(m.emissive); }
			}
			return snapped(basic);
		}));
		if (Array.isArray(own) === false && Array.isArray(o.material)) o.material = o.material[0];
	});
}

// Fog in the background colour from near to far (metres from the camera), or none.
export function fog(scene, near, far) {
	scene.fog = near === null ? null : new THREE.Fog(scene.background, near, far);
}

// How coarse the snapping grid is: w x h steps across the view (the PlayStation drew at 320x240).
export function snapGrid(w, h) { SNAP.value.set(w, h); }
