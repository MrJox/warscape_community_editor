//! Where things are in an open vfx file: its emitters, behaviour chains, render stacks and references by element index, with their
//! properties. Made again whenever the document changes (structural edits renumber the elements).

use wce_formats::xml_doc::XmlDoc;

/// `<property>`, `<ranged_property>` or `<texture_property>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropTag {
    Plain,
    Ranged,
    Texture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub elem: usize,
    pub tag: PropTag,
    pub name: String,

    /// The graphics quality variant (0 the base, 1 and 2 lower).
    pub quality: i32,

    /// `type="float"` and the like (modifiers write it; emitters do not).
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    pub elem: usize,
    pub id: String,
    pub kind: String,
    pub props: Vec<Prop>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    pub elem: usize,
    pub fragment: String,
    pub props: Vec<Prop>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub elem: usize,
    pub id: String,
    pub modifiers: Vec<Modifier>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub elem: usize,
    pub id: String,

    /// The `<emitter id=...>` elements of `emitters_selected` and the `<behaviour_chain id=...>` of `behaviour_chains_selected`.
    pub emitters: Vec<(usize, String)>,
    pub chains: Vec<(usize, String)>,
    pub render_params: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RefEmitter {
    pub elem: usize,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub elem: usize,
    pub inst_name: String,
    pub library: String,
    pub emitters: Vec<RefEmitter>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxIndex {
    /// The `<vfx>` root, the `<vfx_references>` root.
    pub vfx: Option<usize>,
    pub refs_root: Option<usize>,
    pub emitters_list: Option<usize>,
    pub chains_list: Option<usize>,
    pub stacks_list: Option<usize>,
    pub emitters: Vec<Emitter>,
    pub chains: Vec<Chain>,
    pub stacks: Vec<Stack>,
    pub refs: Vec<Reference>,
}

fn child(doc: &XmlDoc, e: usize, name: &str) -> Option<usize> {
    doc.elements()[e].children.iter().copied().find(|&c| doc.elements()[c].name == name)
}

fn children<'a>(doc: &'a XmlDoc, e: usize, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    doc.elements()[e].children.iter().copied().filter(move |&c| doc.elements()[c].name == name)
}

fn props(doc: &XmlDoc, e: usize) -> Vec<Prop> {
    doc.elements()[e]
        .children
        .iter()
        .filter_map(|&c| {
            let tag = match doc.elements()[c].name.as_str() {
                "property" => PropTag::Plain,
                "ranged_property" => PropTag::Ranged,
                "texture_property" => PropTag::Texture,
                _ => return None,
            };

            Some(Prop {
                elem: c,
                tag,
                name: doc.get(c, "name")?,
                quality: doc.get(c, "quality").and_then(|q| q.trim().parse().ok()).unwrap_or(0),
                kind: doc.get(c, "type").unwrap_or_default(),
            })
        })
        .collect()
}

impl VfxIndex {
    pub fn new(doc: &XmlDoc) -> Self {
        let mut ix = VfxIndex::default();
        let text = |e: usize, name: &str| doc.get(e, name).unwrap_or_default();
        for &r in doc.roots() {
            match doc.elements()[r].name.as_str() {
                "vfx" => {
                    ix.vfx = Some(r);
                    ix.emitters_list = child(doc, r, "emitters");
                    ix.chains_list = child(doc, r, "behaviour_chains");
                    ix.stacks_list = child(doc, r, "effect_render_stacks");
                    if let Some(list) = ix.emitters_list {
                        ix.emitters = children(doc, list, "emitter")
                            .map(|e| Emitter { elem: e, id: text(e, "id"), kind: text(e, "type"), props: props(doc, e) })
                            .collect();
                    }

                    if let Some(list) = ix.chains_list {
                        ix.chains = children(doc, list, "behaviour_chain")
                            .map(|c| Chain {
                                elem: c,
                                id: text(c, "id"),
                                modifiers: children(doc, c, "modifier")
                                    .map(|m| Modifier { elem: m, fragment: text(m, "fragment"), props: props(doc, m) })
                                    .collect(),
                            })
                            .collect();
                    }

                    if let Some(list) = ix.stacks_list {
                        ix.stacks = children(doc, list, "effect_render_stack")
                            .map(|s| {
                                let ids = |list: &str, item: &'static str| {
                                    child(doc, s, list)
                                        .map(|l| children(doc, l, item).map(|e| (e, text(e, "id"))).collect())
                                        .unwrap_or_default()
                                };
                                Stack {
                                    elem: s,
                                    id: text(s, "id"),
                                    emitters: ids("emitters_selected", "emitter"),
                                    chains: ids("behaviour_chains_selected", "behaviour_chain"),
                                    render_params: child(doc, s, "render_params"),
                                }
                            })
                            .collect();
                    }
                }

                "vfx_references" => {
                    ix.refs_root = Some(r);
                    ix.refs = children(doc, r, "vfx")
                        .map(|v| Reference {
                            elem: v,
                            inst_name: text(v, "inst_name"),
                            library: text(v, "vfx_ref"),
                            emitters: children(doc, v, "emitter").map(|e| RefEmitter { elem: e, name: text(e, "name") }).collect(),
                        })
                        .collect();
                }

                _ => {}
            }
        }

        ix
    }

    pub fn emitter(&self, id: &str) -> Option<&Emitter> {
        self.emitters.iter().find(|e| e.id == id)
    }

    pub fn chain(&self, id: &str) -> Option<&Chain> {
        self.chains.iter().find(|c| c.id == id)
    }

    pub fn reference(&self, inst_name: &str) -> Option<&Reference> {
        self.refs.iter().find(|r| r.inst_name == inst_name)
    }

    /// The render stacks that draw an emitter.
    pub fn stacks_of(&self, emitter: &str) -> impl Iterator<Item = &Stack> {
        self.stacks.iter().filter(move |s| s.emitters.iter().any(|(_, id)| id == emitter))
    }

    /// The emitters a chain is drawn with (through the stacks that select it).
    pub fn users_of_chain(&self, chain: &str) -> Vec<String> {
        let mut out = vec![];
        for s in self.stacks.iter().filter(|s| s.chains.iter().any(|(_, id)| id == chain)) {
            for (_, e) in &s.emitters {
                if !out.contains(e) {
                    out.push(e.clone());
                }
            }
        }

        out
    }

    /// The chain an emitter is drawn with: its first stack's first chain.
    pub fn chain_of(&self, emitter: &str) -> Option<&Chain> {
        self.stacks_of(emitter).find_map(|s| s.chains.first()).and_then(|(_, id)| self.chain(id))
    }
}

/// A property of `props` for a quality variant: the variant's own, else the base one.
pub fn pick<'a>(props: &'a [Prop], name: &str, quality: i32) -> Option<&'a Prop> {
    props.iter().find(|p| p.name == name && p.quality == quality).or_else(|| props.iter().find(|p| p.name == name && p.quality == 0))
}

/// A name unused among `taken`: `base_copy`, `base_copy2`...
pub fn unique_name(base: &str, taken: &[&str]) -> String {
    let mut name = format!("{base}_copy");
    let mut n = 2;
    while taken.contains(&name.as_str()) {
        name = format!("{base}_copy{n}");
        n += 1;
    }

    name
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "<?xml version=\"1.0\"?>\n<shader_library file=\"fx/particle_vfx_library.hlsl\"/><vfx camera_following=\"false\">\n\t<emitters>\n\t\t<emitter type=\"continuous_mesh\" id=\"fire\">\n\t\t\t<property name=\"Enable\" quality=\"0\" value=\"true\"/>\n\t\t\t<ranged_property name=\"SpawnSpeed\" quality=\"0\" min=\"1.0\" max=\"2.0\"/>\n\t\t\t<property name=\"Enable\" quality=\"1\" value=\"false\"/>\n\t\t</emitter>\n\t</emitters>\n\t<behaviour_chains>\n\t\t<behaviour_chain id=\"bc\">\n\t\t\t<modifier fragment=\"ps_texture\">\n\t\t\t\t<texture_property name=\"diffuse\" type=\"file\" path=\"a.dds\"/>\n\t\t\t</modifier>\n\t\t</behaviour_chain>\n\t</behaviour_chains>\n\t<effect_render_stacks>\n\t\t<effect_render_stack id=\"rs\">\n\t\t\t<emitters_selected>\n\t\t\t\t<emitter id=\"fire\"/>\n\t\t\t</emitters_selected>\n\t\t\t<behaviour_chains_selected>\n\t\t\t\t<behaviour_chain id=\"bc\"/>\n\t\t\t</behaviour_chains_selected>\n\t\t\t<render_params scene_view_type=\"0\" cast_shadows=\"false\"/>\n\t\t</effect_render_stack>\n\t</effect_render_stacks>\n</vfx><vfx_references><vfx inst_name=\"r1\" vfx_ref=\"lib\"><emitter name=\"a\" enable=\"true\"/></vfx></vfx_references>";

    #[test]
    fn indexes_a_file() {
        let doc = XmlDoc::parse(FILE.as_bytes().to_vec()).unwrap();
        let ix = VfxIndex::new(&doc);
        assert_eq!(ix.emitters.len(), 1);
        let e = &ix.emitters[0];
        assert_eq!((e.id.as_str(), e.kind.as_str(), e.props.len()), ("fire", "continuous_mesh", 3));
        assert_eq!(e.props[1].tag, PropTag::Ranged);
        assert_eq!(pick(&e.props, "Enable", 1).map(|p| p.quality), Some(1));
        assert_eq!(pick(&e.props, "SpawnSpeed", 2).map(|p| p.quality), Some(0));
        assert_eq!(ix.chain_of("fire").map(|c| c.id.as_str()), Some("bc"));
        assert_eq!(ix.chains[0].modifiers[0].props[0].tag, PropTag::Texture);
        assert_eq!(ix.users_of_chain("bc"), vec!["fire".to_owned()]);
        assert_eq!(ix.refs[0].library, "lib");
        assert_eq!(ix.refs[0].emitters[0].name, "a");
        assert_eq!(unique_name("fire", &["fire", "fire_copy"]), "fire_copy2");
    }
}
