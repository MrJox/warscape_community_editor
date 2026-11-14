//! Fixes > Light wall torches: the window's words and the two steps of `wce_tile_editor::building_effects`.

use std::path::Path;

use wce_editor::fixes::{Fix, FixSource};
use wce_tile_editor::building_effects::{LOGIC_PACK, VFX_PACK, build, install, uninstall};
use wce_vfs::{Config, Vfs};

const EXPLANATION: &str = "Attila ignores the effects in a standing building's .meta file. That is why the torches on fort walls never burn in \
game even though their models are there, and why the Carthage lighthouse has no fire. Rome II plays the same files.

This builds a fix from the game and the mods ticked in the game list's Mod packs (at the toolbar's left), so the walls of those mods are covered too. In it:

- Each wall piece gets the flames of its torches in its logic file, in a list the game does play. They belong to the wall, so they go out as \
soon as it starts to break.

- The lighthouse gets its fire, at the normal size: that list has no scale.

- Posts that were left inside the step walls are moved up so their flames show.

Two packs go into the game's data folder:

wce_lit_buildings.pack, a mod pack with the changed logic files. It is ticked in the game list's Mod packs (at the toolbar's left) for you, which is what the editor's preview \
and its launches use. To play with it otherwise, enable it in the game's launcher.

wce_lit_buildings_vfx.pack, a movie pack with one copy of the torch effect for each torch on a piece: torches sharing an effect would share \
one flame. The game reads effects before it loads mod packs, so these have to be in a movie pack, and movie packs load on every start of the \
game, enabled or not. Alone it changes nothing you can see.

A torch placed on its own stays dark, as it uses the same files as the ones on walls: giving it a flame would leave one floating when a wall \
falls. Build again after changing your mod packs. Remove deletes both packs and unticks them.";

pub(crate) struct BuildingEffectsFix;

impl Fix for BuildingEffectsFix {
    fn id(&self) -> &str {
        "building_effects"
    }

    fn title(&self) -> &str {
        "Light wall torches"
    }

    fn for_game(&self, game: &str) -> bool {
        game == wce_foundation::game::GameProfile::ATTILA.key
    }

    fn explanation(&self) -> &str {
        EXPLANATION
    }

    fn packs(&self) -> Vec<String> {
        vec![LOGIC_PACK.to_owned(), VFX_PACK.to_owned()]
    }

    fn installed(&self, data: &Path) -> bool {
        data.join(LOGIC_PACK).is_file()
    }

    fn apply(&self, source: &FixSource) -> Result<String, String> {
        let mut config = Config::new(source.game_dir.clone(), source.profile.clone());
        config.use_mods = false;
        config.mod_packs = Some(source.mods.clone());
        let files = Vfs::mount(&config).map_err(|e| e.to_string())?;
        let built = build(&files)?;
        let (logic, raised, copies, skipped) = (built.logic.len(), built.raised, built.effects.len(), built.skipped.clone());
        install(&source.game_dir.join(source.profile.data_dir), built)?;

        let mut done = format!("Installed: {logic} logic files changed, {raised} posts moved up, {copies} copies of an effect.");
        if !skipped.is_empty() {
            let list = skipped.iter().take(8).map(String::as_str).collect::<Vec<_>>().join("\n");
            done += &format!("\n\n{} logic file(s) left as they were, as they could not be written back:\n{list}", skipped.len());
        }

        Ok(done)
    }

    fn remove(&self, data: &Path) -> Result<String, String> {
        uninstall(data)?;
        Ok("Removed: both packs are gone from the data folder.".to_owned())
    }
}
