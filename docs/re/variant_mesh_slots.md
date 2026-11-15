# Variant mesh definitions: slot names (spike, 2026-11-15)

Whether a `SLOT`'s `name` is free or one of a fixed set. Method: every definition of Attila's packs counted (vanilla and with TDD; the Unit
tool's example `slot_kinds names`, lists in `~/wce_re/scratch/slot_kinds`), then the names looked for in `empire.retail.dll` (its strings,
and the code that loads their addresses: `objdump -d` around each reference). Addresses are VAs of the DLL (image base 0x10000000).

## The data: names are labels

- Vanilla Attila: 140 names as written (128 told apart without case), over 8,507 files... with TDD 488. Most are the parts they hold
  (`helmets`, `cloaks`, `boss`, `trousers`, `hair`, `skin`), others one file's own (`upper_arm_jewellery_left`, `first_half`, `iron`),
  some not names at all: 96 slots in 12 files have none, six are `1` to `6`, two `armour top` / `armour skirt` (a space), one `helmets2`.
- `base` is the root slot of 2,580 unit definitions (one each): a convention, not seen in the code.

So a name is free: the definitions show the game reads any, empty included.

## Names the engine looks up

Three places in the DLL ask a model for a slot **by name** through the same call (the object's virtual at +0x8, with the name as a string):

- **A soldier's weapons** (battle model; the strings sit by `SquadTaskReform.cpp`'s): at 0x10423862 a table pairs `shield` with 0 and
  `weapon_1` to `weapon_4` with 1 to 4; a loop asks for each slot and keeps what it holds in a five-entry array of the object (count at
  +0xa0, 0x20 bytes an entry). What the battle does with them (showing, hiding, swapping a figure's weapons and shield) is **not traced**.
  The data agrees: in vanilla `shield` and `weapon_1` to `weapon_4` are each in about 2,750 files, all of the soldiers' root definitions.
- **A ship's damage states**: at 0x1023da3b the name of a piece's state is made from `%S_dm%d` (the piece, then its state number:
  `bow_01_dm2`), the folder `VariantMeshes/VariantMeshDefinitions/` next to it. The editor's assembler already shows a ship intact by these
  (`_dm1`; `_dm2` and higher are damaged: `unit_assembly::damaged`).
- **Two ship slots by their own names**: `misc_99_dm1` (0x1023df55) and `night_lights_dm1` (0x1023e02e). What they are for is not traced
  (night lights: by the name, shown at night, **ASSUMPTION**).

Not known: whether the lookup tells case apart (every vanilla spelling is lower case) and what happens with two slots of one name. TDD has
`shield 1`, `weapon`, `weapon_01`, `weapon_02` and `weapon_5`: if the lookup is exact, the battle doesn't find those as a shield or a weapon.

The DLL also writes definitions (`<SLOT name="`, `probability ="`, `attach_point="`, `use_different_attach_point_parts="true"`) and names
slots `%S__Editable%d`, with the message "Definition %S has %u variants in slot %S": a variant editor of CA's own, not looked at further.

## In the Unit tool

The slot name field lists the names the game uses (most used first) and says when a name is one the engine looks up; Problems warns of a
name that is nearly one of the soldiers' (`weapon_01`, `weapon_5`, `Shield`, `shield 1`).
