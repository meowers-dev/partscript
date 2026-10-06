//! The gallery's pictures (docs/img/*.webp), rendered headless from a local preview:
//!
//!   python3 -m http.server -d site 8765 &        # or any static server: the preview, serving the built models
//!   cargo run -p site -- shots [names]            # needs chromium and ImageMagick (magick) on the PATH
//!
//! Each shot is the contact sheet (sheet.html) showing one model from one direction, then shrunk to
//! WebP. Change SHOTS to change the gallery; `models` must have built the models first.

use std::path::PathBuf;
use std::process::Command;

use crate::root;

const SERVER: &str = "http://localhost:8765";

/// name, prop, view direction, zoom, PSX look
const SHOTS: [(&str, &str, &str, f64, bool); 14] = [
	("fairground", "fairground", ".3,.35,1", 1.5, false),
	("tavern", "tavern_room", ".55,.6,1", 1.4, false),
	("hill_chapel", "hill_chapel", ".9,.35,.7", 2.2, false),
	("ruin_stages", "ruin_stages", ".4,.35,1", 1.5, false),
	("standing_stones", "standing_stones", ".7,.45,1", 1.6, false),
	("apartment", "apartment_open", ".55,.4,1", 1.3, false),
	("graveyard", "graveyard", ".6,.5,1", 1.5, false),
	("writers_desk", "writers_desk", ".7,.5,1", 1.3, false),
	("kitchen_dresser", "kitchen_dresser", ".6,.3,1", 1.4, false),
	("oak_tree", "oak_tree", ".7,.3,1", 1.2, false),
	("fenced_garden", "fenced_garden", ".6,.6,1", 1.4, false),
	("bicycle", "bicycle", "1,.3,.4", 1.4, false),
	("railing", "railing_corners", ".6,.6,1", 1.4, false),
	("fence_odds", "fence_odds", ".25,.75,1", 1.7, false),
];

fn run(command: &mut Command) -> Result<(), String> {
	let output = command.output().map_err(|e| format!("{:?}: {e}", command.get_program()))?;
	if !output.status.success() {
		return Err(format!("{:?} failed: {}", command.get_program(), String::from_utf8_lossy(&output.stderr)));
	}
	Ok(())
}

fn shoot(name: &str, prop: &str, view: &str, zoom: f64, psx: bool, size: u32) -> Result<PathBuf, String> {
	let png = std::env::temp_dir().join(format!("partscript-shot-{}-{name}.png", std::process::id()));
	let url = format!("{SERVER}/sheet.html?only={prop}&cols=1&cell={size}&view={view}&zoom={zoom}{}", if psx { "&psx=1" } else { "" });
	run(Command::new("chromium").args([
		"--headless=new",
		"--use-angle=swiftshader",
		"--enable-unsafe-swiftshader",
		"--hide-scrollbars",
		&format!("--window-size={size},{size}"),
		"--virtual-time-budget=60000",
		&format!("--screenshot={}", png.display()),
		&url,
	]))?;
	let target = root().join("docs/img").join(format!("{name}.webp"));
	// The label row off the top, then WebP.
	let result = run(Command::new("magick").arg(&png).args(["-alpha", "off", "-crop", &format!("{size}x{}+0+22", size - 22), "+repage", "-quality", "82", "-define", "webp:method=6"]).arg(&target));
	let _ = std::fs::remove_file(&png);
	result.map(|_| target)
}

pub fn build(names: &[String]) -> Result<(), String> {
	std::fs::create_dir_all(root().join("docs/img")).map_err(|e| e.to_string())?;
	for (name, prop, view, zoom, psx) in SHOTS {
		if !names.is_empty() && !names.iter().any(|n| n == name) {
			continue;
		}
		let target = shoot(name, prop, view, zoom, psx, 720)?;
		let size = std::fs::metadata(&target).map_err(|e| e.to_string())?.len();
		println!("{}  {} KB", target.strip_prefix(root()).unwrap().display(), size / 1024);
	}
	Ok(())
}
