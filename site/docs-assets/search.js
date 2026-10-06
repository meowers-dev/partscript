// Search the docs: every section of every page, matched word by word, best first.
(async () => {
	const box = document.getElementById("search"), results = document.getElementById("results");
	const index = await (await fetch(window.DOCS_UP + "search.json")).json();
	const lower = index.map(e => (e.title + " " + e.section + " " + e.text).toLowerCase());
	let picked = 0;
	function show() {
		const words = box.value.toLowerCase().split(/\s+/).filter(Boolean);
		if (!words.length) { results.hidden = true; return; }
		const hits = [];
		lower.forEach((text, k) => {
			if (!words.every(w => text.includes(w))) return;
			const head = (index[k].title + " " + index[k].section).toLowerCase();
			hits.push([words.reduce((n, w) => n + (head.includes(w) ? 10 : 0) + text.split(w).length, 0), k]);
		});
		hits.sort((a, b) => b[0] - a[0]);
		picked = 0;
		results.innerHTML = hits.slice(0, 14).map(([, k], n) => {
			const e = index[k];
			const at = e.text.toLowerCase().indexOf(words[0]);
			const snippet = e.text.slice(Math.max(0, at - 50), at + 110).replace(/[<>&]/g, "");
			return `<a class="${n ? "" : "on"}" href="${window.DOCS_UP}${e.page}${e.anchor ? "#" + e.anchor : ""}"><b>${e.section || e.title}</b>` +
				`<small>${e.title} · …${snippet}…</small></a>`;
		}).join("") || "<a>no match</a>";
		results.hidden = false;
	}
	box.addEventListener("input", show);
	box.addEventListener("keydown", event => {
		const links = [...results.querySelectorAll("a")];
		if (event.key === "Escape") { box.value = ""; results.hidden = true; }
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			picked = (picked + (event.key === "ArrowDown" ? 1 : links.length - 1)) % links.length;
			links.forEach((a, n) => a.classList.toggle("on", n === picked));
		}
		if (event.key === "Enter" && links[picked] && links[picked].href) location.href = links[picked].href;
	});
	document.addEventListener("keydown", event => { if (event.key === "/" && document.activeElement !== box) { event.preventDefault(); box.focus(); } });
	document.addEventListener("click", event => { if (!results.contains(event.target) && event.target !== box) results.hidden = true; });
})();
