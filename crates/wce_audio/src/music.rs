//! Music edits (docs/plans/audio.md, step 6). The game's music, as its banks have it: a playlist container holds a depth-first tree
//! of items: groups (no segment, a child count, a play kind: sequence or random) and leaves (a segment; kind 0xFFFFFFFF). A segment
//! holds tracks; a track one streamed source and a clip on the timeline. A segment's duration is its entry cue's position plus the
//! clip's end; its markers are the entry cue and the exit cue (at the duration).
//!
//! New audio is streamed from a loose file, as for sounds (the game does not take edited data banks from a mod pack).

use wce_audio_format::bank::HircObject;
use wce_audio_format::hirc::{MusicSegment, MusicTrack, Object, PlaylistItem};
use wce_audio_format::wem::Wem;

use crate::edit::EditSet;
use crate::library::SoundLibrary;
use crate::{AudioError, Result};

/// The exit cue's marker ID in the game's segments (the entry cue is 43573010).
pub const EXIT_CUE: u32 = 1_539_036_744;
/// The entry cue's marker ID.
pub const ENTRY_CUE: u32 = 43_573_010;

/// A leaf item's kind.
const LEAF: u32 = u32::MAX;

/// A WEM's length in milliseconds.
fn length_ms(wem: &[u8]) -> Result<f64> {
    let wem = Wem::read(wem)?;
    let rate = wem.format().map_or(0, |f| f.sample_rate);
    let frames = wem.vorbis().map_or(0, |v| v.total_frames);
    if rate == 0 {
        return Err(AudioError::Invalid("a sound without a rate".into()));
    }
    Ok(f64::from(frames) * 1000.0 / f64::from(rate))
}

/// The index of the group holding item `leaf` in a depth-first item list.
fn parent_group(items: &[PlaylistItem], leaf: usize) -> Option<usize> {
    // (group index, children still to come)
    let mut stack: Vec<(usize, u32)> = vec![];
    for (i, item) in items.iter().enumerate() {
        while stack.last().is_some_and(|(_, left)| *left == 0) {
            stack.pop();
        }
        let parent = stack.last().map(|(g, _)| *g);
        if let Some(top) = stack.last_mut() {
            top.1 -= 1;
        }
        if i == leaf {
            return parent;
        }
        if item.children > 0 {
            stack.push((i, item.children));
        }
    }
    None
}

impl EditSet {
    fn segment(&self, lib: &SoundLibrary, id: u32) -> Result<MusicSegment> {
        match self.object(lib, id) {
            Some(Object::MusicSegment(s)) => Ok(s),
            _ => Err(AudioError::Invalid(format!("#{id} is not a music segment"))),
        }
    }

    fn track(&self, lib: &SoundLibrary, id: u32) -> Result<MusicTrack> {
        match self.object(lib, id) {
            Some(Object::MusicTrack(t)) => Ok(t),
            _ => Err(AudioError::Invalid(format!("#{id} is not a music track"))),
        }
    }

    /// Fits a segment and its first track to new audio: the track's source becomes `media` (a loose file), its clip the whole
    /// sound, the segment's duration and exit cue its end.
    fn fit_segment(&mut self, lib: &SoundLibrary, segment_id: u32, media: u32, length: f64) -> Result<()> {
        let mut segment = self.segment(lib, segment_id)?;
        let track_id = *segment.base.children.first().ok_or_else(|| AudioError::Invalid("a segment without tracks".into()))?;
        let mut track = self.track(lib, track_id)?;
        let source = track.sources.first_mut().ok_or_else(|| AudioError::Invalid("a track without a source".into()))?;
        source.source_id = media;
        source.stream_type = 2;
        source.file_id = media;
        source.file_offset = None;
        source.in_memory_size = 0;
        track.sources.truncate(1);
        let play_at = track.clips.first().map_or(0.0, |c| c.play_at);
        if let Some(clip) = track.clips.first_mut() {
            clip.source_id = media;
            clip.begin_trim = 0.0;
            clip.end_trim = 0.0;
            clip.source_duration = length;
        }
        track.clips.truncate(1);
        self.put_object(lib, track_id, &Object::MusicTrack(track))?;

        let entry = segment.markers.iter().find(|m| m.id == ENTRY_CUE).map_or(0.0, |m| m.position);
        segment.duration = entry + play_at + length;
        for m in &mut segment.markers {
            if m.id == EXIT_CUE || m.position > segment.duration {
                m.position = segment.duration;
            }
        }
        self.put_object(lib, segment_id, &Object::MusicSegment(segment))
    }

    /// Replaces a music segment's audio (its first track), fitting the segment's duration and exit cue to the new sound.
    ///
    /// # Errors
    ///
    /// When the object is not a segment with a track and a source, or the WEM has no length.
    pub fn replace_segment_audio(&mut self, lib: &SoundLibrary, segment_id: u32, wem: &[u8]) -> Result<u32> {
        let length = length_ms(wem)?;
        let media = self.new_id(lib, segment_id);
        self.fit_segment(lib, segment_id, media, length)?;
        self.loose.insert(format!("audio/{media}.wem"), wem.to_vec());
        Ok(media)
    }

    /// Adds a sound to a playlist container: a new segment and track copied from the segment of item `like` (a leaf), the new leaf
    /// put right after it in the same group. Returns the new segment's ID.
    ///
    /// # Errors
    ///
    /// When the object is not a playlist, `like` is not one of its leaves, or the WEM has no length.
    pub fn add_playlist_entry(&mut self, lib: &SoundLibrary, playlist_id: u32, like: u32, wem: &[u8]) -> Result<u32> {
        let length = length_ms(wem)?;
        let Some(Object::MusicPlaylist(mut playlist)) = self.object(lib, playlist_id) else {
            return Err(AudioError::Invalid(format!("#{playlist_id} is not a music playlist")));
        };
        let at = playlist
            .items
            .iter()
            .position(|i| i.item_id == like && i.children == 0 && i.segment_id != 0)
            .ok_or_else(|| AudioError::Invalid(format!("item {like} is not a segment of the playlist")))?;
        let template_segment = playlist.items[at].segment_id;
        let mut segment = self.segment(lib, template_segment)?;
        let template_track = *segment.base.children.first().ok_or_else(|| AudioError::Invalid("a segment without tracks".into()))?;
        let mut track = self.track(lib, template_track)?;

        let (segment_id, track_id, item_id, media) = {
            let s = self.new_id(lib, playlist_id);
            let t = self.new_id(lib, s);
            let i = self.new_id(lib, t);
            let m = self.new_id(lib, i);
            (s, t, i, m)
        };
        track.node.parent_id = segment_id;
        segment.base.node.parent_id = playlist_id;
        segment.base.children = vec![track_id];

        // The copies go right after the template's objects (tracks before segments before the playlist, as in the game's banks).
        let (tb, ti) = self.find(lib, template_track).ok_or_else(|| AudioError::Invalid("the template track is in no bank".into()))?;
        self.bank_mut(lib, tb).objects_mut().insert(ti + 1, HircObject { kind: 11, id: track_id, body: Object::MusicTrack(track).write() });
        let (sb, si) = self.find(lib, template_segment).ok_or_else(|| AudioError::Invalid("the template segment is in no bank".into()))?;
        self.bank_mut(lib, sb)
            .objects_mut()
            .insert(si + 1, HircObject { kind: 10, id: segment_id, body: Object::MusicSegment(segment).write() });
        self.loose.insert(format!("audio/{media}.wem"), wem.to_vec());
        self.fit_segment(lib, segment_id, media, length)?;

        let mut leaf = playlist.items[at];
        leaf.segment_id = segment_id;
        leaf.item_id = item_id;
        if let Some(group) = parent_group(&playlist.items, at) {
            playlist.items[group].children += 1;
        }
        playlist.items.insert(at + 1, leaf);
        let pos = playlist.base.children.partition_point(|c| *c < segment_id);
        playlist.base.children.insert(pos, segment_id);
        self.put_object(lib, playlist_id, &Object::MusicPlaylist(playlist))?;
        Ok(segment_id)
    }

    /// Removes a leaf item from a playlist; its segment (and the segment's tracks) go too when no other item plays it.
    ///
    /// # Errors
    ///
    /// When the object is not a playlist or the item is not one of its leaves.
    pub fn remove_playlist_entry(&mut self, lib: &SoundLibrary, playlist_id: u32, item_id: u32) -> Result<()> {
        let Some(Object::MusicPlaylist(mut playlist)) = self.object(lib, playlist_id) else {
            return Err(AudioError::Invalid(format!("#{playlist_id} is not a music playlist")));
        };
        let at = playlist
            .items
            .iter()
            .position(|i| i.item_id == item_id && i.children == 0)
            .ok_or_else(|| AudioError::Invalid(format!("item {item_id} is not a leaf of the playlist")))?;
        if let Some(group) = parent_group(&playlist.items, at) {
            playlist.items[group].children -= 1;
        }
        let segment_id = playlist.items.remove(at).segment_id;
        let still = playlist.items.iter().any(|i| i.segment_id == segment_id);
        if !still {
            playlist.base.children.retain(|c| *c != segment_id);
        }
        self.put_object(lib, playlist_id, &Object::MusicPlaylist(playlist))?;

        if !still && let Ok(segment) = self.segment(lib, segment_id) {
            if segment.base.node.parent_id != playlist_id {
                return Ok(());
            }
            for id in segment.base.children.iter().copied().chain(std::iter::once(segment_id)) {
                if let Some((b, i)) = self.find(lib, id) {
                    self.bank_mut(lib, b).objects_mut().remove(i);
                }
            }
        }
        Ok(())
    }

    /// Sets a playlist item's weight (1/1000 %: 50,000 the default) and loop count (0: loop forever).
    ///
    /// # Errors
    ///
    /// When the object is not a playlist or has no such item.
    pub fn set_playlist_item(&mut self, lib: &SoundLibrary, playlist_id: u32, item_id: u32, weight: u32, loop_count: i16) -> Result<()> {
        let Some(Object::MusicPlaylist(mut playlist)) = self.object(lib, playlist_id) else {
            return Err(AudioError::Invalid(format!("#{playlist_id} is not a music playlist")));
        };
        let item =
            playlist.items.iter_mut().find(|i| i.item_id == item_id).ok_or_else(|| AudioError::Invalid(format!("no item {item_id}")))?;
        item.weight = weight;
        item.loop_count = loop_count;
        self.put_object(lib, playlist_id, &Object::MusicPlaylist(playlist))
    }
}

/// The items of a playlist as a tree for display: (depth, item).
pub fn item_tree(items: &[PlaylistItem]) -> Vec<(usize, PlaylistItem)> {
    let mut out = vec![];
    let mut stack: Vec<u32> = vec![];
    for item in items {
        while stack.last() == Some(&0) {
            stack.pop();
        }
        if let Some(top) = stack.last_mut() {
            *top -= 1;
        }
        out.push((stack.len(), *item));
        if item.children > 0 {
            stack.push(item.children);
        }
    }
    out
}

/// A group item's play kind, readably (`kind` of a group; leaves are `u32::MAX`).
pub fn group_kind(kind: u32) -> &'static str {
    match kind {
        0 => "sequence, continuous",
        1 => "sequence, step",
        2 => "random, continuous",
        3 => "random, step",
        LEAF => "segment",
        _ => "group",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(segment_id: u32, item_id: u32, children: u32) -> PlaylistItem {
        PlaylistItem {
            segment_id,
            item_id,
            children,
            kind: if children == 0 { LEAF } else { 3 },
            loop_count: 1,
            loop_min: 0,
            loop_max: 0,
            weight: 50_000,
            avoid_repeat_count: 0,
            use_weight: 0,
            shuffle: 0,
        }
    }

    #[test]
    fn groups_of_items() {
        // root(2) -> [group(2) -> a, b], [group(1) -> c]
        let items = [item(0, 1, 2), item(0, 2, 2), item(10, 3, 0), item(11, 4, 0), item(0, 5, 1), item(12, 6, 0)];
        assert_eq!(parent_group(&items, 1), Some(0));
        assert_eq!(parent_group(&items, 3), Some(1));
        assert_eq!(parent_group(&items, 5), Some(4));
        assert_eq!(item_tree(&items).iter().map(|(d, _)| *d).collect::<Vec<_>>(), vec![0, 1, 2, 2, 1, 2]);
    }
}
