//! `*.material` files: JSON despite the extension.
//!
//! ```json
//! { "materials": { "version": "1.0.0",
//!     "entity_static": { ... },
//!     "entity_alphatest:entity_nocull": { ... } } }
//! ```
//!
//! Each key defines a material; after a `:` comes the material it inherits
//! from. `version` is not a material.

use crate::json::Json;

/// The material names a `.material` file defines.
pub fn material_names(root: &Json) -> Vec<String> {
    root.get("materials")
        .and_then(Json::as_object)
        .map(|m| {
            m.keys()
                .filter(|k| k.as_str() != "version")
                .map(|k| k.split(':').next().unwrap_or(k).trim().to_string())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn names_without_parents_or_version() {
        let v = json::parse(r#"{"materials": {"version": "1.0.0", "entity_static": {}, "entity_alphatest:entity_nocull": {}}}"#)
            .unwrap();
        let mut n = material_names(&v);
        n.sort();
        assert_eq!(n, ["entity_alphatest", "entity_static"]);
    }
}
