//! Pictures of what the Unit tool lets you pick: a model, a definition, a texture, or an alternative of a slot of the open file. Models,
//! definitions and alternatives are rendered by the editor's thumbnail renderer (`wce_editor::thumbnails`, the Create panel's), the
//! parts alone in the previewed unit's standing pose and colours (frame 0 of its loop), framed by their triangles' bounds, and kept in the
//! cache folder: a thumbnail's name is what it shows (models, decals, tints, masks), the skeleton and the colours, so an edit only makes
//! new ones for what it changed. Textures are their own picture, a small mip decoded on the CPU.
//!
//! The pickers built on them: a grid of cells (picture and name), and a browse button whose popup shows the packs' files as a grid or a
//! list, filtered.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::images::ImageRef;
use wce_editor::thumbnails::{Thumbnails, cache_dir};
use wce_editor::{icons, theme};
use wce_formats::variant_mesh::NodePath;
use wce_foundation::math::{Float3, scaling};
use wce_render::unit_assembly::{AssembledPart, Attach, Pins, SlotPin, assemble, assemble_with, decals};
use wce_scene::render_scene::{Payload, SKINNED_NO_OFFSET, SkinnedInstance, SkinnedPart};

use super::{UnitTool, document, file_name};

/// A cell's picture, pixels.
pub(super) const CELL: f32 = 96.0;

/// Thumbnails asked for in a frame at most: each needs its parts assembled and their triangles' bounds.
const REQUESTS_A_FRAME: u32 = 2;

/// Textures decoded in a frame at most.
const TEXTURES_A_FRAME: u32 = 4;

/// What a picture shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Thing {
    Model(String),
    Definition(String),

    /// A texture, or the first of a `.texture_array` (the name without its extension, as decals name them).
    Texture(String),

    /// An alternative of a slot of the open file.
    Alternative(NodePath),
}

/// The kind of files a browse button lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Browse {
    Model,
    Definition,
    Texture,
}

impl Browse {
    fn thing(self, path: &str) -> Thing {
        match self {
            Browse::Model => Thing::Model(path.to_owned()),
            Browse::Definition => Thing::Definition(path.to_owned()),
            Browse::Texture => Thing::Texture(path.to_owned()),
        }
    }
}

#[derive(Default)]
pub(super) struct ThumbState {
    thumbs: Thumbnails,

    /// Pictures on the GPU, by thumbnail id or texture key.
    images: HashMap<String, ImageRef>,

    /// An alternative's thumbnail id as of a document version (assembling it is not free).
    alternatives: HashMap<NodePath, (u64, Option<String>)>,

    /// Textures with no picture (a format the decoder doesn't read, a file gone).
    no_texture: HashSet<String>,
    requests: u32,
    textures: u32,

    /// The browse popups and the Unit files and Units panels show pictures (else lists).
    pub browse_grid: bool,
    pub files_grid: bool,
    pub units_grid: bool,
    filters: HashMap<String, String>,
}

impl ThumbState {
    /// The browse popups start as grids of pictures; the panels as lists.
    pub fn new() -> Self {
        Self { browse_grid: true, ..Self::default() }
    }
}

/// A toggle between a list and a grid of pictures, as an icon button: true when it was clicked.
pub(super) fn grid_toggle(id: &str, grid: &mut bool) -> bool {
    let clicked = gui::small_button(&format!("{}##{id}", if *grid { icons::LIST_TREE } else { icons::LAYOUT_GRID }));
    if clicked {
        *grid = !*grid;
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(if *grid { "Show a list" } else { "Show pictures" });
    }

    clicked
}

/// The pose and colours the pictures are made in.
struct Kit {
    key: String,
    palette: Arc<Vec<[f32; 12]>>,
    bones: Arc<Vec<[f32; 12]>>,
    colours: [[f32; 3]; 4],
}

fn hash_of(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// A name cut to fit a width, with an ellipsis.
fn fit(name: &str, width: f32) -> String {
    let mut n = name.to_owned();
    while n.chars().count() > 3 && gui::calc_text_size(&n).x > width {
        n.pop();
        n.pop();
        n.push('…');
    }

    n
}

impl UnitTool {
    /// Pictures are being made.
    pub(super) fn pics_busy(&self) -> bool {
        self.pics.thumbs.busy()
    }

    /// Per frame: the renderer's work, and the cache folder of the game.
    pub(super) fn update_thumbs(&mut self, e: &mut Editor) {
        let dir = cache_dir(e);
        if self.pics.thumbs.dir != dir {
            self.pics.thumbs = Thumbnails::default();
            self.pics.thumbs.dir = dir;
            self.pics.images.clear();
        }

        self.pics.requests = 0;
        self.pics.textures = 0;
        self.pics.thumbs.update(e);
    }

    fn kit(&self) -> Option<Kit> {
        let model = self.models.first()?;
        let figure = if self.dressed_mount { model.mount.as_ref()? } else { &model.man };
        let body = model
            .instances(-model.phase, false)
            .into_iter()
            .find(|i| i.mount == self.dressed_mount && i.sources.iter().any(Option::is_some))?;
        let colours = self.colours.map_or(model.faction_colours, |c| [c[0], c[1], c[2], model.faction_colours[3]]);
        let key = format!("front|{}|{:?}", figure.rig.skeleton.skeleton, colours.map(|c| c.map(|v| (v * 255.0).round() as u8)));
        Some(Kit { key, palette: body.instance.palette, bones: body.instance.bones, colours })
    }

    /// Asks for the picture of parts (in the kit's pose), under `id`.
    fn request_parts(&mut self, e: &Editor, id: &str, parts: &[AssembledPart], kit: &Kit, first: bool) {
        let Some(model) = self.models.first() else { return };
        let figure = if self.dressed_mount { model.mount.as_ref() } else { Some(&model.man) };
        let Some(figure) = figure else { return };
        let skinned: Vec<SkinnedPart> = parts
            .iter()
            .map(|p| {
                let (bone, offset) = match &p.attach {
                    Attach::None => (None, SKINNED_NO_OFFSET),
                    Attach::Bone(name) => (figure.rig.bone(name), SKINNED_NO_OFFSET),
                    Attach::Point { bone, matrix } => (Some(*bone), *matrix),
                };
                SkinnedPart {
                    model: p.model.clone(),
                    tint: p.tint,
                    decal_diffuse: p.decal_diffuse.clone(),
                    decal_normal: p.decal_normal.clone(),
                    masks: p.masks,
                    bone,
                    offset,
                }
            })
            .collect();
        let world = scaling(1.0 / e.scene.game_units.max(1e-3));
        let mut lo = Float3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut hi = Float3::new(f32::MIN, f32::MIN, f32::MIN);
        for part in &skinned {
            let Some(g) = self.geometry_of(e, &part.model) else { continue };
            for t in g.triangles(part, &world, &kit.palette, &kit.bones) {
                for p in t {
                    lo = Float3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                    hi = Float3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
                }
            }
        }

        let instance = SkinnedInstance {
            parts: skinned,
            palette: kit.palette.clone(),
            bones: kit.bones.clone(),
            casts_shadow: true,
            faction_colours: kit.colours,
            faction_scale: 0.0,
        };
        let parts = if lo.x > hi.x { vec![] } else { vec![(world, Payload::SkinnedInstance(instance))] };
        self.pics.thumbs.request_framed(id, parts, Some((lo, hi)), first);
    }

    /// An alternative of the open file, alone: what its slot shows when it is picked, the slots above it showing the way to it.
    fn alternative_parts(&self, e: &mut Editor, path: &NodePath) -> Option<Vec<AssembledPart>> {
        let vfs = e.vfs().cloned()?;
        let d = document(e)?;
        let tree = d.tree();
        let mut pins = Pins::default();
        for k in (0..path.len()).step_by(2) {
            if k + 1 < path.len() {
                pins.slots.insert(path[..=k].to_vec(), SlotPin::Alternative(path[k + 1]));
            }
        }

        let read = |p: &str| vfs.read(p).ok();
        let mut unit = assemble_with(&read, &tree, 1, &pins);
        unit.parts.retain(|p| p.node.as_ref().is_some_and(|n| n.starts_with(path)));
        Some(unit.parts)
    }

    /// A thing's picture when it is ready; asked for when it isn't (`first`: before the others).
    pub(super) fn picture(&mut self, e: &mut Editor, thing: &Thing, first: bool) -> Option<ImageRef> {
        if let Thing::Texture(name) = thing {
            return self.texture_picture(e, name);
        }

        let kit = self.kit()?;
        let id = match thing {
            Thing::Model(p) => format!("unit|model|{}|{p}", kit.key),
            Thing::Definition(p) => format!("unit|definition|{}|{p}", kit.key),
            Thing::Alternative(path) => {
                let version = document(e).map(|d| d.version())?;
                match self.pics.alternatives.get(path) {
                    Some((v, id)) if *v == version => id.clone()?,
                    _ => {
                        if self.pics.requests >= REQUESTS_A_FRAME {
                            return None;
                        }

                        self.pics.requests += 1;
                        let parts = self.alternative_parts(e, path).unwrap_or_default();
                        let shown: Vec<String> = parts.iter().map(|p| format!("{:?}", AssembledPart { node: None, ..p.clone() })).collect();
                        let id = (!parts.is_empty()).then(|| format!("unit|alternative|{}|{:016x}", kit.key, hash_of(&shown.join("|"))));
                        self.pics.alternatives.insert(path.clone(), (version, id.clone()));
                        let id = id?;
                        if !self.pics.thumbs.known(&id) {
                            self.request_parts(e, &id, &parts, &kit, first);
                        }

                        id
                    }
                }
            }

            Thing::Texture(_) => return None,
        };

        if let Some(img) = self.pics.images.get(&id) {
            return Some(*img);
        }

        if let Some(img) = self.pics.thumbs.get(e, &id) {
            self.pics.images.insert(id, img);
            return Some(img);
        }

        if !self.pics.thumbs.known(&id) && self.pics.requests < REQUESTS_A_FRAME {
            self.pics.requests += 1;
            let parts = match thing {
                Thing::Model(p) => vec![AssembledPart {
                    model: p.clone(),
                    decal_diffuse: None,
                    decal_normal: None,
                    masks: [0, 1, 2],
                    tint: [0.0; 4],
                    attach: Attach::None,
                    node: None,
                }],
                Thing::Definition(p) => {
                    let vfs = e.vfs().cloned()?;
                    let read = |f: &str| vfs.read(f).ok();
                    assemble(&read, p, 1).map(|u| u.parts).unwrap_or_default()
                }

                _ => vec![],
            };
            self.request_parts(e, &id, &parts, &kit, first);
        }

        None
    }

    /// A texture's picture: a mip of 128 pixels or less, decoded here.
    fn texture_picture(&mut self, e: &mut Editor, name: &str) -> Option<ImageRef> {
        let key = format!("unit|texture|{name}");
        if let Some(img) = self.pics.images.get(&key) {
            return Some(*img);
        }

        if let Some(img) = e.images.get(&key) {
            self.pics.images.insert(key, img);
            return Some(img);
        }

        if self.pics.no_texture.contains(name) || e.images.knows(&key) || self.pics.textures >= TEXTURES_A_FRAME {
            return None;
        }

        self.pics.textures += 1;
        let read = |p: &str| e.vfs().and_then(|v| v.read(p).ok());
        let file = decals(&read, name).into_iter().next();
        let decoded = file.and_then(|f| read(&f)).and_then(|bytes| {
            let info = wce_formats::dds_decode::dds_info(&bytes);
            let mip = (0..info.mips.max(1))
                .find(|&m| (info.width >> m) <= 128 && (info.height >> m) <= 128)
                .unwrap_or(info.mips.saturating_sub(1));
            wce_formats::dds_decode::dds_decode_mip(&bytes, mip).ok()
        });
        match decoded {
            Some(img) if img.w > 0 => {
                // Opaque: a decal's alpha is where it shows, not how the picture should look.
                let mut px = img.px;
                px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
                e.images.submit(&key, img.w, img.h, px);
            }

            _ => {
                self.pics.no_texture.insert(name.to_owned());
            }
        }

        None
    }

    /// A cell: the picture (or the kind's icon while there is none) and the name under it. Returns (clicked, hovered).
    pub(super) fn cell(&mut self, e: &mut Editor, thing: &Thing, label: &str, selected: bool, uid: &str) -> (bool, bool) {
        let line = gui::text_line_height_with_spacing();
        let at = gui::cursor_screen_pos();
        let clicked = gui::selectable_sized(&format!("##cell{uid}"), selected, 0, v2(CELL, CELL + line));
        let hovered = gui::is_item_hovered();
        let img = self.picture(e, thing, hovered);
        let dl = gui::window_draw_list();
        let (pmin, pmax) = (v2(at.x + 2.0, at.y + 2.0), v2(at.x + CELL - 2.0, at.y + CELL - 2.0));
        match img {
            Some(img) => dl.image(img.id, pmin, pmax, v2(0.0, 0.0), v2(1.0, 1.0), 0xFFFF_FFFF),
            None => {
                dl.rect_filled(pmin, pmax, theme::u32(theme::mix(theme::palette_background(), theme::text(), 0.07)), 4.0);
                let icon = match thing {
                    Thing::Model(_) => icons::BOX,
                    Thing::Definition(_) => icons::USER,
                    Thing::Texture(_) => icons::IMAGE,
                    Thing::Alternative(_) => icons::LAYERS,
                };
                let size = gui::calc_text_size(icon);
                dl.text(v2(at.x + (CELL - size.x) * 0.5, at.y + (CELL - size.y) * 0.5), theme::u32(theme::dim()), icon);
            }
        }

        if selected {
            dl.rect(pmin, pmax, theme::u32(theme::palette().highlight), 4.0, 2.0);
        }

        dl.text(v2(at.x + 2.0, at.y + CELL), theme::u32(theme::text()), &fit(label, CELL - 4.0));
        if hovered {
            gui::set_tooltip(label);
        }

        (clicked, hovered)
    }

    /// A grid of cells, drawn through a clipper by lines: the index clicked.
    pub(super) fn grid(&mut self, e: &mut Editor, items: &[(Thing, String, bool)], uid: &str) -> Option<usize> {
        let spacing = gui::item_spacing().x;
        let across = (((gui::content_region_avail().x + spacing) / (CELL + spacing)) as usize).max(1);
        let lines = items.len().div_ceil(across);
        let mut picked = None;
        gui::clipped(lines, |l| {
            for (k, i) in (l * across..((l + 1) * across).min(items.len())).enumerate() {
                if k > 0 {
                    gui::same_line();
                }

                let (thing, label, selected) = &items[i];
                if self.cell(e, thing, label, *selected, &format!("{uid}{i}")).0 {
                    picked = Some(i);
                }
            }
        });

        picked
    }

    /// A button with a picture's icon: its popup lists `items` (filtered; as a grid of pictures or a list) and returns the one picked.
    pub(super) fn browse(&mut self, e: &mut Editor, id: &str, current: &str, items: &[String], kind: Browse) -> Option<String> {
        let popup = format!("##browse{id}");
        if gui::small_button(&format!("{}##{id}", icons::IMAGE)) {
            gui::open_popup(&popup);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Browse the packs' files, with pictures.");
        }

        let mut picked = None;
        gui::set_next_window_size(v2(660.0, 520.0), gui::COND_APPEARING);
        if gui::begin_popup(&popup) {
            if gui::is_window_appearing() {
                gui::set_keyboard_focus_here(0);
            }

            let filter = self.pics.filters.entry(id.to_owned()).or_default();
            gui::set_next_item_width(-120.0);
            gui::input_text(&format!("##{id}_filter"), Some("filter: words in any order"), filter, 128, 0);
            let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
            gui::same_line();
            grid_toggle(&format!("{id}_grid"), &mut self.pics.browse_grid);

            let shown: Vec<&String> = items.iter().filter(|i| wce_editor::pickers::matches(i, &words)).collect();
            gui::same_line();
            gui::text_disabled(&format!("{} of {}", shown.len(), items.len()));
            if gui::begin_child(&format!("##{id}_list"), v2(0.0, 0.0), 0, 0) {
                if self.pics.browse_grid {
                    let cells: Vec<(Thing, String, bool)> =
                        shown.iter().map(|p| (kind.thing(p), file_name(p).to_owned(), p.eq_ignore_ascii_case(current))).collect();
                    if let Some(i) = self.grid(e, &cells, id) {
                        picked = Some(shown[i].clone());
                    }
                } else {
                    gui::clipped(shown.len(), |k| {
                        if gui::selectable(shown[k], shown[k].eq_ignore_ascii_case(current), 0) {
                            picked = Some(shown[k].clone());
                        }
                    });
                }
            }

            gui::end_child();
            if picked.is_some() {
                gui::close_current_popup();
            }

            gui::end_popup();
        }

        picked
    }
}
