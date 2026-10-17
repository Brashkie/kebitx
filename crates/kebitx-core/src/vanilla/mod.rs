//! The vanilla catalog: what the GAME itself provides.
//!
//! This is the piece whose absence made the engine's own CLI print a disclaimer:
//! without it, every addon that reuses a vanilla texture or geometry looks broken.
//! On one real pack that was 105 "broken references" of which 2 were real.
//!
//! It feeds `EdgeState::External`, which the engine already had but nothing could
//! prove. The invariant is unchanged and now actually holds: a reference the game
//! supplies is External, not KnownMissing, so it never produces a finding.
//!
//! Two spaces, exactly as the rest of the engine:
//!   - textures live in PATH space    (`textures/entity/cow/cow` + probe `.png`/`.tga`)
//!   - geometry lives in IDENTIFIER space (`geometry.cow`, found inside a model)
//!
//! Vanilla ships its assets across ~56 stacked layers (`vanilla`, `vanilla_1.14`,
//! ... `vanilla_1.26.52`). For "does this exist at all" the union of every layer is
//! the right answer, so no layer ordering is needed here.

pub mod brarchive;
pub mod catalog;
pub mod files;

pub use catalog::{TileRef, VanillaCatalog};

use std::path::{Path, PathBuf};

#[cfg(windows)]
const WINDOWS_APPS: &str = r"C:\Program Files\WindowsApps";

/// Locate the game's own files, on any system.
///
/// `KEBITX_BEDROCK_DIR` first: an installed client (with `data/resource_packs`),
/// a Bedrock Dedicated Server (`resource_packs` at its root — Linux or
/// Windows), or Mojang's published samples (`resource_pack`, `behavior_pack`).
/// Then, on Windows, the installed client: the GDK install under
/// `XboxGames`, else the newest UWP package. Elsewhere there is no client to
/// find; without the variable the catalog is empty and every verdict that
/// needs the game says so, as on a machine without it.
pub fn install_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("KEBITX_BEDROCK_DIR").map(PathBuf::from) {
        return is_source(&d).then_some(d);
    }
    platform_install()
}

/// Does this folder hold the game's packs in one of the known layouts?
pub fn is_source(dir: &Path) -> bool {
    ["resource_packs", "behavior_packs"]
        .iter()
        .any(|k| dir.join("data").join(k).is_dir() || dir.join(k).is_dir())
        || dir.join("resource_pack").is_dir()
}

/// The folder whose `vanilla*` children are the layers of a stack
/// (`resource_packs` or `behavior_packs`): the client's `data/<stack>`, else a
/// server's `<stack>` at the root. For Mojang's samples (`resource_pack`,
/// one layer) it is `None`: see `samples_layer`.
pub fn stack_dir(install: &Path, stack: &str) -> Option<PathBuf> {
    let client = install.join("data").join(stack);
    if client.is_dir() {
        return Some(client);
    }
    let server = install.join(stack);
    server.is_dir().then_some(server)
}

/// Mojang's samples keep one layer per stack: `resource_pack`, `behavior_pack`.
pub fn samples_layer(install: &Path, stack: &str) -> Option<PathBuf> {
    let single = install.join(stack.trim_end_matches('s'));
    single.is_dir().then_some(single)
}

#[cfg(not(windows))]
fn platform_install() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn platform_install() -> Option<PathBuf> {
    // the GDK build (Microsoft Store, 2025 on) installs to XboxGames; the UWP
    // package may still be there too: the newest version is the game
    let gdk = ["C", "D", "E"]
        .iter()
        .map(|d| PathBuf::from(format!(r"{d}:\XboxGames\Minecraft for Windows\Content")))
        .filter(|p| is_source(p));
    gdk.chain(uwp_install()).max_by_key(|p| package_version(p))
}

#[cfg(windows)]
fn uwp_install() -> Option<PathBuf> {
    let mut best: Option<((u32, u32, u32, u32), PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(WINDOWS_APPS) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(v) = version_of(&name) {
                let p = e.path();
                if p.is_dir() && best.as_ref().map(|(bv, _)| v > *bv).unwrap_or(true) {
                    best = Some((v, p));
                }
            }
        }
    }
    if let Some((_, p)) = best {
        return Some(p);
    }
    // WindowsApps does not always allow listing, but it does allow entering a
    // known name. The package's full name is in the registry: `reg.exe` reads
    // it in ~10 ms, where starting PowerShell costs ~500 ms on every run.
    if let Some(p) = from_registry() {
        return Some(p);
    }
    // Last resort: ask the package manager.
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-AppxPackage -Name Microsoft.MinecraftUWP).InstallLocation",
        ])
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!t.is_empty() && Path::new(&t).is_dir()).then(|| PathBuf::from(t))
}

#[cfg(windows)]
/// The newest Minecraft package registered for this user, from the AppModel
/// repository keys (`...\Packages\Microsoft.MinecraftUWP_1.26.5203.0_x64__8wekyb3d8bbwe`).
fn from_registry() -> Option<PathBuf> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages",
            "/f",
            "Microsoft.MinecraftUWP_",
            "/k",
        ])
        .output()
        .ok()?;
    newest_package(&String::from_utf8_lossy(&out.stdout))
        .map(|name| Path::new(WINDOWS_APPS).join(name))
        .filter(|p| p.is_dir())
}

/// The newest `Microsoft.MinecraftUWP_*` package name in `reg query` output.
#[cfg_attr(not(windows), allow(dead_code))]
fn newest_package(reg_output: &str) -> Option<String> {
    reg_output
        .lines()
        .filter_map(|l| l.trim().rsplit('\\').next())
        .filter_map(|name| version_of(name).map(|v| (v, name.to_string())))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, name)| name)
}

fn version_of(dir_name: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = dir_name.strip_prefix("Microsoft.MinecraftUWP_")?;
    let parts: Vec<&str> = rest.split('_').next()?.split('.').collect();
    if parts.len() < 4 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

/// Game version as players see it. The package is `1.26.5203.0` but the game is
/// `1.26.52`: that third field packs patch and build together. Mixing them up
/// makes comparisons against a pack's `min_engine_version` wrong.
pub fn game_version(install: &Path) -> String {
    match package_version(install) {
        Some(v) => format!("{}.{}.{}", v.0, v.1, v.2 / 100),
        None => "unknown".into(),
    }
}

/// The package version (`1.26.5203.0`): in a UWP install's folder name, else in
/// the `<Identity Version="...">` of a GDK install's `MicrosoftGame.Config` or
/// `appxmanifest.xml`.
pub fn package_version(install: &Path) -> Option<(u32, u32, u32, u32)> {
    if let Some(v) = version_of(&install.file_name().unwrap_or_default().to_string_lossy()) {
        return Some(v);
    }
    for file in [
        "MicrosoftGame.Config",
        "MicrosoftGame.config",
        "appxmanifest.xml",
        "AppxManifest.xml",
    ] {
        let Ok(text) = std::fs::read_to_string(install.join(file)) else {
            continue;
        };
        let Some(identity) = text
            .split("<Identity")
            .nth(1)
            .and_then(|t| t.split('>').next())
        else {
            continue;
        };
        let Some(v) = identity
            .split("Version=\"")
            .nth(1)
            .and_then(|t| t.split('"').next())
        else {
            continue;
        };
        if let Some(parsed) = version_of(&format!("Microsoft.MinecraftUWP_{v}_")) {
            return Some(parsed);
        }
    }
    None
}

/// The `vanilla`, `vanilla_1.14`, ... layers of a pack root.
pub fn layers(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .map(|n| {
                        let n = n.to_string_lossy();
                        n == "vanilla" || n.starts_with("vanilla_")
                    })
                    .unwrap_or(false)
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_package_from_reg_output() {
        let out = r"
HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages\Microsoft.MinecraftUWP_1.21.9401.0_x64__8wekyb3d8bbwe
HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages\Microsoft.MinecraftUWP_1.26.5203.0_x64__8wekyb3d8bbwe
End of search: 2 match(es) found.
";
        assert_eq!(
            newest_package(out).as_deref(),
            Some("Microsoft.MinecraftUWP_1.26.5203.0_x64__8wekyb3d8bbwe")
        );
        assert_eq!(newest_package("End of search: 0 match(es) found."), None);
    }
}
