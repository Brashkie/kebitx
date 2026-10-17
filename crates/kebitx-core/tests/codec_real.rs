//! The codecs against real files: every entry of an installed Java `.jar`
//! (each one's CRC-32 checked) and every PNG in it decoded. Skipped where no
//! Java Edition is installed.

use kebitx_core::codec::{png, zip::ZipArchive};
use std::path::PathBuf;

fn newest_jar() -> Option<PathBuf> {
    let base = std::env::var_os("KEBITX_JAVA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(".minecraft")))?;
    let mut jars: Vec<PathBuf> = std::fs::read_dir(base.join("versions"))
        .ok()?
        .flatten()
        .map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            e.path().join(format!("{id}.jar"))
        })
        .filter(|p| p.is_file())
        .collect();
    jars.sort();
    jars.pop()
}

#[test]
fn every_jar_entry_and_every_png_in_it() {
    let Some(jar) = newest_jar() else { return };
    let zip = ZipArchive::open(&jar).expect("opens");
    let mut names: Vec<&String> = zip.names().collect();
    names.sort();
    let (mut entries, mut pngs) = (0, 0);
    for name in names {
        let data = zip
            .read(name, 256 * 1024 * 1024)
            .unwrap_or_else(|e| panic!("{e}"));
        entries += 1;
        if name.ends_with(".png") {
            let img = png::decode(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
            let (w, h) = kebitx_core::image::png(&data).expect("header");
            assert_eq!((img.width, img.height), (w, h), "{name}");
            pngs += 1;
        }
    }
    assert!(
        entries > 1000 && pngs > 1000,
        "{entries} entries, {pngs} PNG"
    );
}
