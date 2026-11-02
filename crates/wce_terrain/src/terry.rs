//! A `.terry` project (docs/re/terry.md): `<project version><scene version>`, a flat list of `<entity id name>` with `<components>`
//! (one element per component: `ECTransform`, `ECRectangle`, `ECBattleCatchmentArea`, ...) and `<children>` (`<child id/>`). The text is
//! a lossless [`XmlDoc`]: an unedited project saves byte for byte and an edit rewrites only its attribute's value. This module indexes
//! the entities; their components are read from the document by name.

use std::collections::HashMap;

use wce_formats::xml_doc::{XmlDoc, XmlError};

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
    }

    pub fn save(&self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn entity(&self, id: u64) -> Option<&TerryEntity> {
        self.by_id.get(&id).map(|&i| &self.entities[i])
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
mod tests {
    use super::*;

    const TEXT: &str = r#"<project version="3">
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
