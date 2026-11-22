# Tile editor: rivers and water (plan, 2026-11-22)

Better tools for the water of a battle tile, on `feature/river_tools`. The user's request: **rivers snap into the tile's river links**, so the
terrain at the join is shaped for the connection and the river runs on into the neighbour's without a break; plus more river tools of my
proposing; the user's answers of 2026-11-22 are in Decisions and Order. Nothing here is built yet.

## What there is today

- **Rivers** are TEd's `SplineRiverInstance`: Bezier points with `width`, `foam_amount`, `alpha_fade`, `flow_speed`, `terrain_offset`,
  landscape (`terrain_offset` over the ground) or freeform; drawn from the Create panel (`edits/new_items.rs`), edited as any spline, drawn
  with the game's `rigidriver` shaders, written to `river_spline<n>.xml` on save (`wce_tile::river_files`; heights in the game's frame for
  the rivers the editor makes, `wce_river_height="game"`, docs/re/water.md).
- **River links** (Phase 4c of tile_ted_parity.md, `tile_document/river_links.rs`): `river_ends` finds the ends within half a cell of an edge,
  `river_link_problems` reports target cells without an end, ends at cells that are no target and ends more than 8 m off their cell's
  centre, and `snap_river_ends` moves the ends there (x / z only: **the height, width, direction and terrain stay**). It is a button in the
  Problems and a script command, not a snap while drawing.
- **Road links** snap live (`snap_road_links`, `road_link_points`, `tool.rs`): a road point drawn or dragged within reach goes onto the link,
  rings show while a road is drawn. The river snap should feel the same.
- **The terrain under a river** is the user's to sculpt: nothing carves a bed. Procedural roads own height and blend layers
  (`wce_tile::terrain_layers`, docs/plans/procedural_roads.md "Layers"), which is the model to follow.
- **The map context** draws the neighbours' rivers (mid ring) and their final heights, so the open tile can be compared with what it meets.
- **Lakes** are `water_plane` model instances scaled over them; placed by hand as any building.
- **Shores** (reeds and stones on the banks) went with the ground coverage, shelved 2026-11-21: they come back as this plan's last step.

## What we do not know yet (RE first, black box on the kit)

Making the join right needs the shape the game and BOB expect at a river link, which is not traced (docs/re/battle_maps.md: "the links'
`blend_size`, `no_offline_blend`, `<blend_quad/>` and the tile sets' `linking_tile` are the inputs (not traced yet)"). Measured on the kit's
river tiles (`river/*`, `river_ford/*`, `river_mouth/*`, the river settlements such as `dunedain_city_osgiliath`) and their exports:

- **R1. The river's end at a link**: for the kit's 187 river ends at an edge: the water height in the file's frame (`-6.28` in `river/*` and
  `river_ford/*`, `-8` in the eastern cities: one value per family, or per tile?), the width, the angle of the end's tangent against the
  edge's normal, how far the handle reaches. If there is one profile per link set (`river`, `river_shallow`), the snap uses it; if not, the
  neighbour decides (R4).
- **R2. The bed at a link**: the editing heights and `final_heights` across a river target cell, against a land edge's 0 m fade. Does the
  export fade a river-linked edge to 0 as a land edge (then the bed meets the LF's 0 and the water at -6 would be over dry ground), keep it
  as the sea's edges keep -20, or blend the `linking_tile` (`river\1x1_blend`) in over `blend_size` pixels? The answer fixes the cross-
  section the snap carves and whether the editor's own fade (`TileProject::faded_height_at`) needs a river case.
- **R3. Flow**: whether the spline's point order is the flow (a kit river's first point on its `is_entry` link?), what `flow_speed` does
  when the order is the other way, and how `calculate_flow`'s flip turns a placed tile's river.
- **R4. Neighbours that disagree**: on the kit's battle maps, do two river tiles that meet have ends that agree (height, width, place) to
  within what tolerance? That gives the check's thresholds and says whether "match the neighbour" is ever needed over "match the profile".

**Settled 2026-11-22** (docs/re/tile_links.md, "Rivers at their links"; `examples/river_link_profile.rs`):
- R1: two standard connections. `river`: bed -12 m, water -6.2 m in the files (-7.0 in the settlements), width 45. `river_shallow`: bed
  -1 m, water +0.42 m, width 45. The end goes through the cell's middle, square to the edge, about 6 m past it.
- R2: the export **forces** the linking tile's cross-section (`river/1x1_blend`, `river_shallow/1x1_blend`) along the link's cell,
  whatever the tile has there, and fades back to the tile's heights over about 64 m. So a bed that already has that cross-section over
  the link's cell comes out unchanged; one that does not gets a step where the export's blend meets it.
- R3: not settled. The kit's point order follows `is_entry` in the deep rivers and runs the other way in every shallow one: an in-game
  look comes before any flow check (step 3).
- R4: kit neighbours always meet in the bed (the export sees to it); their water may step 0.8 m. Tolerance for the check: 1 m.

## 1. River link snap (the user's request)

**While drawing or dragging.** As with roads: a river point drawn or dragged within reach (12 m, or the snap tolerance) of a river target
goes onto it; the river targets show as rings while a river is drawn or selected; the snap chip gets *snap rivers to river links* (on by
default); `snap rivers on|off`. The existing *Snap river ends* button stays for rivers already there, and does the full snap below.

**What a snapped end gets** (the standard connection of the link's set, R1; or the neighbour's end when the map context has one):
- its place: the cell's centre along the edge, the overshoot past it (today's `RiverEnd::snapped`);
- its **height**: the link's water height, written in the river's own frame (a river with `wce_river_height` in metres; an old kit river by
  its file's factor, so that saving does not shift its other points);
- its **direction**: the end's handle turned across the edge (the kit's angle, R1), its length kept;
- its **width** (and `terrain_offset` for a landscape river, so the water sits where the link wants it): the link's, eased into the
  river's own over the last segment rather than jumping at the end point.

**The terrain at the join.** A river owns a height layer, as a road does (not listed, not editable by hand, made again when the river or
the ground under it changes, after the user's layers). For this step it holds only the **joins**: over each snapped end's cell, the linking tile's
cross-section (R2: the export writes it there anyway), at full strength for the first 64 m in (the export's blend zone, so the export
changes nothing there) and blended into the user's ground over the next 64 m along the river. The cross-sections are tables taken from
the kit's measures (deep and shallow), not read from the kit at run time. A snapped end is marked
on its point (`wce_link="x,y"`), so the join follows if the river is moved and goes if it is unsnapped (dragged off the ring).
Step 2 grows the same layer into the whole bed.

**Contiguous with the neighbour.** In a battle map context, the check also compares each end with the neighbour's river end across the edge
(their files, as the context already reads them): gap along the edge, height step, width step. Over the thresholds (R4) it is a Problem
with *Match neighbour* (the snap to that end instead of the profile), and the seam shows a short marker on the ground. Without a context
the check is the profile alone, as now.

**The links themselves.** A river end at an edge cell that offers no river link gets a quick fix: *Add river link here* (a `river` target,
and the rule the kit's river tiles of that size have at that cell, through `links_editor.rs`), one undo entry. A target cell with no river
keeps its Problem, now with *Draw river from here* (starts a river at the snapped point).

**Done when**: on osgiliath minor and a `river/*` tile, a river drawn from edge to edge snaps both ends, the bed at both links matches the
kit's profile within 0.25 m, the check is clean, and in a battle map context the open tile's river meets the neighbours' with no visible
step; in game (Launch in game, a map with the tile between two kit river tiles), the water runs on across both seams.

## 2. Procedural river bed

A river's *Carve bed* (on for rivers the editor makes, off for the kit's, which have their beds sculpted): its height layer cuts the bed
along the whole river: depth under the water, bed width (a fraction of the water width, the kit's ratio measured: 40..80 m bed for 90..120 m
water, docs/re/water.md), bank slope (degrees, the road's `max_side` logic), a little noise on the bed, seeded. Where the ground is already
lower than the bed the layer leaves it (a river through a gorge keeps its gorge). Its blend layer paints the bed and banks with the climate's
river textures (channels picked as the road's are), feathered. The joins of step 1 are this layer's ends.

Settings as attributes of the `SplineRiverInstance` (`wce_bed_depth`, `wce_bank_slope`, ...; TEd never sees them), in the Inspector with a
preset (Creek, River, Great river: widths, depths and speeds from the kit's rivers), Advanced for every number.

## 3. Profile and flow

- **Long profile**: a panel under the Inspector with the river's bed and water heights along its length (the ground sampled under the
  centre line, the water as it is drawn). Points drag up and down there; the link ends are locked; *Make downhill* evens out every rise
  so the water never runs uphill (landscape rivers over a bumpy bed do today, and look wrong).
- **Flow arrows** on the river while it is selected (and in the links' *show on the ground*), and the check of R3: a river whose order
  runs against its `is_entry` link is a Problem with *Reverse river* (points reversed, handles swapped, per-point values kept with their
  points).
- **Ramps of the per-point values**: select points and set width / flow speed / foam / terrain offset as a ramp from the first to the
  last selected (a river that widens towards its mouth), instead of point by point.

## 4. Joining rivers

- **Tributaries**: an end dragged onto another river snaps to its centre line at its water height and turns into its flow; the beds merge
  (the lower one wins where they overlap). The game's rivers are separate ribbons, so the meeting is the two ribbons overlapping: the
  snap puts the tributary's end just inside the main river's water so no gap shows.
- **Split / join**: split a river at a point into two (one undo entry), join two rivers whose ends meet into one spline.

## 5. Lakes and the sea

- **Lake tool** (Create > Water > Lake): draw an outline, get a `water_plane` instance scaled to cover it at a height picked with a click
  on the ground or typed; *Flood preview* shows, before placing, what would be under water at that height (the water depth view's colours,
  limited to what connects to the click). An optional *Carve basin* height layer, as the river bed's.
- **River into lake / sea**: a river end dragged onto a lake's plane or the sea snaps to its level (0 m for the sea of a `river_mouth` or
  coast tile) and runs a little way under it so no gap shows.

## 6. Crossings

- **Fords**: select a span of a river and *Make ford*: the bed rises to a wading depth there (the kit fords' depth, measured), the bank
  slopes ease so units can get down, the water stays; it also says whether the tile's links want `river_shallow` instead of `river`.
- **Kit bridges**: placing a bridge building over a river snaps it across (perpendicular to the river at that point, centred, its deck
  height kept), and the platform check runs on it at once. Procedural roads already bridge rivers on their own.

## 7. Water checks (Problems)

- **Spills**: the water above the ground beside it (a bank lower than the water within the river's width plus a margin): the river would
  float or flood sideways. With the bank height there.
- **Buried**: the ground over the water along the centre: the river disappears there.
- **Steps**: a drop between two points steeper than the ribbon can show without a visible kink.
- **Old factors**: a kit river whose file factor (1.25) puts it under its bank in game (docs/re/water.md); fix: *Use real heights*
  (sets `wce_river_height` and keeps where it shows).

## Decisions

| Question | Answer |
|---|---|
| What the snap shapes the end to | The kit's standard river connection (R1/R2), **forced** at a snapped link even over ground the user sculpted (user, 2026-11-22: most tiles share a standard connection shape); the neighbour's end only when the map context has one and the user asks (*Match neighbour*) |
| Where the join's terrain lives | A height layer the river owns (as a road's), made again, never edited; the user's own layers are not touched, the river's layer applies over them |
| Carve bed by default | **Yes** for the rivers the editor makes (user, 2026-11-22); off for the kit's, which have their beds sculpted |
| Kit rivers | Never carved, never re-heighted unless a command says so; snapping a kit river keeps its file's factor |
| Live snapping | On by default, one chip with the road links' |
| Games | **Attila first**; Rome 2 once it works there (user, 2026-11-22), on `origin/feature/rome2_river_splines`'s fitted rivers |
| Lakes | **Wanted** (user, 2026-11-22: see whether a working lake tool can be made); its own RE spike first (how the kit's lakes are built, what BOB and the game do with a `water_plane`) |
| Shores | **Back as the last step**, after Rome 2 (user, 2026-11-22): reeds and stones along the carved beds, from `feature/shelved_ground_coverage`'s Shores |

## Order

1. RE R1 to R4 (and the lake spike L1 below, when step 5 comes).
2. Step 1, river link snap; the user looks at it before step 2.
3. Step 2, procedural river bed.
4. Step 7, water checks.
5. Step 3, profile and flow.
6. Step 4, joining rivers.
7. Step 6, crossings.
8. Step 5, lakes.
9. Rome 2: the same tools on Rome 2's rivers (its link sets, its river block's order of floats, its heights).
10. Shores along the rivers and lakes.

**L1. Lakes (RE spike before step 5)**: on the kit's 22 `lakes` tiles: how the basin and the `water_plane` relate (the plane's height
against the shore's ground, its scale against the basin's outline, one plane or several), what the links of a lake tile are, whether
BOB writes the plane into `hf_water_map.data` and the ground types (`GT_SHALLOW_WATER` / `GT_DEEP_WATER`) as it does for rivers, and
whether units treat it as water in game (a test lake on a scratch tile, Launch in game).

## Risks

- R2 may show that the export blends a linking tile we cannot reproduce exactly; then the join is matched to `final_heights` as measured,
  and the in-game check decides.
- A river layer on a big tile (many rivers, 1 m pixels) must remake off the editor's thread, as the roads' do, or dragging a point lags.
- Old kit rivers have files that do not follow their `tile.ted` (docs: `river_files`): snapping one writes its file, which moves the
  river in game to where the editor shows it. Expected, but worth a line in the Problem's text.
