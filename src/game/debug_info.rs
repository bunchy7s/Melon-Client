//! The F3 lines that need the running game: the server line, what is looked

use pumpkin_data::{Block, BlockStateId};

use super::InWorld;
use crate::gui::hud::debug::DebugExtras;
use crate::world::clouds::CloudStatus;

/// A block state as `BlockState.toString` says it: the block's id and its
pub fn describe_state(state: u16) -> (String, Vec<String>) {
    let Some(id) = BlockStateId::new(state) else {
        return ("minecraft:air".to_owned(), Vec::new());
    };
    let block = Block::from_state_id(id);
    let properties = block
        .properties(id)
        .map(|p| {
            p.to_props()
                .into_iter()
                .map(|prop| format!("{}: {}", prop.key_str(), prop.value_str()))
                .collect()
        })
        .unwrap_or_default();
    (format!("minecraft:{}", block.name), properties)
}

/// `ClipboardCommand` of F3+I for a block: the `/setblock` that makes it.
pub fn setblock_command(pos: [i32; 3], state: u16) -> String {
    let (name, properties) = describe_state(state);
    let properties = if properties.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = properties.iter().map(|p| p.replace(": ", "=")).collect();
        format!("[{}]", list.join(","))
    };
    format!(
        "/setblock {} {} {} {name}{properties}",
        pos[0], pos[1], pos[2]
    )
}

impl InWorld {
    pub(super) fn debug_extras(&self, clouds: CloudStatus, biome_blend: u8) -> DebugExtras {
        let mut extras = DebugExtras::default();
        let (sent, received) = self.subsystems.packets();
        extras.server = Some(if self.is_integrated() {
            match self.server_mspt() {
                Some(mspt) => {
                    format!("Integrated server @ {mspt:.1}/50.0 ms, {sent:.0} tx, {received:.0} rx")
                }
                None => format!("Integrated server, {sent:.0} tx, {received:.0} rx"),
            }
        } else {
            let brand = self.registries.brand.as_deref().unwrap_or("unknown");
            format!("\"{brand}\" server, {sent:.0} tx, {received:.0} rx")
        });
        extras.impactors.push(format!(
            "{}B: {biome_blend}",
            match clouds {
                CloudStatus::Off => "",
                CloudStatus::Fast => "fast-clouds ",
                CloudStatus::Fancy => "fancy-clouds ",
            }
        ));
        extras
            .impactors
            .push("Terrain Rendering: multidrawindirect".to_owned());
        if !self.debug_view.extended {
            return extras;
        }

        let feet = self.player.pos();
        let (bx, by, bz) = (
            feet.x.floor() as i32,
            feet.y.floor() as i32,
            feet.z.floor() as i32,
        );
        extras.speed = Some(format!(
            "Speed: {:.3} blocks/tick",
            self.player.velocity().length()
        ));
        if let Some(hit) = self.hit_block() {
            let [x, y, z] = hit.pos;
            let state = self.level.block_state(x, y, z).unwrap_or(0);
            let (name, properties) = describe_state(state);
            let mut group = vec![format!("Targeted Block: {x}, {y}, {z}"), name];
            group.extend(properties);
            extras.groups.push(group);
        }
        if let Some(entity) = self.hit_entity().and_then(|hit| self.entities.get(hit.id)) {
            extras.groups.push(vec![
                "Targeted Entity".to_owned(),
                format!("minecraft:{}", entity.ty.resource_name),
            ]);
        }
        if let Some(biome) = self.level.biome(bx, by, bz) {
            let name = self.biomes.name(biome).unwrap_or("[unregistered]");
            extras.regular.push(format!("Biome: {name}"));
        }
        let (block, sky) = self.level.light_at(bx, by, bz);
        extras.regular.push(format!(
            "Client Light: {} ({sky} sky, {block} block)",
            block.max(sky)
        ));
        // `DebugEntrySoundMood`.
        let [used, streaming] = self.sound_channels;
        extras.regular.push(format!(
            "Sounds: {used}/{} + {streaming}/{} (Mood {}%)",
            crate::audio::stats::STATIC_CHANNELS,
            crate::audio::stats::STREAMING_CHANNELS,
            (self.mood() * 100.0 + 0.5).floor() as i32,
        ));
        extras.regular.push(format!(
            "Chunks[C] W: {} E: {}",
            self.level.chunk_count(),
            self.entities.len()
        ));
        extras
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_reads_as_its_id_and_properties() {
        let (name, properties) = describe_state(Block::STONE.default_state.id.as_u16());
        assert_eq!(name, "minecraft:stone");
        assert!(properties.is_empty());
        let furnace = Block::FURNACE.default_state.id.as_u16();
        let (name, properties) = describe_state(furnace);
        assert_eq!(name, "minecraft:furnace");
        assert!(
            properties.iter().any(|p| p.starts_with("facing: ")),
            "{properties:?}"
        );
    }

    #[test]
    fn a_setblock_command_names_the_properties() {
        let command = setblock_command([1, 64, -2], Block::FURNACE.default_state.id.as_u16());
        assert!(
            command.starts_with("/setblock 1 64 -2 minecraft:furnace["),
            "{command}"
        );
        assert!(command.contains("facing=north"), "{command}");
        assert_eq!(
            setblock_command([0, 0, 0], Block::STONE.default_state.id.as_u16()),
            "/setblock 0 0 0 minecraft:stone"
        );
    }
}
