//! Which model a stack shows ([`ItemKey`]): its item and what its
//! components change about the model ([`Variant`]). Looked up for every
//! drawn stack, every frame.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{DamageImpl, MaxDamageImpl};
use pumpkin_data::item_stack::ItemStack;

use super::{COUNT_CUTS, Count, count_cuts, definition_id, potion_color};

/// Which model a stack shows: its item, and the potion colour for items
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ItemKey {
    pub item: u16,
    /// What the stack's components add (`item_model`, `custom_model_data`,
    pub variant: Option<Arc<Variant>>,
    pub potion_color: Option<[u8; 3]>,
    /// The stack size as far as the item's model tells sizes apart: the
    pub count: u8,
    pub max_stack: u8,
}

impl ItemKey {
    pub fn of(assets_root: &Path, stack: &ItemStack) -> Self {
        let max_stack = stack.get_max_stack_size();
        let variant = Variant::of(stack).map(Arc::new);
        Self {
            item: stack.item.id,
            potion_color: potion_color(stack),
            count: model_count(assets_root, stack, max_stack, variant.as_deref()),
            max_stack,
            variant,
        }
    }

    /// The count the model is resolved with.
    pub fn count(&self) -> Count {
        Count {
            value: i32::from(self.count),
            max_stack: i32::from(self.max_stack),
        }
    }
}

/// The components of a stack that pick its model: `item_model` (another
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Variant {
    /// The definition named by `item_model`, when it isn't the item's own.
    pub model: Option<String>,
    /// `CustomModelData.floats`, as bits to be hashable.
    pub floats: Vec<u32>,
    pub flags: Vec<bool>,
    pub strings: Vec<String>,
    pub colors: Vec<i32>,
    pub dye: Option<i32>,
    /// `ItemStack.nextDamageWillBreak`: the `minecraft:broken` condition.
    pub broken: bool,
    /// `ItemStack.has`: bit `1 << DataComponent::to_id()`.
    pub present: u128,
    /// `ItemStack.hasNonDefault`: components in the patch.
    pub patched: u128,
    /// `minecraft:trim` material (`SelectItemModel` `trim_material`).
    pub trim_material: Option<String>,
}

impl Variant {
    /// `None` for a stack with none of these components.
    pub fn of(stack: &ItemStack) -> Option<Self> {
        use pumpkin_data::data_component_impl::{
            CustomModelDataImpl, DyedColorImpl, ItemModelImpl, TrimImpl,
        };
        let own = stack.item.registry_key;
        let model = stack
            .get_data_component::<ItemModelImpl>()
            .map(|m| m.id.to_string())
            .filter(|id| id.trim_start_matches("minecraft:") != own);
        let data = stack.get_data_component::<CustomModelDataImpl>();
        let dye = stack.get_data_component::<DyedColorImpl>().map(|d| d.rgb);
        let trim_material = stack
            .get_data_component::<TrimImpl>()
            .and_then(|trim| trim.material_enum())
            .map(|material| material.asset_id().to_owned());
        let broken = next_damage_will_break(stack);
        let (present, patched) = component_bits(stack);
        if model.is_none()
            && data.is_none()
            && dye.is_none()
            && trim_material.is_none()
            && !broken
            && patched == 0
        {
            return None;
        }
        let mut variant = Self {
            model,
            dye,
            broken,
            present,
            patched,
            trim_material,
            ..Self::default()
        };
        if let Some(data) = data {
            variant.floats = data.floats.iter().map(|f| f.to_bits()).collect();
            variant.flags.clone_from(&data.flags);
            variant.strings.clone_from(&data.strings);
            variant.colors.clone_from(&data.colors);
        }
        Some(variant)
    }

    /// `CustomModelData.getFloat`.
    pub(super) fn float(&self, index: usize) -> Option<f32> {
        self.floats.get(index).map(|&bits| f32::from_bits(bits))
    }
}

fn model_count(
    assets_root: &Path,
    stack: &ItemStack,
    max_stack: u8,
    variant: Option<&Variant>,
) -> u8 {
    let definition_key = definition_id(stack.item.registry_key, variant);
    let mut known = COUNT_CUTS.lock().unwrap_or_else(|e| e.into_inner());
    let cuts = known
        .get_or_insert_with(HashMap::new)
        .entry(definition_key.to_owned())
        .or_insert_with(|| {
            let definition = crate::assets::json_resource(assets_root, "items", definition_key);
            let mut cuts = Vec::new();
            if let Some(definition) = definition {
                count_cuts(&definition["model"], i32::from(max_stack), &mut cuts);
            }
            cuts.sort_unstable();
            cuts.dedup();
            cuts
        });
    let count = i32::from(stack.item_count);
    cuts.iter()
        .rev()
        .find(|&&cut| cut <= count)
        .map_or(1, |&cut| cut.clamp(1, 255) as u8)
}

fn component_bits(stack: &ItemStack) -> (u128, u128) {
    let mut present = 0u128;
    let mut patched = 0u128;
    for id in 0u8..=DataComponent::CushionColor.to_id() {
        let Some(component) = DataComponent::try_from_id(id) else {
            continue;
        };
        if stack.has_data_component(component) {
            present |= 1u128 << id;
        }
    }
    for (id, data) in &stack.patch {
        if data.is_some() {
            patched |= 1u128 << id.to_id();
        }
    }
    (present, patched)
}

/// `ItemStack.nextDamageWillBreak`: one more hit and the stack is gone.
fn next_damage_will_break(stack: &ItemStack) -> bool {
    stack
        .get_data_component::<MaxDamageImpl>()
        .is_some_and(|max| {
            let damage = stack
                .get_data_component::<DamageImpl>()
                .map_or(0, |d| d.damage);
            damage >= max.max_damage - 1
        })
}
