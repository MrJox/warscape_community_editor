//! The Inspector on several `.terry` entities at once: an attribute they share set on all of them, one step on the history.

use std::sync::Arc;

use wce_editor_core::document::Document;
use wce_editor_core::inspector::{InspectTarget, Inspector};
use wce_scene::render_scene::SourceRef;
use wce_terrain::terry::TerryProject;
use wce_terrain::{Item, MapModel};
use wce_terrain_editor::MapDocument;
use wce_terrain_editor::document::MapSource;
use wce_tile_model::Blob;

const TEXT: &str = r#"<project version="3">
    <scene version="10">
        <entity id="10" name="A">
            <components>
                <ECRectangle width="2048" height="2048" active="true"/>
                <ECTransform rotation="0 0 0" scale="1 1 1" active="true" position="0 0 0"/>
                <ECBattleCatchmentArea catchment_type="land_normal" active="true"/>
            </components>
            <children/>
        </entity>
        <entity id="11" name="B">
            <components>
                <ECRectangle width="2048" height="2048" active="true"/>
                <ECTransform rotation="0 0 0" scale="1 1 1" active="true" position="2048 0 0"/>
                <ECBattleCatchmentArea catchment_type="land_ambush" active="true"/>
            </components>
            <children/>
        </entity>
    </scene>
</project>
"#;

/// Records the fields; sets one string field.
#[derive(Default)]
struct Rows {
    target: InspectTarget,
    fields: Vec<String>,
    set: Option<(String, String)>,
}

impl Inspector for Rows {
    fn target(&self) -> InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: InspectTarget) {
        self.target = target;
    }

    fn section(&mut self, _: &str) {}

    fn text(&mut self, field: &str, label: &str, _: &str) {
        self.fields.push(format!("{field}={label}"));
    }

    fn edit_float(&mut self, field: &str, label: &str, _: &mut f32, _: f32, _: f32, _: f32) -> bool {
        self.fields.push(format!("{field}={label}"));
        false
    }

    fn edit_float3(&mut self, field: &str, label: &str, _: &mut [f32; 3], _: f32) -> bool {
        self.fields.push(format!("{field}={label}"));
        false
    }

    fn edit_float2(&mut self, _: &str, _: &str, _: &mut [f32; 2], _: f32) -> bool {
        false
    }

    fn edit_int(&mut self, _: &str, _: &str, _: &mut i64, _: f32) -> bool {
        false
    }

    fn edit_color3(&mut self, _: &str, _: &str, _: &mut [f32; 3]) -> bool {
        false
    }

    fn edit_color4(&mut self, _: &str, _: &str, _: &mut [f32; 4]) -> bool {
        false
    }

    fn edit_bool(&mut self, field: &str, label: &str, _: &mut bool) -> bool {
        self.fields.push(format!("{field}={label}"));
        false
    }

    fn edit_string(&mut self, field: &str, label: &str, v: &mut String) -> bool {
        self.fields.push(format!("{field}={label}"));
        match &self.set {
            Some((f, x)) if f == field => {
                v.clone_from(x);
                true
            }
            _ => false,
        }
    }

    fn edit_choice(&mut self, field: &str, label: &str, index: &mut i32, list: &[String]) -> bool {
        self.fields.push(format!("{field}={label}"));
        match &self.set {
            Some((f, x)) if f == field => {
                *index = list.iter().position(|v| v == x).unwrap_or(0) as i32;
                true
            }
            _ => false,
        }
    }

    fn gesture_ended(&mut self) -> bool {
        true
    }
}

fn types(d: &MapDocument) -> Vec<String> {
    let p = d.project().expect("project");
    p.entities.iter().filter_map(|e| p.component(e, "ECBattleCatchmentArea").and_then(|c| p.doc.get(c, "catchment_type"))).collect()
}

#[test]
fn an_attribute_set_on_both_and_undone_at_once() {
    let project = TerryProject::parse(TEXT.as_bytes().to_vec()).expect("xml");
    let model = MapModel { project: Some(Item::new("t.terry", project, Blob::Bytes(Arc::from(TEXT.as_bytes())))), ..Default::default() };
    let mut d = MapDocument::new("t".to_owned(), MapSource::Kit("t".into()), Some(model), Arc::default(), None, Arc::default());
    let items = [SourceRef { doc: 0, item: 0x10 }, SourceRef { doc: 0, item: 0x11 }];

    let mut rows = Rows::default();
    assert!(d.inspect_many(&items, &mut rows));
    assert!(rows.fields.iter().any(|f| f == "many.ECBattleCatchmentArea.catchment_type=catchment_type (1 differ)"), "{:?}", rows.fields);
    assert!(rows.fields.iter().any(|f| f.starts_with("many.ECTransform.position=")), "positions are shown");

    let before = d.history().version();
    let mut rows =
        Rows { set: Some(("many.ECBattleCatchmentArea.catchment_type".to_owned(), "coastal_battle".to_owned())), ..Default::default() };
    d.inspect_many(&items, &mut rows);
    assert_eq!(types(&d), ["coastal_battle", "coastal_battle"]);
    assert_eq!(d.history().size(), 1, "one step");
    assert_ne!(d.history().version(), before);

    d.history_mut().undo();
    assert_eq!(types(&d), ["land_normal", "land_ambush"]);
}
