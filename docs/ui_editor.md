# UI editor (the UI tool, `wce-ui`, the MCP server)

Total War UI layouts (the extension-less files under `ui/`, plus `.cml`, `.fc`, `.twui.images`) are binary files. The UI tool opens them from
the game packs, shows them as XML and as a picture you edit with the mouse, and saves them back. `wce-ui` does the same from the command line
and as an MCP server, so a coding agent can edit a layout and look at the result.

Crates: `wce_ui_format` (binary <-> XML), `wce_ui_model` (the edit model, the draw list, a CPU renderer, writing into a pack), `wce_ui_tool`
(the editor tool), `wce_ui_cli` (the `wce-ui` binary). The converter is a port of taw/etwng's `ui` converter (MIT, `third_party/etwng/LICENSE`).

## The XML

One element per primitive of the file, in file order: `<u>`, `<i>`, `<s>`, `<unicode>`, `<flt>`, `<byte>`, `<yes />` / `<no />`, `<uuid>`, `<data>`,
under structure tags (`<uientry>`, `<states>`, `<state>`, `<images>`, `<image_uses>`, `<children>`, ...). Arrays carry `count`. Comments name the
fields as far as the format is understood (`<!-- x offset -->`); they are ignored on conversion and are not needed in hand-written XML. Editing
rules that keep the file valid:

* keep the order and number of primitives of an element (the file has no field names; position is the meaning);
* `count` is corrected from the items when the layout is saved or converted, so adding or removing an item needs no count edit;
* ids (`<u>` with an `ID` comment) tie an image use to its image and an entry to its default state: duplicate with the tool (new ids) rather than
  by hand.

Round trip: an untouched file converts to XML and back to the same bytes. Gate: `cargo run --release -p wce_ui_format --example ui_roundtrip --
--game <game folder>` (225 of 225 identical in Attila's packs, versions 002 to 097). `cargo run --release -p wce_ui_model --example model_check --
--game <game folder>` checks the tree printing the same text, the entries, and the edit operations on every layout.

Games: the converter has the layout logic of every version etwng reads (Empire to Three Kingdoms, 025 and up), but only Attila's versions
(and the `.cml` 002) could be checked here. Other games' files go through the same code; a version that fails reports its offset.

## The tool

`UI` in the tool switcher (Ctrl+3). Panels:

* **UI Canvas**: the layout drawn from its default states: pictures from the game's packs (mod packs over vanilla; a picture that is not found is a
  magenta outline), text with the editor's font. Click selects the topmost component under the cursor, drag moves, the eight handles resize, the
  wheel zooms, middle mouse or Space+drag pans, arrows nudge (Shift: 10), Delete deletes, Ctrl+D duplicates. Outlines / Pictures / Text toggles.
* **UI XML**: the XML of the selected component (or the whole file). Typing applies when you pause for a third of a second, if the XML parses and
  converts to a layout; otherwise the message says why and the canvas keeps the last good state. Edits made on the canvas show here at once.
* **UI Fields**: every primitive of the component (and of its default state) with the name the format gives it; Enter or leaving the field commits.
* **Outliner** (the tree of components, with the click selecting on the canvas), **Inspector** (name, offsets, size, text, text colour), **History**.

Everything is one model with one undo stack, so the canvas, the XML and the fields cannot disagree. Menu `UI`: add child, duplicate, delete, paint
order, move into the previous sibling / out of the parent (the place on screen is kept), save into a mod pack, export XML.

Saving: File > Save writes the binary layout as a loose file under the output folder (`output_dir` in wce.ini) at its in-pack path, or in place for
a file opened from disk. `UI > Save into a mod pack...` writes it into a `.pack` (created if missing). The game's own packs are never written.

How a layout is drawn (checked against a game screenshot of the Custom Battle screen):

* **Pictures with margins are cut in nine**: the four corners keep their size, the edges and the middle stretch, or repeat when the image use is
  `tiled` (the title plaques). The margins are the last floats of the image use (top, right, bottom, left).
* **Visibility**: the seventh of the twelve flags after a component's offsets is "visible when the layout loads". Tab pages, unit cards and timers the
  game shows later have it off; a hidden component hides everything below it (toolbar: *Honour hidden*). Components whose names end in `template`
  are filled in by the game's code (list items, player slots) and are skipped too (*Skip templates*). Both can be overridden per component
  (Fields panel: *Shown in preview*; the file is not changed).
* **States** belong to single components: the file has no state that applies to the whole layout (each component lists its own states, one of
  which is its default, and the game's code switches a component by the state's name). So there is no global state dropdown: select a component
  and pick its state in the Fields panel (*Preview state*). The command line can still draw "every component that has a state called X"
  (`--state hover`), which is only a shortcut for agents.
* **Docking and sizes** follow the game's own rules, ported from `TWUI::Component::Dock` / `Resize` and `ComponentState::DockImages*`:
  * A component with a dock point (the `docking?` value after the tooltip: 1..9 = top left, top, top right, left, centre, right, bottom left,
    bottom, bottom right of the parent; 10..18 the same but outside it) is placed at that point of its parent plus the dock offset
    (`docking x?`, `docking y?`). **The stored offset is ignored for it**: the game does this when the component loads. Where the stored offset
    differs from the dock result (the layout editor does not always update it) the canvas shows the dock result, and `wce-ui tree` prints
    `(stored x,y)`. Dragging a docked component changes its dock offset; changing its size keeps the dock offset and moves it as its dock point
    says. On the Custom Battle screen 192 of 213 docked components already sit where the rule puts them (the rest differ by a pixel or by stale
    offsets); across Attila's 224 layouts 4309 of 5747 do (`layout_check`).
  * Toolbar *Screen* (or `--screen 1920x1080`): sizes the root to a screen the way the game does (`SizeToScreen`: only on the axes the root
    allows, its children keep their sizes), and every docked component is placed again. Undocked components stay where they are. It is a
    preview: the file does not change. (How the game decides to size a given layout's root is not something the layout says; the default is the
    layout's own size.)
  * A picture inside a component can be docked to the component's edges too (`dock position` of the image use) and stretches with the component
    when its stretch flags say so.
* **Related layouts** (toolbar *Related*, the *UI Related* panel, `ui_related`, `ui_render related`): layouts that are used together with this
  one, drawn faded and not editable, as reference. The layout format does **not** store which layout the game loads into which (code and scripts
  do that), so the link is found by name: a layout whose file name is a component of this one is a *child* (drawn at that component), and a
  layout that has a component named like this file is a *parent* (this layout is drawn at that component). It is a guess: names such as `tooltip`,
  `background` or `main` are marked common and left out unless asked, and the panel lets you switch each one off.
* **Animations** (`funcs` of a component; Fields panel *Animation*, `ui_animations`, `ui_render animation time`): the player is ported from
  `TWUI::ComponentAnimation` and `ComponentAnimationFrame`. An animation is a list of frames that play one after another; each goes from where the
  last ended to its own target in its time (milliseconds) along its easing curve (linear, quadratic, cubic, exponential, circular; in, out, in and
  out; with a weight). A frame moves only what its mask says: colour, position (relative to where the component was, or absolute), size,
  opacity, text colour (shader values, rotation and picture cross-fade are in the file but not drawn). When a frame ends its triggers set a
  state of a component or start another animation, which the preview follows; with *propagate* the colour, opacity and size reach the descendants.
  Play, pause, loop and scrub in the Fields panel; the layout file is never changed by playing. Edit frames in the XML (`anim` elements under
  `funcs`/`func`/`anims`). All 851 animations (1335 frames) of Attila's layouts parse and play (`model_check`).
* **Skins**: toolbar *Skin* lists the folders under `ui/skins/`; pictures named `ui/skins/default/x.png` are taken from `ui/skins/<skin>/x.png` when it exists
  and from the default skin otherwise. (The factions table's `ui_skin` column names the folder a faction uses.)

* **Text uses the game's own fonts**: the bitmap fonts `font/<name>_<size>.cuf` (format and layout ported from the engine's `SPRITE_FONT`: glyph
  bitmaps with a top and advance, the kerning table, word wrap, tab stops). A state's font category (`ui/fontcategories.fc`: `ingame_text`,
  `fe_button_text`, ...) decides font, size and colour; `Default Font Category` uses the state's own font, size and colour. Alignment: centred
  when the first text value of the state is 4, else the third value (0 left, 1 centre, 2 right); vertically centred when the second is 4. The pictures
  are the engine's alpha bitmaps drawn with plain alpha blending; the engine's own UI shaders are not needed for this.

## Changing the structure: the UI Layers panel

The *UI Layers* tab (next to Outliner and Assets) is the layout's tree with the buttons for changing it:

* **Add** makes a new component inside the selected one (inside the root when nothing is selected) and lets you type its name at once.
  **Duplicate** copies the selected component with everything inside it; **Delete** removes it with everything inside it.
* **Up / Down** change the order among its siblings (a component is drawn over the ones above it in the list).
  **In** makes it a child of the sibling above it; **Out** makes it a sibling of its parent. A component keeps its place on screen when it changes parent.
* **Drag a row onto another row**: the middle makes it a child (the last one), the top or bottom edge puts it before or after, with a line showing where.
* **Double-click** a row (or right-click > Rename) renames it; right-click also has Add child, Duplicate and Delete. All of it is undoable (Ctrl+Z).

The same operations are in the UI menu, on the keyboard (Del, Ctrl+D) and for scripts and agents (`ui_add`, `ui_dup`, `ui_delete`, `ui_move_to <component> <target> before|after|inside`,
`ui_rename`).

## Properties, in the game's words

The Fields panel (tabs *Component* and *State*) names every field the way the game's own code does and gives it a control that fits:
drop-downs for modes (dock point, moveable, text alignment, text wrapping, picture draw mode, mouse event), check boxes for yes/no fields and
for the bits of flag words (a state's *Interactive*, *Disabled*, *Pixel-accurate hit test* and *Dock pictures as a nine-patch* share one integer in
the file; a mouse action's fade mask is eight check boxes), a colour picker for text and tint colours, a drop-down of the component's own
states for *Default state* and *Go to state*, and of its own pictures for a picture use. Fonts, shaders, font categories and sound categories
offer the values Attila's layouts use next to a free text box. Hover a name for what it does and the converter's old name for it.
*Advanced* shows the rarely needed ones; the whole file, field by field, is in the UI XML panel. Numbers are drag boxes (drag to change, Ctrl+click
to type) with - and + buttons; a drag is one undo step.

## Making a skin of your own

A skin is a folder of pictures under `ui/skins/<name>`; a component's picture `ui/skins/default/x.png` is taken from `ui/skins/<name>/x.png` when
the skin has one, else from the default skin.

* **New skin**: *UI > New skin...* (or the button beside the Skin list) asks for a name and makes the folder on disk (the first time also the
  working folder, `Documents\WarscapeCommunityEditor\ui_skins_project`). The skin is selected at once. Put PNGs in it, named like the game's pictures; the
  preview updates within a second of each save. *Open skin folder* (next to the list, for loose skins) opens the folder in the file explorer.
* **In the Skin list**: `[loose]` is a skin of your own, a folder on disk; `[loose, overrides game packs]` is one whose name the game's packs also have.
  The order the editor reads pictures in is always: your loose folder, then the mod packs you chose, then the game's release packs. So a loose
  picture beats a pack's picture of the same name, a picture you did not provide comes from the pack's skin of that name, and anything missing
  from both comes from the default skin. If a pack that contains your skin is selected later, your loose files still win in the editor (in the game
  the pack's own priority decides).
* **Layouts**: Ctrl+S saves a layout into the working folder under its game path; that version opens next time. A layout file that changes on disk
  reloads by itself (as an undo step) unless you have unsaved changes in it; the status line says so then.
* **Export**: *UI > Export mod pack...* saves layouts with unsaved changes into the folder, then writes every file of it (all your skins and layouts)
  into a new `.pack` to share. The game's own packs are never written.

## Which packs are read

Only the game's release packs (the extra packs and those in `manifest.txt`) and the mod packs you choose: at first start, or later in
*File > Mod packs...*. The launcher's list of enabled mods is not used any more. The choice is `mod_packs=a.pack;b.pack` in `wce.ini`
(`WCE_MOD_PACKS` for the command line). Advanced: `overlay_folders=` in `wce.ini` (or `WCE_OVERLAY`) adds more read-only loose folders; scripts use
`ui_new_skin <name>`, `ui_project <folder>` and `ui_export <pack>`.

Where the names come from: `TWUI::Component`, `ComponentState` and `ComponentImageMetrics` (members and enums in `Attila.h`) and the loaders in the
decompiled executable. Values were checked against all 225 layouts (`props_check`). Corrections to the converter's guesses:

* `title2` is the *callback name*; `uientry flag 3`-`6` are one 32-bit *moveable* value (0 none, 1 XP style, 2 Windows 7 style).
* A state's `text xsize?`/`text ysize?` are the *horizontal and vertical alignment* (2 left, 3 right, 4 centre / 0 top, 1 bottom, 4 centre; anything
  else is left or top), `text xalign?` is the *focus type*, `text yalign?` the *text wrapping* (split by character, by word, never, shrink to fit).
  The old `text behavior?` is the low byte of the vertical behaviour (*shrink font to fit*), `state stuff 4?` is *text is localised*,
  `localization id` and `tooltip id` are the text and tooltip localisation keys.
* The unnamed integer after the text margins is the flag word described above; the two integers after the pictures are the editor-only position.
* `end of uientry 1?`/`2?` are the *sound category* and *layout transition*; `stretch x?`/`y?` of a picture are *stretch with width/height*.
* The text canvas now follows the game for alignment (it used a different field for right alignment).

Not decoded (shown as *Unknown*): the extra values some versions append to a component, and the v97 controller-focus data. Text group, focus group and model
data are shown as read-only *Has ...* flags; edit them in the XML.

What is not shown: hover/click behaviour, scripts, list and table layouts (the game arranges the items of lists, which are mostly created at runtime),
rotation and shader values, and whatever the game's code creates at runtime (the player cards, the unit grid rows, the map picture).

Scripted use (see `scripts/ui_check.sh`): `ui_open`, `ui_select`, `ui_set`, `ui_add`, `ui_dup`, `ui_delete`, `ui_order`, `ui_into_previous`,
`ui_prop`, `ui_assert_prop`, `ui_move_out`, `ui_set_xml`, `ui_save_xml`, `ui_save_pack`, `ui_zoom`, `ui_fit`, `ui_outlines`, `ui_skin`, `ui_state`, `ui_screen`, `ui_related`, `ui_animate`, `ui_show_hidden`, `ui_show_templates`, `ui_preview` and `ui_assert_*` (`ui_assert_screen_rect`, `ui_assert_related`). Scripted mouse positions for this
tool are window pixels.

## wce-ui (command line)

```
wce-ui files --filter settlement
wce-ui to-xml "ui/campaign ui/settlement_panel" --out settlement.xml
wce-ui from-xml settlement.xml --out settlement.bin
wce-ui tree "ui/campaign ui/settlement_panel"
wce-ui render "ui/frontend ui/battle_setup" --out shot.png --skin rohan --show_hidden
wce-ui render "ui/frontend ui/battle_setup" --out shot.png --screen 1920x1080 --related      # docking at a screen size, with reference layouts
wce-ui related "ui/frontend ui/battle_setup"
wce-ui animations "ui/campaign ui/popup_game_saved" --entry 1
wce-ui render "ui/campaign ui/popup_game_saved" --animation OnCreate --animation_entry 1 --time 300 --out fade.png
wce-ui skins                         # the folders under ui/skins
wce-ui states "ui/frontend ui/battle_setup"
wce-ui set_entry mylayout.bin --entry 3 --x 10 --y 20 --save            # edits a file on disk in place
wce-ui set_entry "ui/campaign ui/settlement_panel" --entry 3 --x 10 --save --pack my_mod.pack
wce-ui script edits.txt            # several tools in one session
wce-ui tools                       # every tool and its arguments
```

The game folder comes from `--game`, else `wce.ini` (`game_dir`, the editor's setting), `WCE_GAME`, or `ATTILA_DIR`. Packs are read in process
through rpfm_lib (the same reader as the editor); no rpfm_server is needed. A component is named by its number in `tree`, by a path
(`root/panel/button`) or by a unique name.

## MCP

`wce-ui mcp` serves the same tools (`ui_files`, `ui_open`, `ui_tree`, `ui_entry`, `ui_state`, `ui_xml`, `ui_set_xml`, `ui_set_entry`, `ui_set_field`,
`ui_set_property`, `ui_add`, `ui_duplicate`, `ui_delete`, `ui_move`, `ui_reparent`, `ui_render`, `ui_validate`, `ui_save`, `ui_close`, `ui_list_open`, `ui_skins`, `ui_states`, `ui_related`, `ui_animations`) over stdio. `ui_render`
returns a PNG. The attila-toolkit plugin starts it (`.mcp.json`, server `wce-ui`; the executable is found through the plugin setting `wce_ui_path`,
the `WCE_UI` variable, the plugin's `bin` folder or PATH).
