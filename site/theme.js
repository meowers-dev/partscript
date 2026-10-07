// Light or dark: what you picked last with a menu bar's "Dark mode" button, else what the system prefers.
// Loaded in <head>, before the page draws, so it never flashes the other one. docs.css does the rest.
(() => {
	const KEY = "partscript:theme", root = document.documentElement, system = matchMedia("(prefers-color-scheme: dark)");
	function apply() {
		root.dataset.theme = localStorage.getItem(KEY) || (system.matches ? "dark" : "light");
		const dark = root.dataset.theme === "dark";
		for (const button of document.querySelectorAll("button.theme")) {
			button.textContent = dark ? "Light mode" : "Dark mode";
			button.setAttribute("aria-pressed", dark);
		}
	}
	apply();
	system.addEventListener("change", apply);
	window.addEventListener("storage", event => { if (event.key === KEY) apply(); });  // another tab changed it
	document.addEventListener("DOMContentLoaded", () => {
		apply();
		for (const button of document.querySelectorAll("button.theme")) {
			button.addEventListener("click", () => {
				localStorage.setItem(KEY, root.dataset.theme === "dark" ? "light" : "dark");
				apply();
			});
		}
	});
})();
