//! An example kebitx filter, showing the whole contract.
//!
//! A filter is any program. It runs in the build's scratch folder, finds the
//! packs through `KEBITX_BP` / `KEBITX_RP`, changes them in place, and exits
//! with 0 on success. This one:
//!
//! - `stamp`: writes `built_by.txt` into the behavior pack;
//! - `fail`: exits with 3, to show a filter stopping the build.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::exit;

fn main() {
    match env::args().nth(1).as_deref() {
        Some("stamp") => {
            let Some(bp) = env::var_os("KEBITX_BP") else {
                eprintln!("no behavior pack in this project");
                exit(1);
            };
            let name = env::var("KEBITX_NAME").unwrap_or_default();
            if let Err(e) = fs::write(
                std::path::Path::new(&bp).join("built_by.txt"),
                format!("{name}\n"),
            ) {
                eprintln!("{e}");
                exit(1);
            }
        }
        Some("fail") => exit(3),
        _ => {
            eprintln!("usage: kebitx-filter-example stamp|fail");
            exit(2);
        }
    }
}
