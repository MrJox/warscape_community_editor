//! A `.terry` project (docs/re/terry.md): `<project version><scene version>`, a flat list of `<entity id name>` with `<components>`
//! (one element per component: `ECTransform`, `ECRectangle`, `ECBattleCatchmentArea`, ...) and `<children>` (`<child id/>`). The text is
//! a lossless [`XmlDoc`]: an unedited project saves byte for byte and an edit rewrites only its attribute's value. This module indexes
//! the entities; their components are read from the document by name.
//!
//! Positions (`ECTransform position="x y z"`) are metres from the map's centre, x east, z north (256 a cell: docs/re/terry.md), relative
//! to the transform parent's: the entity whose `ECTransform` lists it in its own `<children>`. That is not the entity tree (`<entity>`'s
//! `<children>`): a BCA's entity children are its `Catchment Region` and its deployment zones, its transform's only the region, and the
//! zones' regions are placed on the map itself (`DZR 0` at -105150 under a BCA at -104192; BOB's deployment files agree). A polyline's point (x, y) is at (x, z) + its entity's position (checked against BOB's deployment files: the
//! points come out shifted by the tile's centre, y as z). No vanilla entity is turned or scaled (`rotation="0 0 0"`, `scale="1 1 1"`
//! on all of the campaign battle map's 52 016 transforms), so the shapes are read as translated only.
//!
//! Structural edits (an entity added or removed) are text splices of the document ([`XmlSplice`]): small, exact undo records.

use std::collections::HashMap;

use wce_formats::xml_doc::{XmlDoc, XmlError, XmlSplice};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerryEntity {
    /// Its `<entity>` element.
    pub elem: usize,

    /// The `id` attribute (hexadecimal in the file).
    pub id: u64,
    pub name: String,

    /// (component name, its element), in the file's order.
    pub components: Vec<(String, usize)>,

    /// The children's ids.
    pub children: Vec<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct TerryProject {
    pub doc: XmlDoc,
    pub entities: Vec<TerryEntity>,
    by_id: HashMap<u64, usize>,
    parents: HashMap<u64, u64>,
    transform_parents: HashMap<u64, u64>,
}

/// What an entity is, by its components (Terry's creation modes make these).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityKind {
    /// A battle catchment area (`ECBattleCatchmentArea`) and its type.
    Bca(String),
    /// A BCA's `Catchment Region` (a rectangle child).
    CatchmentRegion,
    DeploymentZone,
    DeploymentRegion,
    Other,
}

impl EntityKind {
    pub fn name(&self) -> String {
        match self {
            Self::Bca(t) => format!("battle catchment area ({t})"),
            Self::CatchmentRegion => "catchment region".to_owned(),
            Self::DeploymentZone => "deployment zone".to_owned(),
            Self::DeploymentRegion => "deployment zone region".to_owned(),
            Self::Other => "entity".to_owned(),
        }
    }
}

/// Numbers of an attribute ("x y z").
fn numbers(v: &str) -> Vec<f64> {
    v.split_whitespace().filter_map(|t| t.parse().ok()).collect()
}

/// A number as Terry writes it (up to 9 significant digits, no trailing zeros).
pub fn number_text(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }

    let mut t = format!("{v:.9}");
    let digits = v.abs().log10().floor() as i32 + 1;
    if digits > 0 {
        t = format!("{v:.prec$}", prec = (9 - digits).max(0) as usize);
    }

    if t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_owned();
    }

    t
}

impl TerryProject {

    /// # Errors
    ///
    /// The text is not XML.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, XmlError> {
        let doc = XmlDoc::parse(bytes)?;
        let mut p = Self { doc, ..Default::default() };
        p.index();
        Ok(p)
    }

    fn index(&mut self) {
        self.entities.clear();
        self.by_id.clear();
        let elems = self.doc.elements();
        let Some(scene) = self.doc.roots().iter().flat_map(|&r| elems[r].children.iter().copied()).find(|&e| elems[e].name == "scene") else { return };
        for &e in &elems[scene].children {
            if elems[e].name != "entity" {
                continue;
            }

            let id = self.doc.get(e, "id").and_then(|v| u64::from_str_radix(&v, 16).ok()).unwrap_or(0);
            let mut ent = TerryEntity { elem: e, id, name: self.doc.get(e, "name").unwrap_or_default(), ..Default::default() };
            for &c in &elems[e].children {
                match elems[c].name.as_str() {
                    "components" => ent.components = elems[c].children.iter().map(|&k| (elems[k].name.clone(), k)).collect(),
                    "children" => {
                        ent.children = elems[c].children.iter().filter_map(|&k| self.doc.get(k, "id").and_then(|v| u64::from_str_radix(&v, 16).ok())).collect();
                    }

                    _ => {}
                }
            }

            self.by_id.insert(id, self.entities.len());
            self.entities.push(ent);
        }

        self.parents = self.entities.iter().flat_map(|e| e.children.iter().map(move |&c| (c, e.id))).collect();
        let elems = self.doc.elements();
        let mut tp = HashMap::new();
        for e in &self.entities {
            let Some(t) = self.component(e, "ECTransform") else { continue };
            for &c in elems[t].children.iter().filter(|&&c| elems[c].name == "children") {
                for &k in &elems[c].children {
                    if let Some(id) = self.doc.get(k, "id").and_then(|v| u64::from_str_radix(&v, 16).ok()) {
                        tp.insert(id, e.id);
                    }
                }
            }
        }

        self.transform_parents = tp;
    }

    /// The entity an entity's position is relative to (its parent's `ECTransform` lists it).
    pub fn transform_parent_of(&self, id: u64) -> Option<u64> {
        self.transform_parents.get(&id).copied()
    }

    /// Reads the entities again (after an edit of the document that changes names or the tree).
    pub fn reindex(&mut self) {
        self.index();
    }

    pub fn save(&self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn entity(&self, id: u64) -> Option<&TerryEntity> {
        self.by_id.get(&id).map(|&i| &self.entities[i])
    }

    pub fn parent_of(&self, id: u64) -> Option<u64> {
        self.parents.get(&id).copied()
    }

    /// What an entity is.
    pub fn kind(&self, e: &TerryEntity) -> EntityKind {
        let has = |n: &str| e.components.iter().any(|(c, _)| c == n);
        if let Some(c) = self.component(e, "ECBattleCatchmentArea") {
            return EntityKind::Bca(self.doc.get(c, "catchment_type").unwrap_or_default());
        }

        if has("ECDeploymentZone") {
            EntityKind::DeploymentZone
        } else if has("ECDeploymentZoneRegion") {
            EntityKind::DeploymentRegion
        } else if has("ECRectangle") && self.parent_of(e.id).and_then(|p| self.entity(p)).is_some_and(|p| self.component(p, "ECBattleCatchmentArea").is_some()) {
            EntityKind::CatchmentRegion
        } else {
            EntityKind::Other
        }
    }

    /// An entity's position relative to its parent (metres), none without a transform.
    pub fn local_position(&self, e: &TerryEntity) -> Option<[f64; 3]> {
        let t = self.component(e, "ECTransform")?;
        let v = numbers(&self.doc.get(t, "position")?);
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    }

    /// An entity's position on the map: its own added to its transform ancestors' (an entity without a transform: its transform parent's).
    pub fn world_position(&self, id: u64) -> [f64; 3] {
        let mut p = [0.0; 3];
        let mut at = Some(id);
        let mut guard = 0;
        while let Some(i) = at.filter(|_| guard < 64) {
            if let Some(l) = self.entity(i).and_then(|e| self.local_position(e)) {
                p = [p[0] + l[0], p[1] + l[1], p[2] + l[2]];
            }

            at = self.transform_parent_of(i);
            guard += 1;
        }

        p
    }

    /// An `ECRectangle`'s width and height (metres, centred on the entity).
    pub fn rectangle(&self, e: &TerryEntity) -> Option<[f64; 2]> {
        let c = self.component(e, "ECRectangle")?;
        Some([self.doc.get(c, "width")?.parse().ok()?, self.doc.get(c, "height")?.parse().ok()?])
    }

    /// An `ECPolyline`'s points (x, z offsets from the entity, metres) and whether it is closed.
    pub fn polyline(&self, e: &TerryEntity) -> Option<(Vec<[f64; 2]>, bool)> {
        let c = self.component(e, "ECPolyline")?;
        let elems = self.doc.elements();
        let line = elems[c].children.iter().copied().find(|&k| elems[k].name == "polyline")?;
        let closed = self.doc.get(line, "closed").is_some_and(|v| v == "true");
        let pts = elems[line].children.iter().filter(|&&k| elems[k].name == "point")
            .filter_map(|&k| Some([self.doc.get(k, "x")?.parse().ok()?, self.doc.get(k, "y")?.parse().ok()?]))
            .collect();
        Some((pts, closed))
    }

    /// Sets an attribute of an entity's component; its value before (none: no such entity, component or attribute).
    pub fn set_attr(&mut self, id: u64, component: &str, attr: &str, value: &str) -> Option<String> {
        let e = self.entity(id)?;
        let c = self.component(e, component)?;
        let before = self.doc.get(c, attr)?;
        self.doc.set(c, attr, value);
        Some(before)
    }

    /// Lists `child` in `parent`'s entity `<children>` (opening a `<children/>`). The splices that did it.
    pub fn add_child(&mut self, parent: u64, child: u64) -> Result<Vec<XmlSplice>, XmlError> {
        self.doc.bake_pending()?;
        let mark = self.doc.journal_mark();
        let (eol, indent) = self.style();
        let step = if indent.len() >= 8 { indent[..indent.len() / 2].to_owned() } else { "    ".to_owned() };
        let e = self.entity(parent).ok_or_else(|| XmlError::Edit("no such entity".to_owned()))?;
        let elems = self.doc.elements();
        let c = elems[e.elem].children.iter().copied().find(|&k| elems[k].name == "children").ok_or_else(|| XmlError::Edit("no <children>".to_owned()))?;
        let el = &elems[c];
        let line = format!("{indent}{step}{step}<child id=\"{child:x}\"/>{eol}");
        if el.self_closing {
            let text = format!(">{eol}{line}{indent}{step}</children>");
            self.doc.splice(el.attrs_end, el.start_end - el.attrs_end, text.as_bytes())?;
        } else {
            let t = self.doc.original_text();
            let close = el.end_off - "</children>".len();
            let ls = t[..close].iter().rposition(|&b| b == b'\n').map_or(close, |p| p + 1);
            self.doc.splice(ls, 0, line.as_bytes())?;
        }

        let out = self.doc.journal_since(mark);
        self.index();
        Ok(out)
    }

    /// An unused entity id (one past the largest).
    pub fn next_id(&self) -> u64 {
        self.entities.iter().map(|e| e.id).max().map_or(0x1_0000_0000_0000, |m| m + 1)
    }

    /// The text's line ending and the scene's entity indent (as the kit writes: CRLF or LF, eight spaces).
    fn style(&self) -> (&'static str, String) {
        let t = self.doc.original_text();
        let eol = if t.windows(2).any(|w| w == b"\r\n") { "\r\n" } else { "\n" };
        let indent = self.entities.first().map_or_else(|| "        ".to_owned(), |e| {
            let off = self.doc.elements()[e.elem].start_off;
            let ls = t[..off].iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
            String::from_utf8_lossy(&t[ls..off]).into_owned()
        });

        (eol, indent)
    }

    /// Adds entities (their XML, `{eol}` for the line ending, `{i}` for one indent step: the text indented as a scene's entities are) at
    /// the scene's end. The splices that did it (empty on failure).
    pub fn insert_entities(&mut self, xml: &str) -> Result<Vec<XmlSplice>, XmlError> {
        self.doc.bake_pending()?;
        let mark = self.doc.journal_mark();

        // An old project (`<project version="2"><scene version="3">`, the kit's empty ones) is written as Terry writes one with entities,
        // version 3 and 10: BOB's `Terry file` fails on entities under the old header (exit 1, no message).
        let elems = self.doc.elements();
        let root = self.doc.roots().iter().copied().find(|&r| elems[r].name == "project");
        let scene_el = root.and_then(|r| elems[r].children.iter().copied().find(|&e| elems[e].name == "scene"));
        let old = |e: usize, min: u32| self.doc.get(e, "version").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v < min);
        let (old_project, old_scene) = (root.is_some_and(|r| old(r, 3)), scene_el.is_some_and(|s| old(s, 10)));
        if let (true, Some(r)) = (old_project, root) {
            self.doc.set(r, "version", "3");
        }

        if let (true, Some(s)) = (old_scene, scene_el) {
            self.doc.set(s, "version", "10");
        }

        self.doc.bake_pending()?;
        let (eol, indent) = self.style();
        let step = if indent.len() >= 8 { &indent[..indent.len() / 2] } else { "    " };
        let body: String = xml.lines().map(|l| format!("{indent}{}{eol}", l.replace("{i}", step))).collect();
        let elems = self.doc.elements();
        let scene = self.doc.roots().iter().flat_map(|&r| elems[r].children.iter().copied()).find(|&e| elems[e].name == "scene")
            .ok_or_else(|| XmlError::Edit("no scene".to_owned()))?;
        let el = &elems[scene];
        if el.self_closing {
            // `<scene version="10"/>` (an empty project) opened: the tag's `/>` becomes `>`, the entities, then `</scene>` at the scene's indent.
            let t = self.doc.original_text();
            let ls = t[..el.start_off].iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
            let scene_indent = String::from_utf8_lossy(&t[ls..el.start_off]).into_owned();
            let text = format!(">{eol}{body}{scene_indent}</scene>");
            let (pos, len) = (el.attrs_end, el.start_end - el.attrs_end);
            self.doc.splice(pos, len, text.as_bytes())?;
        } else {
            let t = self.doc.original_text();
            let close = el.end_off - "</scene>".len();
            let ls = t[..close].iter().rposition(|&c| c == b'\n').map_or(close, |p| p + 1);
            self.doc.splice(ls, 0, body.as_bytes())?;
        }

        let out = self.doc.journal_since(mark);
        self.index();
        Ok(out)
    }

    /// Replaces an `ECPolyline`'s points (x, z offsets from the entity, metres): the `<point>` lines between `<polyline>` and its end tag
    /// written again in the first point's indent (the file's line ending), the rest of the file as it was. The splices that did it.
    pub fn set_polyline(&mut self, id: u64, pts: &[[f64; 2]]) -> Result<Vec<XmlSplice>, XmlError> {
        self.doc.bake_pending()?;
        let missing = || XmlError::Edit("no polyline".to_owned());
        let e = self.entity(id).ok_or_else(missing)?;
        let c = self.component(e, "ECPolyline").ok_or_else(missing)?;
        let elems = self.doc.elements();
        let line = elems[c].children.iter().copied().find(|&k| elems[k].name == "polyline").ok_or_else(missing)?;
        let el = &elems[line];
        if el.self_closing {
            return Err(XmlError::Edit("a polyline without points".to_owned()));
        }

        let t = self.doc.original_text();
        let indent_of = |off: usize| {
            let ls = t[..off].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
            String::from_utf8_lossy(&t[ls..off]).into_owned()
        };

        let (eol, _) = self.style();
        let own = indent_of(el.start_off);
        let first = el.children.iter().copied().find(|&k| elems[k].name == "point");
        let step = if own.contains('\t') { "\t".to_owned() } else { "    ".to_owned() };
        let point_indent = first.map_or_else(|| format!("{own}{step}"), |k| indent_of(elems[k].start_off));
        let mut text = String::from(eol);
        for q in pts {
            text += &format!("{point_indent}<point x=\"{}\" y=\"{}\"/>{eol}", number_text(q[0]), number_text(q[1]));
        }

        text += &own;
        let close = el.end_off - "</polyline>".len();
        let (pos, len) = (el.start_end, close - el.start_end);
        let mark = self.doc.journal_mark();
        self.doc.splice(pos, len, text.as_bytes())?;
        let out = self.doc.journal_since(mark);
        self.index();
        Ok(out)
    }

    /// Removes entities, their descendants and every `<child id>` naming them. The splices that did it.
    pub fn remove_entities(&mut self, ids: &[u64]) -> Result<Vec<XmlSplice>, XmlError> {
        self.doc.bake_pending()?;
        let mark = self.doc.journal_mark();
        let mut gone: Vec<u64> = vec![];
        let mut todo: Vec<u64> = ids.to_vec();
        while let Some(id) = todo.pop() {
            if gone.contains(&id) {
                continue;
            }

            gone.push(id);
            if let Some(e) = self.entity(id) {
                todo.extend(e.children.iter().copied());
            }
        }

        // The entities' elements and the references to them, last first (the earlier elements' indices stay).
        let elems = self.doc.elements();
        let mut doomed: Vec<usize> = gone.iter().filter_map(|&id| self.entity(id).map(|e| e.elem)).collect();
        let is_ref = |k: usize| elems[k].name == "child" && self.doc.get(k, "id").and_then(|v| u64::from_str_radix(&v, 16).ok()).is_some_and(|i| gone.contains(&i));
        doomed.extend((0..elems.len()).filter(|&k| is_ref(k)));
        doomed.sort_unstable();
        doomed.dedup();

        // An element inside another doomed one goes with it.
        let inside = |k: usize| {
            let mut p = elems[k].parent;
            while let Some(q) = p {
                if doomed.binary_search(&q).is_ok() {
                    return true;
                }

                p = elems[q].parent;
            }

            false
        };

        let top: Vec<usize> = doomed.iter().copied().filter(|&k| !inside(k)).collect();
        for &k in top.iter().rev() {
            self.doc.remove_element_inplace(k)?;
        }

        let out = self.doc.journal_since(mark);
        self.index();
        Ok(out)
    }

    /// An entity's XML and its descendants' (each `<entity>` element's text as the file has it, without the indent of its first line).
    pub fn entity_text(&self, id: u64) -> Vec<(u64, String)> {
        let mut out = vec![];
        let mut todo = vec![id];
        while let Some(i) = todo.pop() {
            let Some(e) = self.entity(i) else { continue };
            let el = &self.doc.elements()[e.elem];
            out.push((i, String::from_utf8_lossy(&self.doc.text_of_range(el.start_off, el.end_off)).into_owned()));
            todo.extend(e.children.iter().rev().copied());
        }

        out
    }

    /// Applies recorded splices again (redo), then reads the entities again.
    pub fn redo(&mut self, splices: &[XmlSplice]) {
        for s in splices {
            let _ = self.doc.apply_splice(s);
        }

        self.index();
    }

    /// Takes recorded splices back (undo), then reads the entities again.
    pub fn undo(&mut self, splices: &[XmlSplice]) {
        for s in splices.iter().rev() {
            let _ = self.doc.unapply_splice(s);
        }

        self.index();
    }

    /// The element of an entity's component.
    pub fn component(&self, ent: &TerryEntity, name: &str) -> Option<usize> {
        ent.components.iter().find(|(n, _)| n == name).map(|&(_, e)| e)
    }

    /// The entities with a component.
    pub fn with_component<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a TerryEntity> + 'a {
        self.entities.iter().filter(move |e| e.components.iter().any(|(n, _)| n == name))
    }

    /// The count of each component, by name (sorted).
    pub fn component_counts(&self) -> Vec<(String, usize)> {
        let mut n: HashMap<&str, usize> = HashMap::new();
        for e in &self.entities {
            for (c, _) in &e.components {
                *n.entry(c.as_str()).or_default() += 1;
            }
        }

        let mut v: Vec<(String, usize)> = n.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        v.sort();
        v
    }
}

#[cfg(test)]
fn tests_text() -> &'static str {
    tests::TEXT
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const TEXT: &str = r#"<project version="3">
    <scene version="10">
        <entity id="140532c302f8fd6" name="Thermopylae">
            <components>
                <ECRectangle width="2048" height="2048" active="true"/>
                <ECTransform rotation="0 0 0" scale="1 1 1" active="true" position="-31744 26.9883385 -13568">
                    <children>
                        <child id="140532c302f8fd7"/>
                    </children>
                </ECTransform>
                <ECBattleCatchmentArea catchment_type="land_normal" active="true"/>
            </components>
            <children>
                <child id="140532c302f8fd7"/>
            </children>
        </entity>
        <entity id="140532c302f8fd7" name="Catchment Region">
            <components>
                <ECRectangle width="3434.16406" height="3352.6875" active="true"/>
            </components>
            <children/>
        </entity>
    </scene>
</project>
"#;

    #[test]
    fn index_and_edit() {
        let mut p = TerryProject::parse(TEXT.as_bytes().to_vec()).expect("xml");
        assert_eq!(p.save(), TEXT.as_bytes());
        assert_eq!(p.entities.len(), 2);
        let bca = p.with_component("ECBattleCatchmentArea").next().expect("a BCA").clone();
        assert_eq!(bca.name, "Thermopylae");
        assert_eq!(bca.children, vec![0x1_4053_2c30_2f8f_d7]);
        assert_eq!(p.entity(bca.children[0]).map(|e| e.name.as_str()), Some("Catchment Region"));
        let t = p.component(&bca, "ECBattleCatchmentArea").expect("component");
        p.doc.set(t, "catchment_type", "land_ambush");
        assert_eq!(p.save(), TEXT.replace("catchment_type=\"land_normal\"", "catchment_type=\"land_ambush\"").as_bytes());
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    const EMPTY: &str = "<project version=\"3\">\r\n    <scene version=\"10\"/>\r\n</project>\r\n";

    fn bca(id: u64) -> String {
        format!("<entity id=\"{id:x}\" name=\"new\">\n{{i}}<components>\n{{i}}{{i}}<ECRectangle width=\"2048\" height=\"2048\" active=\"true\"/>\n{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"256 0 -512\"/>\n{{i}}{{i}}<ECBattleCatchmentArea catchment_type=\"land_normal\" active=\"true\"/>\n{{i}}</components>\n{{i}}<children/>\n</entity>")
    }

    #[test]
    fn insert_into_an_empty_scene_and_back() {
        let mut p = TerryProject::parse(EMPTY.as_bytes().to_vec()).expect("xml");
        let s = p.insert_entities(&bca(0x10)).expect("inserted");
        let text = String::from_utf8(p.save()).expect("utf8");
        assert!(text.starts_with("<project version=\"3\">\r\n    <scene version=\"10\">\r\n        <entity id=\"10\" name=\"new\">\r\n            <components>\r\n"), "{text}");
        assert!(text.ends_with("        </entity>\r\n    </scene>\r\n</project>\r\n"), "{text}");
        assert_eq!(p.entities.len(), 1);
        assert_eq!(p.world_position(0x10), [256.0, 0.0, -512.0]);
        assert_eq!(p.kind(&p.entities[0]), EntityKind::Bca("land_normal".to_owned()));
        p.undo(&s);
        assert_eq!(p.save(), EMPTY.as_bytes());
        assert!(p.entities.is_empty());
        p.redo(&s);
        assert_eq!(String::from_utf8(p.save()).expect("utf8"), text);
    }

    #[test]
    fn an_old_project_gets_terrys_versions() {
        let old = "<project version=\"2\">
    <scene version=\"3\"/>
</project>
";
        let mut p = TerryProject::parse(old.as_bytes().to_vec()).expect("xml");
        let s = p.insert_entities(&bca(0x10)).expect("inserted");
        let text = String::from_utf8(p.save()).expect("utf8");
        assert!(text.starts_with("<project version=\"3\">
    <scene version=\"10\">
        <entity id=\"10\""), "{text}");
        p.undo(&s);
        assert_eq!(p.save(), old.as_bytes());
    }

    #[test]
    fn remove_takes_children_and_references() {
        let mut p = TerryProject::parse(super::tests_text().as_bytes().to_vec()).expect("xml");
        let before = p.save();
        assert_eq!(p.world_position(0x1_4053_2c30_2f8f_d7), [-31744.0, 26.9883385, -13568.0]);
        let s = p.insert_entities(&bca(0x99)).expect("inserted");
        let s2 = p.remove_entities(&[0x1_4053_2c30_2f8f_d6]).expect("removed");
        assert_eq!(p.entities.len(), 1);
        assert!(!String::from_utf8_lossy(&p.save()).contains("140532c302f8fd7"), "the child and every reference go");
        p.undo(&s2);
        p.undo(&s);
        assert_eq!(p.save(), before);
        assert_eq!(p.entities.len(), 2);
    }

    #[test]
    fn a_zone_under_a_bca() {
        let mut p = TerryProject::parse(super::tests_text().as_bytes().to_vec()).expect("xml");
        let before = p.save();
        let zone = "<entity id=\"77\" name=\"DZ 0/0\">\n{i}<components>\n{i}{i}<ECDeploymentZone alliance_id=\"0\" active=\"true\"/>\n{i}</components>\n{i}<children/>\n</entity>";
        let mut s = p.insert_entities(zone).expect("inserted");
        s.extend(p.add_child(0x1_4053_2c30_2f8f_d6, 0x77).expect("child"));
        assert_eq!(p.parent_of(0x77), Some(0x1_4053_2c30_2f8f_d6));
        assert_eq!(p.transform_parent_of(0x77), None, "a zone is no transform child");
        assert_eq!(p.transform_parent_of(0x1_4053_2c30_2f8f_d7), Some(0x1_4053_2c30_2f8f_d6));
        assert_eq!(p.world_position(0x77), [0.0; 3]);
        p.undo(&s);
        assert_eq!(p.save(), before);
    }

    #[test]
    fn numbers_as_terry_writes_them() {
        assert_eq!(number_text(-31744.0), "-31744");
        assert_eq!(number_text(26.988338470458984), "26.9883385");
        assert_eq!(number_text(3434.164_06), "3434.16406");
        assert_eq!(number_text(0.5), "0.5");
    }

    #[test]
    fn a_polylines_points_rewritten_and_back() {
        let mut p = TerryProject::parse(super::tests_text().as_bytes().to_vec()).expect("xml");
        let region = "<entity id=\"20\" name=\"DZR 0\">\n{i}<components>\n{i}{i}<ECDeploymentZoneRegion active=\"true\" region_type=\"DZRT_ADDITIVE\"/>\n\
{i}{i}<ECPolyline active=\"true\">\n{i}{i}{i}<polyline closed=\"true\">\n{i}{i}{i}{i}<point x=\"0\" y=\"0\"/>\n{i}{i}{i}{i}<point x=\"100\" y=\"0\"/>\n\
{i}{i}{i}{i}<point x=\"100\" y=\"-50\"/>\n{i}{i}{i}</polyline>\n{i}{i}</ECPolyline>\n{i}{i}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"10 0 20\"/>\n\
{i}</components>\n{i}<children/>\n</entity>";
        p.insert_entities(region).expect("inserted");
        let before = p.save();
        let s = p.set_polyline(0x20, &[[0.0, 0.0], [120.5, 0.0], [120.5, -60.0], [0.0, -60.0]]).expect("set");
        let e = p.entity(0x20).expect("the region").clone();
        assert_eq!(p.polyline(&e), Some((vec![[0.0, 0.0], [120.5, 0.0], [120.5, -60.0], [0.0, -60.0]], true)));
        let text = String::from_utf8(p.save()).expect("utf-8");
        assert!(text.contains("                        <point x=\"120.5\" y=\"-60\"/>\n                        <point x=\"0\" y=\"-60\"/>\n                    </polyline>"));
        p.undo(&s);
        assert_eq!(p.save(), before);
    }
}
