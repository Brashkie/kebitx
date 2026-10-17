//! kebitx-repair — repairs that can be checked.
//!
//! Repair is the most dangerous thing this tool does. In the reference world,
//! 86 blocks turned into missing textures not because of an add-on but because
//! of an automatic "fix" that rebuilt atlas entries from what it had read — and
//! it could not read `variations`. So every repair here follows five rules:
//!
//! 1. **Preview first.** `plan` only describes changes; nothing is written
//!    until `apply` is called with that plan.
//! 2. **Backup.** Every file touched is copied, with an index, OUTSIDE the
//!    world (so it is never packaged with it) before anything is written;
//!    `restore` puts it all back.
//! 3. **Surgical edits.** A change replaces one exact quoted literal in the
//!    text. Files are never re-serialized: comments, key order, formatting,
//!    tints, weights, `variations` and unknown keys stay byte for byte.
//! 4. **Verified.** The world is analysed again after writing. Unless the
//!    result is strictly better — nothing worse, something better — every
//!    change is rolled back.
//! 5. **Only what is proved.** References whose case differs from the file
//!    on disk (they break on Android/iOS), and — only when asked — deleting
//!    files the orphan pass proved unused.
#![forbid(unsafe_code)]

pub mod apply;
pub mod plan;
pub mod score;

pub use apply::{apply, restore, Applied};
pub use plan::{plan, Change, Fix, Plan};
pub use score::{score, Score};
