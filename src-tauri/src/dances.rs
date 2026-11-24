//! The dances folder: MMD dance motions (.vmd) that any MMD model can play when
//! you ask her to dance. It is a plain folder so people can drop dances in by hand:
//!
//! - a loose `Name.vmd` is a dance called "Name" (a `Name.wav`/`.mp3`/`.ogg` next
//!   to it is its music), or
//! - a folder `Name/` is a dance called "Name": every .vmd inside it is played
//!   together (dance packs often split body, face and lip motion), except camera
//!   motions, and the first audio file inside is its music.
//!
//! Default: `Documents\Desktop Assistant\Dances` (setting `dances_folder`).
//! Files are served to the page as `model://localhost/dance/<relative path>`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Dance {
    pub name: String,
    /// Motion files relative to the dances folder, with "/".
    pub motions: Vec<String>,
    /// Music relative to the dances folder, if there is any.
    pub audio: Option<String>,
}

pub const README: &str = "\
Dances for Desktop Assistant
============================

Ask an MMD (.pmx) character to dance in chat (\"dance\", \"can you dance?\",
\"dance Lamb\") and she plays one of the dances in this folder, then goes back
to her idle. With no dances here she does a simple built-in dance.

Add a dance in either of these ways:

  * Drop a single motion file here, e.g.  Lamb.vmd
    Music with the same name next to it (Lamb.wav / Lamb.mp3 / Lamb.ogg) plays along.

  * Or make a folder for it, e.g.  Lamb\\  and put the dance's .vmd files inside.
    All .vmd files in the folder play together (body, face and lip motions);
    camera motions (camera.vmd, カメラ.vmd) are skipped. The first .wav, .mp3 or
    .ogg file in the folder is the music.

Say a dance's name to pick it (\"dance Lamb\"); otherwise she picks one at random.
Dance motions are made for the standard MMD skeleton, so most of them work with
most models. Check each motion's terms of use: many are free for personal use only.
";

const AUDIO: [&str; 3] = ["wav", "mp3", "ogg"];

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn is_camera(path: &Path) -> bool {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    stem.contains("camera") || stem.contains("カメラ") || stem == "cam" || stem.starts_with("cam_") || stem.ends_with("_cam")
}

fn rel(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    out.sort();
    out
}

/// Every file under `dir`, up to `depth` folders down.
fn files_under(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    for path in sorted_entries(dir) {
        if out.len() >= 200 {
            return;
        }
        if path.is_dir() {
            if depth > 0 {
                files_under(&path, depth - 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// The dances in `root`, sorted by name.
pub fn list(root: &Path) -> Vec<Dance> {
    let mut dances = Vec::new();
    let top = sorted_entries(root);
    for path in &top {
        if dances.len() >= 200 {
            break;
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if path.is_dir() {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut files = Vec::new();
            files_under(path, 2, &mut files);
            let motions: Vec<String> =
                files.iter().filter(|f| ext(f) == "vmd" && !is_camera(f)).filter_map(|f| rel(root, f)).collect();
            if motions.is_empty() {
                continue;
            }
            let audio = files.iter().find(|f| AUDIO.contains(&ext(f).as_str())).and_then(|f| rel(root, f));
            dances.push(Dance { name, motions, audio });
        } else if ext(path) == "vmd" && !is_camera(path) {
            let audio = top
                .iter()
                .find(|a| a.is_file() && AUDIO.contains(&ext(a).as_str()) && a.file_stem() == path.file_stem())
                .and_then(|a| rel(root, a));
            dances.push(Dance { name, motions: rel(root, path).into_iter().collect(), audio });
        }
    }
    dances.sort_by_key(|d| d.name.to_lowercase());
    dances
}

/// Makes the folder (with a README explaining how to add dances) if it isn't there yet.
pub fn ensure(root: &Path) {
    if root.is_dir() {
        return;
    }
    if fs::create_dir_all(root).is_ok() {
        let _ = fs::write(root.join("README.txt"), README.replace('\n', "\r\n"));
    }
}

/// Opens the folder in the file manager.
pub fn open(root: &Path) -> Result<(), String> {
    ensure(root);
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(root).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_loose_files_and_folders() {
        let root = std::env::temp_dir().join(format!("da-dances-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Lamb/motion")).unwrap();
        fs::create_dir_all(root.join("Empty")).unwrap();
        fs::write(root.join("Wave.vmd"), "x").unwrap();
        fs::write(root.join("Wave.mp3"), "x").unwrap();
        fs::write(root.join("Solo.vmd"), "x").unwrap();
        fs::write(root.join("camera.vmd"), "x").unwrap();
        fs::write(root.join("README.txt"), "x").unwrap();
        fs::write(root.join("Lamb/motion/body.vmd"), "x").unwrap();
        fs::write(root.join("Lamb/lip.vmd"), "x").unwrap();
        fs::write(root.join("Lamb/カメラ.vmd"), "x").unwrap();
        fs::write(root.join("Lamb/song.wav"), "x").unwrap();
        fs::write(root.join("Empty/notes.txt"), "x").unwrap();

        let dances = list(&root);
        assert_eq!(
            dances,
            vec![
                Dance {
                    name: "Lamb".into(),
                    motions: vec!["Lamb/lip.vmd".into(), "Lamb/motion/body.vmd".into()],
                    audio: Some("Lamb/song.wav".into()),
                },
                Dance { name: "Solo".into(), motions: vec!["Solo.vmd".into()], audio: None },
                Dance { name: "Wave".into(), motions: vec!["Wave.vmd".into()], audio: Some("Wave.mp3".into()) },
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_writes_a_readme_once() {
        let root = std::env::temp_dir().join(format!("da-dances-new-{}", std::process::id())).join("Dances");
        let _ = fs::remove_dir_all(&root);
        ensure(&root);
        assert!(root.join("README.txt").is_file());
        assert!(list(&root).is_empty());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }
}
