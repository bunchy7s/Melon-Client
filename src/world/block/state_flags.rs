//! Per-block-state flags the mesher and the culling ask for every block, as

use pumpkin_data::BlockStateId;

const STATES: usize = BlockStateId::COUNT as usize;
const WORDS: usize = STATES.div_ceil(64);

/// A set of block states, one bit each.
struct StateBits([u64; WORDS]);

/// Builds a [`StateBits`] holding every state `rule` accepts. A macro, since
macro_rules! state_bits {
    ($rule:path) => {{
        let mut words = [0u64; WORDS];
        let mut id = 0;
        while id < STATES {
            if let Some(state) = BlockStateId::new(id as u16)
                && $rule(state)
            {
                words[id / 64] |= 1 << (id % 64);
            }
            id += 1;
        }
        StateBits(words)
    }};
}

impl StateBits {
    #[inline]
    fn contains(&self, id: u16) -> bool {
        let id = id as usize;
        self.0
            .get(id / 64)
            .is_some_and(|word| word >> (id % 64) & 1 != 0)
    }
}

/// Vanilla `BlockState.isSolidRender`: a full, opaque cube. It hides the
const fn rule_solid(state: BlockStateId) -> bool {
    state.is_solid_render()
}

/// Whether a state may have anything to draw: everything but air. What it
const fn rule_drawn(state: BlockStateId) -> bool {
    !state.is_air()
}

static SOLID: StateBits = state_bits!(rule_solid);
static DRAWN: StateBits = state_bits!(rule_drawn);

fn extra_state(id: u16) -> Option<&'static pumpkin_data::BlockState> {
    if id < BlockStateId::COUNT {
        return None;
    }
    BlockStateId::new(id).map(pumpkin_data::BlockStateId::to_state)
}

/// See [`rule_solid`].
#[inline]
pub fn is_solid(id: u16) -> bool {
    SOLID.contains(id) || extra_state(id).is_some_and(pumpkin_data::BlockState::is_solid_render)
}

/// See [`rule_drawn`].
#[inline]
pub fn is_drawn(id: u16) -> bool {
    DRAWN.contains(id) || extra_state(id).is_some_and(|state| !state.is_air())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_opaque_cubes_are_solid() {
        let stone = pumpkin_data::Block::STONE.default_state.id.as_u16();
        let glass = pumpkin_data::Block::GLASS.default_state.id.as_u16();
        let ice = pumpkin_data::Block::ICE.default_state.id.as_u16();
        let short_grass = pumpkin_data::Block::SHORT_GRASS.default_state.id.as_u16();
        assert!(is_solid(stone) && is_drawn(stone));
        assert!(!is_solid(0) && !is_drawn(0));
        assert!(!is_solid(glass) && is_drawn(glass));
        assert!(!is_solid(ice) && is_drawn(ice));
        assert!(!is_solid(short_grass) && is_drawn(short_grass));
        assert!(!is_solid(u16::MAX));
    }
}
