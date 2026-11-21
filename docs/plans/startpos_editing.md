# Startpos editing: armies, agents and settlements on the campaign map

Goal: the campaign map in the Terrain tool shows a campaign's start position as the game draws it (armies, agents, navies and
the settlements) and edits it by clicking on them: move, owner, an army's units, a settlement's buildings. Attila and TDD first;
Thrones and Rome 2 later (their startpos layouts differ).

User decisions (2026-11-21): edits go to the Assembly Kit's `start_pos_*` tables in the project, with the compiled `startpos.esf`
as the fallback for maps without tables (TDD); the game's own models from the start; first edits: move, owner, army units,
settlement buildings.

## What is known (research of 2026-11-21)

### The startpos (`CAMPAIGN_STARTPOS/CAMPAIGN_ENV/CAMPAIGN_MODEL/WORLD`)

- Characters: `FACTION_ARRAY/FACTION/CHARACTER_ARRAY/CHARACTER`, their faction the FACTION they sit under. [0] `LOCOMOTABLE`
  ([0] Coord2d world x, y; [1] / [2] hex column / row; [3] facing, pi in 574 of 578), [1] id, [2] `CHARACTER_DETAILS` ([5] faction
  key, [16] agent type), [3] type (Attila's start: 337 `general`, 240 `colonel` (garrison commanders), 1 `spy`), [4] force id.
  Off the map: (0, 0) (59 in Attila, 155 in TDD). In a settlement: on its primary slot's cell.
- Armies and navies: `FACTION/ARMY_ARRAY/MILITARY_FORCE`: [0] id, [1] commander, [6] `UNIT_CONTAINER/UNITS_ARRAY/UNIT` ([1]
  `UNIT_RECORD_KEY`[0] the unit key, [4] / [5] men now / max), [7] the residence it stands in, [19] navy. No position of its own:
  its commander's. Garrisons are forces led by a `colonel`, hung off `REGION/SETTLEMENT/GARRISON_RESIDENCE`.
- Regions: `REGION` [14] owner faction id, [31] unowned; slots `REGION_SLOT_MANAGER/.../REGION_SLOT` with `BUILDING_BASE` ([1]
  key, [4] owner faction key) when built.
- Coordinates (RPFM `coords.rs` `MapScale`): the kit's `startx` / `starty` are hex cells (1016 x 720 on `main_attila_map`); world
  x = col W / (cols - 1), y = row 8 sy + (odd col ? 4 sy : 0), sy = H / (rows 8 + 4), W x H `map_data.esf`'s theatre (678.5 x
  556.144). The same world frame as `wce_terrain_io::attila::cities`.

### The kit tables and RPFM

- `start_pos_characters` (ID, Type, faction, startx / starty, portrait_id = art set, override_general_unit, ...),
  `start_pos_land_units` (general, unit_type, soldiers %), `start_pos_naval_units`, `start_pos_settlements` (primary_building,
  port_building, building1..5), `start_pos_regions` (owning_faction), `start_pos_character_to_settlements`, `start_pos_factions`.
- RPFM's generator (`rpfm_extensions::startpos`, the user's `../rpfm`) builds the startpos from them (Attila about 0.90 of the
  fields equal to the game's); its extractor (`extract::extract_tables`) turns a startpos back into the tables (TDD's included). Garrison
  units are derived from the buildings, not stored.
- Patching a startpos directly: a move is three leaves (plus `MILITARY_FORCE`[7] / `GARRISON_RESIDENCE`[1] in and out of a
  settlement, and the AI's `CAI_SITUATED` cell); an owner change moves records between factions' arrays and touches a dozen linked
  records (provinces, ownership lists, buildings, garrisons, AI mirrors); new units need fresh global ids and AI entries; a building in
  an empty slot inserts a record. Line of sight is recomputed by the game; whether it rebuilds the AI's records is not known.
- rpfm_lib's ESF encoder keeps the content but not the bytes (other size padding, a weaker LZMA: 14 MB for 7 MB); whether the game
  loads it is untested.

### The models

- Character -> art set (`start_pos_characters.portrait_id`; agents with none: `campaign_character_art_sets` by agent type, faction,
  culture, subculture, sex) -> `campaign_character_arts` rows by level / age / season / faction leader -> `uniform` (land),
  `sea_uniform` (army at sea), `navy_uniform` (admiral) -> `agent_uniforms.filename` ->
  `variantmeshes/variantmeshdefinitions/<f>.variantmeshdefinition`. Skinned (`rome_man`, agents `rome_man_game`); idles from the
  campaign animation tables (`animations/campaign_animation_tables/`, set -> fragment -> clips: `sword_general`, `champion_male`).
  Ships the same way (`campaign_ship_civ_army` / `_fleet` / `_agent`, animated). The general stands on foot (the horse only in the
  walk); his banner is a separate 3D UI piece (`ui/3dui/models/<skin>/army_banner_*`, `ARMY_FLAG_SCALE` 0.65).
- The editor already assembles and draws skinned variant meshes for battle units (`unit_assembly.rs`, `renderer/skinned.rs`,
  `unit_animation.rs`, `Payload::SkinnedInstance`); missing: the campaign animation tables and a campaign loader.
- Scale: `CHARACTER_MODEL_SCALE` 1.0, times a per-entity factor not traced (and 0.4 in some settlement case); check against the
  user's screenshots before guessing.

### The Terrain tool

- Picking: an ID pass (meshes, terrain) and overlays; skinned meshes are not in the ID pass (the Tile tool picks its reference
  soldiers with spheres). Settlement models are added locked with no item (`prop_models.rs`), so not pickable.
- Editing: `Document::inspect` / `ImGuiInspector`, `Command`s on `MapDocument`'s stack (`TerryAttrCommand` is the model), the
  ImGuizmo gizmo through `item_transform` / `transform_items`.
- Saving: nothing writes kit db XML or a startpos yet.

## Phases

1. **The start as a model** (`wce_terrain_io::attila::startpos`, new): `Start { factions, characters (id, type, faction, cell,
   art set, force), forces (id, commander, navy, units), settlements (region, owner, slots and buildings) }`, read from the kit
   tables (the project's, then the kit's: the data source order) when the campaign has them, else from the startpos (packs, then the
   game's `data`). One model for both, so everything after it does not care where it came from. Checked: Attila's tables and its
   startpos give the same characters, cells and units (less the generated garrisons).
2. **Drawn with the game's models**: a campaign character loader (art set -> arts row -> uniform -> variant mesh; the campaign
   animation tables for the idle; ships for navies and armies at sea), placed at the cell's world point on the ground, facing pi.
   Garrisons and characters inside settlements not drawn (the game doesn't). Scale and the banner checked against the user's
   screenshots (Documents, `325610_*.png`); the banner after the figures.
3. **Clicking**: characters picked by a sphere or box around each figure (as the Tile tool's soldiers), settlements by their
   models (pickable, an item id per settlement); a selection item that says which (character id / region key).
4. **Editing** (undoable commands on `MapDocument`'s stack, the view rebuilt from the model):
   - move: the gizmo or a drag, snapped to hex cells, onto land for armies and agents (sea for navies); into a settlement when
     dropped on one;
   - owner: a faction list (a character takes its force along; a settlement its region);
   - an army's units: list, add (the units the faction can recruit, by `units_to_groupings_military_permissions` or all), remove,
     strength;
   - a settlement's buildings: per slot, the buildings its slot type allows; the drawn buildings and roads follow.
5. **Saving**: the kit tables written to the project (`raw_data/db/start_pos_*.xml`, the kit's own XML layout, only the tables
   that changed, the rest untouched). The startpos fallback (TDD): see the open question below.
6. **Export** (later): a startpos for the game (RPFM's generator, or the patched startpos) into the mod pack.

## Open questions for the user

1. **TDD's fallback.** Two ways: (a) patch TDD's startpos directly: moves are easy, unit and building changes mean new ids and
   inserted records, owner changes touch a dozen linked records and the AI's mirrors (unknown whether the game rebuilds them); or
   (b) extract TDD's startpos into kit tables in the project once (RPFM's extractor), edit the tables like Attila's, and generate
   the startpos on export (RPFM's generator: about 0.90 of the fields equal for Attila, so the rest of TDD's startpos would come out
   as the generator builds it, not as TDD shipped it). Recommendation: (a) for moves, units and buildings, owner changes later;
   (b) when the generator's gaps close.
2. **rpfm_extensions as a dependency** (for the extractor / generator and `MapScale`): it lives in the user's `../rpfm` branch
   `startpos-thrones-rome2`, not on rpfm's `develop`. Use it (path or git branch dependency), or reimplement what phase 1 needs here?
3. **Export's startpos**: generate it from the tables with RPFM, or leave it to the user's RPFM?
