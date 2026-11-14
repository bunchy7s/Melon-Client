//! Which textures go into the item atlas, built once per resource load
//! (vanilla `AtlasProvider.itemsList`: every `textures/item/` file), plus
//! what the packs' `atlases/items.json` and `atlases/blocks.json` add, since
//! custom-model packs list their own folders there.
//!
//! Held items sample this atlas or the block atlas; only what neither
//! holds (a downloaded skin, a composed banner, a texture no atlas source
//! names) gets a texture of its own.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::assets::packs::{self, ResourceStack};

/// Every texture ID (`namespace:path`, without `textures/` and `.png`) the
/// item atlas packs.
pub fn texture_ids(assets_root: &Path) -> Vec<String> {
    let stack = packs::active();
    let namespaces = stack.namespaces(assets_root);
    let mut ids = BTreeSet::new();
    for namespace in &namespaces {
        list_dir(&stack, assets_root, namespace, "item", &mut ids);
    }
    for atlas in ["items", "blocks"] {
        for bytes in stack.find_all(assets_root, "minecraft", &format!("atlases/{atlas}.json")) {
            let Ok(json) = serde_json::from_slice::<Value>(packs::meta::strip_bom(&bytes)) else {
                continue;
            };
            for source in json["sources"].as_array().into_iter().flatten() {
                add_source(&stack, assets_root, &namespaces, source, &mut ids);
            }
        }
    }
    ids.into_iter().collect()
}

/// One `sources` entry. `directory` and `single` only, and only where the
/// sprite is named after its file: other sources (palettes, unstitching)
/// stay with the textures of their own.
fn add_source(
    stack: &ResourceStack,
    assets_root: &Path,
    namespaces: &BTreeSet<String>,
    source: &Value,
    ids: &mut BTreeSet<String>,
) {
    let kind = source["type"].as_str().unwrap_or("");
    match kind.trim_start_matches("minecraft:") {
        "directory" => {
            let Some(dir) = source["source"].as_str() else {
                return;
            };
            let dir = dir.trim_end_matches('/');
            // The whole block folder is the block atlas's; the item folder
            // is listed already.
            if dir == "block" || dir == "item" {
                return;
            }
            if source["prefix"].as_str() != Some(&format!("{dir}/")) {
                return;
            }
            for namespace in namespaces {
                list_dir(stack, assets_root, namespace, dir, ids);
            }
        }
        "single" => {
            let Some(resource) = source["resource"].as_str() else {
                return;
            };
            if source["sprite"].as_str().is_some_and(|s| s != resource) {
                return;
            }
            ids.insert(canonical(resource));
        }
        _ => {}
    }
}

/// The PNGs under `textures/<dir>/` of `namespace`, as texture IDs.
fn list_dir(
    stack: &ResourceStack,
    assets_root: &Path,
    namespace: &str,
    dir: &str,
    ids: &mut BTreeSet<String>,
) {
    for path in stack.list(assets_root, namespace, &format!("textures/{dir}")) {
        if let Some(texture) = path
            .strip_prefix("textures/")
            .and_then(|p| p.strip_suffix(".png"))
        {
            ids.insert(format!("{namespace}:{texture}"));
        }
    }
}

/// `namespace:path`, `minecraft` when none is given.
pub fn canonical(id: &str) -> String {
    let (namespace, path) = crate::assets::split_id(id);
    format!("{namespace}:{path}")
}
