//! The Inspector of an environment file by effect (`XmlFileDocument::inspect_environment`): sections and tooltips, toggles, a setting the file
//! lacks being added when edited, whole numbers staying integers, and edits changing only their own spans of the file.

use std::path::PathBuf;

use wce_editor_core::document::Document;
use wce_editor_core::inspector::{InspectTarget, Inspector};
use wce_weather::xml_file_document::{Source, XmlFileDocument};

const FILE: &str = "<SCENE name='Scene'>\n\
  <LIGHTING name='Lighting' sun_colour='rgb(1.0,0.5,0.25)' sun_direction='euler(0,3.141593,0)'/>\n\
  <FOG name='Fog' fog_distance_strength='3' terrain_relative_height_fog='true' volume_fog_colour='argb(0.5,0.5,0.5,0.0039)'>\n\
    <DEPTH_OF_FIELD name='Depth Of Field' use_DOF='false' DOF_focus='variance(0.3,40)'/>\n\
    <HDR name='HDR' low_tones_bias='0.3' low_tones_bias_red='0.4'/>\n\
  </FOG>\n\
  <POST_PROCESSING name='Post Processing' brightness_r='1' brightness_g='1' brightness_b='1'/>\n\
</SCENE>\n";

/// What one pass of the Inspector asks for, and the edits it is told the user made.
#[derive(Default)]
struct Script {
    target: InspectTarget,
    sections: Vec<String>,
    rows: Vec<String>,
    hints: Vec<String>,
    dims: Vec<String>,

    /// (field suffix, value) the next pass reports as edited.
    set_float: Vec<(&'static str, f32)>,
    set_trio: Vec<(&'static str, [f32; 3])>,
    toggle: Vec<&'static str>,
    pending_hint: String,
    pending_dim: bool,
    ended: bool,
}

impl Script {
    fn row(&mut self, field: &str) {
        self.rows.push(field.to_owned());
        let hint = std::mem::take(&mut self.pending_hint);
        self.hints.push(hint);
        if std::mem::take(&mut self.pending_dim) {
            self.dims.push(field.to_owned());
        }
    }
}

impl Inspector for Script {
    fn target(&self) -> InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: InspectTarget) {
        self.target = target;
    }

    fn hint(&mut self, text: &str) {
        self.pending_hint = text.to_owned();
    }

    fn dim_next(&mut self) {
        self.pending_dim = true;
    }

    fn section(&mut self, title: &str) {
        self.sections.push(title.to_owned());
    }

    fn text(&mut self, field: &str, _: &str, _: &str) {
        self.row(field);
    }

    fn edit_float(&mut self, field: &str, _: &str, v: &mut f32, _: f32, _: f32, _: f32) -> bool {
        self.row(field);
        let hit = self.set_float.iter().find(|(s, _)| field.ends_with(s)).map(|(_, x)| *x);
        self.ended = hit.is_some();
        match hit {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_float3(&mut self, field: &str, _: &str, v: &mut [f32; 3], _: f32) -> bool {
        self.row(field);
        let hit = self.set_trio.iter().find(|(s, _)| field.ends_with(s)).map(|(_, x)| *x);
        self.ended = hit.is_some();
        match hit {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_float2(&mut self, field: &str, _: &str, _: &mut [f32; 2], _: f32) -> bool {
        self.row(field);
        false
    }

    fn edit_int(&mut self, field: &str, _: &str, _: &mut i64, _: f32) -> bool {
        self.row(field);
        false
    }

    fn edit_color3(&mut self, field: &str, _: &str, _: &mut [f32; 3]) -> bool {
        self.row(field);
        false
    }

    fn edit_color4(&mut self, field: &str, _: &str, _: &mut [f32; 4]) -> bool {
        self.row(field);
        false
    }

    fn edit_bool(&mut self, field: &str, _: &str, v: &mut bool) -> bool {
        self.row(field);
        let hit = self.toggle.iter().any(|s| field.ends_with(s));
        self.ended = hit;
        if hit {
            *v = !*v;
        }

        hit
    }

    fn edit_string(&mut self, field: &str, _: &str, _: &mut String) -> bool {
        self.row(field);
        false
    }

    fn edit_choice(&mut self, field: &str, _: &str, _: &mut i32, _: &[String]) -> bool {
        self.row(field);
        false
    }

    fn gesture_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }
}

fn document() -> XmlFileDocument {
    XmlFileDocument::new(Source::default(), FILE.as_bytes().to_vec(), PathBuf::new()).expect("parses")
}

fn saved(d: &XmlFileDocument) -> String {
    String::from_utf8(d.xml().save()).expect("utf-8")
}

#[test]
fn sections_by_effect_with_tooltips() {
    let mut d = document();
    let mut s = Script::default();
    d.inspect_environment(&mut s);
    for title in ["Sun", "Ambient light", "Fog", "Exposure and tone mapping", "Colour grading", "Depth of field", "Water", "Wind and snow", "Other attributes"] {
        assert!(s.sections.iter().any(|t| t == title), "section {title} in {:?}", s.sections);
    }

    // The file has no SKYGEN: the Sky section says so instead of inventing rows.
    assert!(s.rows.iter().any(|r| r == "missing"));
    assert!(!s.rows.iter().any(|r| r.contains("sky_colour")));

    // No two rows share an id (Dear ImGui would route a click to the wrong widget).
    let mut ids = s.rows.iter().filter(|r| *r != "missing").collect::<Vec<_>>();
    ids.sort();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "row ids are unique");

    // Every row of a setting has a tooltip.
    for (row, hint) in s.rows.iter().zip(&s.hints) {
        if row != "missing" {
            assert!(!hint.is_empty(), "{row} has a tooltip");
        }
    }

    // What the file lacks is dim: ambient_scale is in no element of the file's LIGHTING; sun_colour is there.
    assert!(s.dims.iter().any(|r| r.ends_with("ambient_scale")));
    assert!(!s.dims.iter().any(|r| r.ends_with("sun_colour")));

    // The legacy single tone bias is listed apart, with why the battle game ignores it.
    let other = s.rows.iter().position(|r| r.starts_with("other.") && r.ends_with(".low_tones_bias")).expect("legacy row");
    assert!(s.hints[other].contains("Not read by the battle game"));
    assert!(!d.modified());
}

#[test]
fn a_toggle_flips_the_file_and_undoes() {
    let mut d = document();
    let mut s = Script { toggle: vec!["use_DOF"], ..Default::default() };
    d.inspect_environment(&mut s);
    assert_eq!(saved(&d), FILE.replace("use_DOF='false'", "use_DOF='true'"));
    assert!(d.modified());
    assert_eq!(d.history().size(), 1);

    // A second click is a second entry (a toggle never merges into the one before it).
    let mut s = Script { toggle: vec!["use_DOF"], ..Default::default() };
    d.inspect_environment(&mut s);
    assert_eq!(saved(&d), FILE);
    assert_eq!(d.history().size(), 2);

    assert!(d.history_mut().undo());
    assert!(saved(&d).contains("use_DOF='true'"));
    assert!(d.history_mut().undo());
    assert_eq!(saved(&d), FILE);
}

#[test]
fn a_setting_the_file_lacks_is_added_when_edited() {
    let mut d = document();
    let mut s = Script { set_float: vec![("fog_height_top", 12.5)], ..Default::default() };
    d.inspect_environment(&mut s);
    assert!(s.dims.iter().any(|r| r.ends_with("fog_height_top")));
    assert!(saved(&d).contains("fog_height_top='12.5'"), "{}", saved(&d));

    // The next frames of the drag edit the attribute that now exists, as one more undo entry.
    let before = d.history().size();
    for x in [13.0, 14.0, 15.0] {
        let mut s = Script { set_float: vec![("fog_height_top", x)], ..Default::default() };
        d.inspect_environment(&mut s);
    }

    assert!(saved(&d).contains("fog_height_top='15'"));
    assert!(d.history().size() <= before + 3);
    while d.history_mut().undo() {}
    assert_eq!(saved(&d), FILE);
}

#[test]
fn whole_numbers_stay_integers() {
    let mut d = document();
    let mut s = Script { set_float: vec![("fog_distance_strength", 4.0)], ..Default::default() };
    d.inspect_environment(&mut s);
    assert!(saved(&d).contains("fog_distance_strength='4'"));

    let mut s = Script { set_float: vec![("fog_distance_strength", 2.5)], ..Default::default() };
    d.inspect_environment(&mut s);
    assert!(saved(&d).contains("fog_distance_strength='2.5'"));
}

#[test]
fn three_channel_rows_write_only_the_changed_channel() {
    let mut d = document();
    let mut s = Script { set_trio: vec![("brightness", [1.0, 1.2, 1.0])], ..Default::default() };
    d.inspect_environment(&mut s);
    assert_eq!(saved(&d), FILE.replace("brightness_g='1'", "brightness_g='1.2'"));

    // A channel the file lacks (the HDR's green tone bias) is added alone.
    let mut s = Script { set_trio: vec![("low_tones_bias", [0.4, 0.25, 0.5])], ..Default::default() };
    d.inspect_environment(&mut s);
    let out = saved(&d);
    assert!(out.contains("low_tones_bias_green='0.25'") && out.contains("low_tones_bias_blue='0.5'") && out.contains("low_tones_bias_red='0.4'"), "{out}");
}
