//! Where the game keeps its `com.mojang` folder.
//!
//! `KEBITX_COM_MOJANG` names it on any system; Android's is found from a
//! terminal on the device. Two layouts exist on Windows. The current launcher keeps packs under
//! `%APPDATA%\Minecraft Bedrock\Users\Shared\games\com.mojang`; the old UWP
//! package kept everything under its `LocalState`. The first candidate that
//! already has `development_behavior_packs` wins; otherwise the first that
//! exists.

use std::env;
use std::path::PathBuf;

pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    // any system: named by hand
    if let Some(d) = env::var_os("KEBITX_COM_MOJANG") {
        out.push(PathBuf::from(d));
    }
    // Android (from a terminal on the device)
    out.push(PathBuf::from(
        "/storage/emulated/0/Android/data/com.mojang.minecraftpe/files/games/com.mojang",
    ));
    if let Some(appdata) = env::var_os("APPDATA") {
        out.push(
            PathBuf::from(appdata)
                .join("Minecraft Bedrock")
                .join("Users")
                .join("Shared")
                .join("games")
                .join("com.mojang"),
        );
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        out.push(
            PathBuf::from(local)
                .join("Packages")
                .join("Microsoft.MinecraftUWP_8wekyb3d8bbwe")
                .join("LocalState")
                .join("games")
                .join("com.mojang"),
        );
    }
    out
}

pub fn com_mojang() -> Option<PathBuf> {
    let c = candidates();
    c.iter()
        .find(|p| p.join("development_behavior_packs").is_dir())
        .or_else(|| c.iter().find(|p| p.is_dir()))
        .cloned()
}
