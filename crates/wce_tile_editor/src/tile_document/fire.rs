//! The burning sequence's preview: a building set on fire plays the engine's stages over time, as a battle burns it (docs/re/buildings.md,
//! "Burning, the sequence"). A view state of the tile, not an edit: the file keeps its `burn` and `burn_amount`.
//!
//! - Stage one, `BUILDING_FIRE_STAGE_ONE_DURATION` (18 s): the flames (`burning_entity` over the pieces' meshes and the pieces' `ip_fire`
//!   effects); the char (the fire shaders' burn amount, POSITION6.x) rises from 0 to `BURN_SHADER_AMOUNT` (0.3).
//! - Stage two, `BUILDING_FIRE_STAGE_TWO_DURATION` (10 s): `burning_entity_collapse`, `entity_collapse` and `burning_entity_aftermath`; the
//!   char rises from 0.3 to 1.
//! - Then the burnt building: char 1, the aftermath's smoke going on, until the preview is stopped.
//!
//! ASSUMPTION: stage two follows stage one at once (the engine starts its second timer on an event that was not traced; the screenshots
//! of a burnt longhouse fit "the fire went out"), and the collapse effects play at its start. The engine's `burning_entity_pre_smoke` (for
//! entities whose virtual says so) and `burning_entity_small` set (entity type 0xb) are not played: which buildings take them is not known.

use std::collections::HashMap;

use wce_scene::render_scene::RenderScene;

use super::Tile;

/// The engine's tweakers (empire.retail.dll, their registrations at 0x10047e00..): seconds and burn amounts.
pub const STAGE_ONE_SECONDS: f64 = 18.0;
pub const STAGE_TWO_SECONDS: f64 = 10.0;
pub const BURN_SHADER_AMOUNT: f32 = 0.3;

/// What a fire stage plays over a burning piece's mesh (stage 1: the flames; 2: the collapse and the aftermath).
pub(crate) fn stage_effects(stage: u8) -> &'static [&'static str] {
    match stage {
        1 => &["burning_entity"],
        2 => &["burning_entity_collapse", "entity_collapse", "burning_entity_aftermath"],
        _ => &[],
    }
}

/// The burn amount and the stage `t` seconds into the fire.
pub fn fire_at(t: f64) -> (f32, u8) {
    if t < STAGE_ONE_SECONDS {
        return (BURN_SHADER_AMOUNT * (t.max(0.0) / STAGE_ONE_SECONDS) as f32, 1);
    }

    let k = ((t - STAGE_ONE_SECONDS) / STAGE_TWO_SECONDS).min(1.0) as f32;
    (BURN_SHADER_AMOUNT + (1.0 - BURN_SHADER_AMOUNT) * k, 2)
}

/// The buildings on fire in the preview: when each started (the editor's clock), and what was last shown.
#[derive(Default)]
pub(crate) struct FirePreview {
    started: HashMap<usize, f64>,
    now: f64,
    shown: HashMap<usize, (f32, u8)>,
}

impl Tile {
    /// Sets a building on fire in the preview (again from the start when it burns already), at the editor's time `now`.
    pub fn fire_start(&mut self, elem: usize, now: f64) {
        self.fire.started.insert(elem, now);
        self.fire.now = now;
    }

    /// Ends a building's fire preview (all of them for `None`): the buildings look as their properties say again.
    pub fn fire_stop(&mut self, elem: Option<usize>) {
        let stopped: Vec<usize> = match elem {
            Some(e) => self.fire.started.remove(&e).map(|_| e).into_iter().collect(),
            None => self.fire.started.drain().map(|(e, _)| e).collect(),
        };

        for e in stopped {
            self.fire.shown.remove(&e);
            self.fire_refresh(e, true);
        }
    }

    /// The editor's time the preview last saw.
    pub fn fire_now(&self) -> f64 {
        self.fire.now
    }

    /// Seconds into a building's fire preview, `None` when it is not burning in the preview.
    pub fn fire_time(&self, elem: usize) -> Option<f64> {
        self.fire.started.get(&elem).map(|&s| self.fire.now - s)
    }

    /// The burn amount and stage the preview gives a building now.
    pub(crate) fn fire_look(&self, elem: usize) -> Option<(f32, u8)> {
        self.fire_time(elem).map(fire_at)
    }

    /// Per frame (the tool's update, with the editor's time): the burning buildings' char and stage; a piece is published again when its
    /// char moved, the effects when the stage changed.
    pub fn update_fire(&mut self, scene: &mut RenderScene, now: f64) {
        self.fire.now = now;
        if self.fire.started.is_empty() {
            return;
        }

        let elems: Vec<usize> = self.fire.started.keys().copied().collect();
        for e in elems {
            let Some(look) = self.fire_look(e) else { continue };
            // What was last published (not last seen: the char moves by less than the step every frame).
            let before = self.fire.shown.get(&e).copied();
            let stage_changed = before.is_none_or(|b| b.1 != look.1);
            if before.is_none_or(|b| (b.0 - look.0).abs() > 0.002) || stage_changed {
                self.fire.shown.insert(e, look);
                self.fire_refresh(e, stage_changed);
            }
        }

        self.flush_pieces(scene);
    }

    /// Publishes a building's pieces again (and its effects when `effects`).
    fn fire_refresh(&mut self, elem: usize, effects: bool) {
        let pieces = self.entity_count.get(elem).copied().map_or(0, usize::from);
        for k in 0..pieces {
            self.piece_pending.push((elem, k));
        }

        if effects {
            self.columns_element_changed(elem);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_char_follows_the_engines_stages() {
        assert_eq!(fire_at(0.0), (0.0, 1));
        assert!((fire_at(9.0).0 - 0.15).abs() < 1e-6, "halfway through stage one");
        assert_eq!(fire_at(18.0), (0.3, 2));
        assert!((fire_at(23.0).0 - 0.65).abs() < 1e-6, "halfway through stage two");
        assert_eq!(fire_at(28.0), (1.0, 2));
        assert_eq!(fire_at(100.0), (1.0, 2));
    }
}
