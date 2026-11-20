//! Animation sounds (docs/plans/audio.md, step 7): an animation's `.snd.meta` (the `.meta` layout, `wce_formats::anim_meta`) holds
//! `SOUND_TRIGGER` entries: at a time of the clip, a 0-based row of `sound_animation_triggers.csv`, which names the events fired
//! in battle and on the campaign map. Editing writes the `.snd.meta` again (`Meta::encode`, byte for byte when unchanged).

use wce_audio_format::triggers::Triggers;
use wce_formats::anim_meta::{Meta, MetaEntry, MetaField};

/// The entry kind a sound frame is.
pub const SOUND_TRIGGER: &str = "SOUND_TRIGGER";

/// A sound frame of a `.snd.meta`, read for display.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundFrame {
    /// The entry's index in the file.
    pub entry: usize,
    /// Seconds into the clip.
    pub time: f32,
    pub trigger: usize,
}

/// The sound frames of a decoded `.snd.meta`.
pub fn frames(meta: &Meta) -> Vec<SoundFrame> {
    meta.entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == SOUND_TRIGGER)
        .filter_map(|(i, e)| Some(SoundFrame { entry: i, time: e.start, trigger: e.u32(0)? as usize }))
        .collect()
}

/// A trigger row's name and its battle and campaign events.
pub fn trigger_events(triggers: &Triggers, row: usize) -> (String, String, String) {
    let get = |c: &str| triggers.get(row, c).unwrap_or_default().to_owned();
    (get("name"), get("sound_event_name_battle"), get("sound_event_name_campaign"))
}

/// Adds a sound frame (a `SOUND_TRIGGER` at `time`, version 2 as the game's files) and returns its entry index.
pub fn add_frame(meta: &mut Meta, time: f32, trigger: usize) -> usize {
    let entry = MetaEntry {
        kind: SOUND_TRIGGER.to_owned(),
        version: Some(2),
        start: time,
        end: time,
        tag: String::new(),
        fields: vec![MetaField::U32(trigger as u32), MetaField::U32(0)],
    };
    let at = meta.entries.iter().position(|e| e.kind == SOUND_TRIGGER && e.start > time).unwrap_or(meta.entries.len());
    meta.entries.insert(at, entry);
    at
}

/// Moves a sound frame to another time.
pub fn move_frame(meta: &mut Meta, entry: usize, time: f32) {
    if let Some(e) = meta.entries.get_mut(entry) {
        e.start = time;
        e.end = time;
    }
}

/// Sets a sound frame's trigger row.
pub fn set_trigger(meta: &mut Meta, entry: usize, trigger: usize) {
    if let Some(e) = meta.entries.get_mut(entry)
        && let Some(MetaField::U32(v)) = e.fields.first_mut()
    {
        *v = trigger as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut meta = Meta { version: 2, entries: vec![] };
        add_frame(&mut meta, 0.5, 84);
        add_frame(&mut meta, 0.2, 92);
        let bytes = meta.encode();
        let back = Meta::decode(&bytes).unwrap();
        let f = frames(&back);
        assert_eq!(f.iter().map(|f| (f.time, f.trigger)).collect::<Vec<_>>(), vec![(0.2, 92), (0.5, 84)]);
        let mut back = back;
        move_frame(&mut back, 0, 0.3);
        set_trigger(&mut back, 0, 93);
        assert_eq!(frames(&back)[0], SoundFrame { entry: 0, time: 0.3, trigger: 93 });
    }
}
