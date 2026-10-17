use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Largest text file (JSON, scripts, functions, materials) read whole. The
/// largest in the reference world is 3.8 MB; a file of gigabytes is hostile
/// or broken, and is reported as unreadable instead of filling memory.
pub const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
/// Largest texture or sound read whole (to compare bytes).
pub const MAX_ASSET_BYTES: u64 = 256 * 1024 * 1024;

/// `fs::read`, refusing anything larger than `max` bytes — checked before
/// reading, and enforced while reading, so a file that grows meanwhile is
/// refused too.
pub fn read_limited(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let f = fs::File::open(path)?;
    let too_big = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("larger than {max} bytes"),
        )
    };
    let len = f.metadata()?.len();
    if len > max {
        return Err(too_big());
    }
    let mut out = Vec::with_capacity(len as usize);
    f.take(max + 1).read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(too_big());
    }
    Ok(out)
}

/// `fs::read_to_string` with `MAX_TEXT_BYTES`.
pub fn read_text(path: &Path) -> io::Result<String> {
    String::from_utf8(read_limited(path, MAX_TEXT_BYTES)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Pass 0a: discover texture files under `<rp_root>/textures/`.
/// Relative to `rp_root`, WITH extension, `/` separators (Windows `\` normalized).
/// Only `.png`/`.tga`; content is never opened (0-byte fixtures are enough).
pub fn discover_textures(rp_root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(&rp_root.join("textures"), &mut |path| {
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "tga"))
            .unwrap_or(false);
        if is_image {
            if let Ok(rel) = path.strip_prefix(rp_root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    });
    out
}

/// Pass 0b: discover entity files (`*.json`) under `<rp_root>/entity/`.
pub fn discover_entities(rp_root: &Path) -> Vec<PathBuf> {
    collect_json(&rp_root.join("entity"))
}

/// Pass 0c: discover model files (`*.json`) under `<rp_root>/models/`.
pub fn discover_models(rp_root: &Path) -> Vec<PathBuf> {
    collect_json(&rp_root.join("models"))
}

/// Pass 0d: animation and animation-controller files. Both folders, because a
/// file is indexed by the keys it actually contains, not by where it sits.
pub fn discover_animation_files(rp_root: &Path) -> Vec<PathBuf> {
    let mut out = collect_json(&rp_root.join("animations"));
    out.extend(collect_json(&rp_root.join("animation_controllers")));
    out
}

/// Pass 0e: render-controller files.
pub fn discover_render_controllers(rp_root: &Path) -> Vec<PathBuf> {
    collect_json(&rp_root.join("render_controllers"))
}

const IMAGE_EXTS: [&str; 4] = [".png", ".tga", ".jpg", ".jpeg"];
const SOUND_EXTS: [&str; 4] = [".ogg", ".wav", ".fsb", ".mp3"];

/// A pack-relative path (lowercased, `/`) of a texture or sound file: the
/// assets that can be unused, duplicated, or shipped by the game.
pub fn is_asset(lower_rel: &str) -> bool {
    (lower_rel.starts_with("textures/") && IMAGE_EXTS.iter().any(|e| lower_rel.ends_with(e)))
        || (lower_rel.starts_with("sounds/") && SOUND_EXTS.iter().any(|e| lower_rel.ends_with(e)))
}

/// The form in which an asset path is compared: lowercased, `/` separators,
/// no leading `./` or `/`, no image/sound extension. References omit the
/// extension and the game ignores case, so `Textures/Blocks/Stone.PNG` and
/// `textures/blocks/stone` are the same asset.
pub fn asset_stem(s: &str) -> String {
    let mut s = s.trim().replace('\\', "/").to_ascii_lowercase();
    while let Some(rest) = s.strip_prefix("./").or_else(|| s.strip_prefix('/')) {
        s = rest.to_string();
    }
    for e in IMAGE_EXTS.iter().chain(SOUND_EXTS.iter()) {
        if let Some(stem) = s.strip_suffix(e) {
            // Trimmed AFTER the extension goes too: a file `raw_burger .png`
            // and a reference `"raw_burger "` (a real pair in the reference
            // world) must give the same stem. Trimming only the whole string
            // left the file's space before `.png` and made a used file look
            // unused.
            return stem.trim_end().to_string();
        }
    }
    s
}

/// Every file under `dir`, recursively. Missing dir => empty.
pub fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut |p| out.push(p.to_path_buf()));
    out
}

/// Every `*.json` under `dir`, recursively.
pub fn json_files(dir: &Path) -> Vec<PathBuf> {
    collect_json(dir)
}

fn collect_json(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut |path| {
        let is_json = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if is_json {
            out.push(path.to_path_buf());
        }
    });
    out
}

/// Recursively visit every file under `dir`, calling `f` on each. Missing dir => no-op.
fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // The listing already says what each entry is: no extra system call per
        // file (`is_dir` costs one, and on Windows it adds up over thousands).
        // Only a symbolic link needs a real check of what it points to.
        let is_dir = match entry.file_type() {
            Ok(t) if t.is_symlink() => path.is_dir(),
            Ok(t) => t.is_dir(),
            Err(_) => path.is_dir(),
        };
        if is_dir {
            walk(&path, f);
        } else {
            f(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_over_the_limit_is_refused_not_read() {
        let dir = std::env::temp_dir().join(format!("kebitx-limit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("big.json");
        fs::write(&f, vec![b' '; 1000]).unwrap();
        assert_eq!(
            read_limited(&f, 1000).unwrap().len(),
            1000,
            "exactly at the limit: read"
        );
        let e = read_limited(&f, 999).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(&dir);
    }
}
