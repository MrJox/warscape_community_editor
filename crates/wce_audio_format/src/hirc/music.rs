//! The interactive music objects (kinds 10 to 13), bank version 112.
//!
//! Segments, switch containers and playlist containers share a [`MusicNode`]: flags, the node parameters, children, the meter
//! (23 bytes) and stingers. Switch and playlist containers add transition rules; a segment adds its duration and markers. A track
//! holds its sources, its clips on the timeline, clip automation, the node parameters and, for a switch track, its switch.

use super::{CurvePoint, NodeBase, Source, TreeNode, read_node, read_point, read_source, read_tree, write_node, write_source, write_tree};
use crate::Result;
use crate::io::{Put, Reader};

/// What every music segment and container shares.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicNode {
    pub flags: u8,
    pub node: NodeBase,
    pub children: Vec<u32>,
    pub meter: Meter,
    pub stingers: Vec<Stinger>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    pub grid_period: f64,
    pub grid_offset: f64,
    pub tempo: f32,
    pub beats_per_bar: u8,
    pub beat_value: u8,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stinger {
    pub trigger_id: u32,
    pub segment_id: u32,
    pub sync_play_at: u32,
    pub cue_filter_hash: u32,
    pub dont_repeat_time: i32,
    pub segment_look_ahead: u32,
}

/// When moving from one music object to another, how to leave the source and enter the destination. The source rule (21
/// bytes), destination rule (24 bytes) and transition segment (30 bytes) are kept as bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionRule {
    pub sources: Vec<u32>,
    pub destinations: Vec<u32>,
    pub source_rule: [u8; 21],
    pub destination_rule: [u8; 24],
    pub transition_flag: u8,
    /// Present when the flag is 1.
    pub transition_object: Option<[u8; 30]>,
}

/// Kind 10: a music segment: tracks (its children) on a timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicSegment {
    pub base: MusicNode,
    /// Milliseconds.
    pub duration: f64,
    pub markers: Vec<Marker>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub id: u32,
    pub position: f64,
    pub name: Vec<u8>,
}

/// Kind 11: a music track: its sources and where they play.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicTrack {
    pub flags: u8,
    pub sources: Vec<Source>,
    pub clips: Vec<TrackClip>,
    /// Present when there are clips.
    pub sub_tracks: Option<u32>,
    pub automation: Vec<ClipAutomation>,
    pub node: NodeBase,
    /// 0 normal, 1 random, 2 sequence, 3 switch.
    pub track_type: u8,
    /// Present for a switch track (type 3).
    pub switch: Option<TrackSwitch>,
    pub look_ahead_time: i32,
}

/// A source placed on a track's timeline (times in milliseconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackClip {
    pub track_id: u32,
    pub source_id: u32,
    pub play_at: f64,
    pub begin_trim: f64,
    pub end_trim: f64,
    pub source_duration: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipAutomation {
    pub clip_index: u32,
    pub kind: u32,
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackSwitch {
    pub group_type: u8,
    pub group_id: u32,
    pub default_switch: u32,
    pub assignments: Vec<u32>,
    /// Source transition time, fade curve, fade offset, sync type, cue filter, destination transition time, fade curve, fade offset.
    pub transition: [u32; 8],
}

/// Kind 12: a music switch container: picks a child by a decision tree over switch and state groups.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicSwitch {
    pub base: MusicNode,
    pub rules: Vec<TransitionRule>,
    pub continue_playback: u8,
    pub arguments: Vec<(u32, u8)>,
    pub mode: u8,
    pub nodes: Vec<TreeNode>,
}

/// Kind 13: a music playlist container: plays its segments by a playlist tree.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicPlaylist {
    pub base: MusicNode,
    pub rules: Vec<TransitionRule>,
    pub items: Vec<PlaylistItem>,
}

/// One item of a playlist tree, depth first: a segment (leaf) or a group of `children` items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaylistItem {
    pub segment_id: u32,
    pub item_id: u32,
    pub children: u32,
    /// The group's kind: sequence or random variants.
    pub kind: u32,
    pub loop_count: i16,
    pub loop_min: i16,
    pub loop_max: i16,
    pub weight: u32,
    pub avoid_repeat_count: u16,
    pub use_weight: u8,
    pub shuffle: u8,
}

fn ids32(r: &mut Reader) -> Result<Vec<u32>> {
    let n = r.count(4)?;
    (0..n).map(|_| r.u32()).collect()
}

fn put_ids32(w: &mut Vec<u8>, ids: &[u32]) {
    w.put_u32(ids.len() as u32);
    ids.iter().for_each(|id| w.put_u32(*id));
}

fn read_music_node(r: &mut Reader) -> Result<MusicNode> {
    let flags = r.u8()?;
    let node = read_node(r)?;
    let children = ids32(r)?;
    let meter = Meter {
        grid_period: r.f64()?,
        grid_offset: r.f64()?,
        tempo: r.f32()?,
        beats_per_bar: r.u8()?,
        beat_value: r.u8()?,
        flags: r.u8()?,
    };
    let n = r.count(24)?;
    let stingers = (0..n)
        .map(|_| {
            Ok(Stinger {
                trigger_id: r.u32()?,
                segment_id: r.u32()?,
                sync_play_at: r.u32()?,
                cue_filter_hash: r.u32()?,
                dont_repeat_time: r.u32()? as i32,
                segment_look_ahead: r.u32()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(MusicNode { flags, node, children, meter, stingers })
}

fn write_music_node(w: &mut Vec<u8>, m: &MusicNode) {
    w.put_u8(m.flags);
    write_node(w, &m.node);
    put_ids32(w, &m.children);
    w.put_f64(m.meter.grid_period);
    w.put_f64(m.meter.grid_offset);
    w.put_f32(m.meter.tempo);
    w.put_u8(m.meter.beats_per_bar);
    w.put_u8(m.meter.beat_value);
    w.put_u8(m.meter.flags);
    w.put_u32(m.stingers.len() as u32);
    for s in &m.stingers {
        w.put_u32(s.trigger_id);
        w.put_u32(s.segment_id);
        w.put_u32(s.sync_play_at);
        w.put_u32(s.cue_filter_hash);
        w.put_u32(s.dont_repeat_time as u32);
        w.put_u32(s.segment_look_ahead);
    }
}

fn read_rules(r: &mut Reader) -> Result<Vec<TransitionRule>> {
    let n = r.count(54)?;
    (0..n)
        .map(|_| {
            let sources = ids32(r)?;
            let destinations = ids32(r)?;
            let source_rule = r.array()?;
            let destination_rule = r.array()?;
            let transition_flag = r.u8()?;
            let transition_object = if transition_flag == 1 { Some(r.array()?) } else { None };
            Ok(TransitionRule { sources, destinations, source_rule, destination_rule, transition_flag, transition_object })
        })
        .collect()
}

fn write_rules(w: &mut Vec<u8>, rules: &[TransitionRule]) {
    w.put_u32(rules.len() as u32);
    for rule in rules {
        put_ids32(w, &rule.sources);
        put_ids32(w, &rule.destinations);
        w.extend_from_slice(&rule.source_rule);
        w.extend_from_slice(&rule.destination_rule);
        w.put_u8(rule.transition_flag);
        if let Some(object) = &rule.transition_object {
            w.extend_from_slice(object);
        }
    }
}

pub(super) fn read_segment(r: &mut Reader) -> Result<MusicSegment> {
    let base = read_music_node(r)?;
    let duration = r.f64()?;
    let n = r.count(16)?;
    let markers = (0..n)
        .map(|_| {
            let (id, position) = (r.u32()?, r.f64()?);
            let len = r.count(1)?;
            Ok(Marker { id, position, name: r.take(len)?.to_vec() })
        })
        .collect::<Result<_>>()?;
    Ok(MusicSegment { base, duration, markers })
}

pub(super) fn write_segment(w: &mut Vec<u8>, s: &MusicSegment) {
    write_music_node(w, &s.base);
    w.put_f64(s.duration);
    w.put_u32(s.markers.len() as u32);
    for m in &s.markers {
        w.put_u32(m.id);
        w.put_f64(m.position);
        w.put_u32(m.name.len() as u32);
        w.extend_from_slice(&m.name);
    }
}

pub(super) fn read_track(r: &mut Reader) -> Result<MusicTrack> {
    let flags = r.u8()?;
    let n = r.count(14)?;
    let sources = (0..n).map(|_| read_source(r)).collect::<Result<_>>()?;
    let n = r.count(40)?;
    let clips = (0..n)
        .map(|_| {
            Ok(TrackClip {
                track_id: r.u32()?,
                source_id: r.u32()?,
                play_at: r.f64()?,
                begin_trim: r.f64()?,
                end_trim: r.f64()?,
                source_duration: r.f64()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let sub_tracks = if clips.is_empty() { None } else { Some(r.u32()?) };
    let n = r.count(12)?;
    let automation = (0..n)
        .map(|_| {
            let (clip_index, kind) = (r.u32()?, r.u32()?);
            let points = r.count(12)?;
            Ok(ClipAutomation { clip_index, kind, points: (0..points).map(|_| read_point(r)).collect::<Result<_>>()? })
        })
        .collect::<Result<_>>()?;
    let node = read_node(r)?;
    let track_type = r.u8()?;
    let switch = if track_type == 3 {
        let (group_type, group_id, default_switch) = (r.u8()?, r.u32()?, r.u32()?);
        let assignments = ids32(r)?;
        let mut transition = [0u32; 8];
        for t in &mut transition {
            *t = r.u32()?;
        }
        Some(TrackSwitch { group_type, group_id, default_switch, assignments, transition })
    } else {
        None
    };
    let look_ahead_time = r.u32()? as i32;
    Ok(MusicTrack { flags, sources, clips, sub_tracks, automation, node, track_type, switch, look_ahead_time })
}

pub(super) fn write_track(w: &mut Vec<u8>, t: &MusicTrack) {
    w.put_u8(t.flags);
    w.put_u32(t.sources.len() as u32);
    t.sources.iter().for_each(|s| write_source(w, s));
    w.put_u32(t.clips.len() as u32);
    for c in &t.clips {
        w.put_u32(c.track_id);
        w.put_u32(c.source_id);
        w.put_f64(c.play_at);
        w.put_f64(c.begin_trim);
        w.put_f64(c.end_trim);
        w.put_f64(c.source_duration);
    }
    if let Some(n) = t.sub_tracks {
        w.put_u32(n);
    }
    w.put_u32(t.automation.len() as u32);
    for a in &t.automation {
        w.put_u32(a.clip_index);
        w.put_u32(a.kind);
        w.put_u32(a.points.len() as u32);
        a.points.iter().for_each(|p| super::put_point(w, p));
    }
    write_node(w, &t.node);
    w.put_u8(t.track_type);
    if let Some(s) = &t.switch {
        w.put_u8(s.group_type);
        w.put_u32(s.group_id);
        w.put_u32(s.default_switch);
        put_ids32(w, &s.assignments);
        s.transition.iter().for_each(|v| w.put_u32(*v));
    }
    w.put_u32(t.look_ahead_time as u32);
}

pub(super) fn read_switch(r: &mut Reader) -> Result<MusicSwitch> {
    let base = read_music_node(r)?;
    let rules = read_rules(r)?;
    let continue_playback = r.u8()?;
    let (arguments, mode, nodes) = read_tree(r)?;
    Ok(MusicSwitch { base, rules, continue_playback, arguments, mode, nodes })
}

pub(super) fn write_switch(w: &mut Vec<u8>, s: &MusicSwitch) {
    write_music_node(w, &s.base);
    write_rules(w, &s.rules);
    w.put_u8(s.continue_playback);
    write_tree(w, &s.arguments, s.mode, &s.nodes);
}

pub(super) fn read_playlist(r: &mut Reader) -> Result<MusicPlaylist> {
    let base = read_music_node(r)?;
    let rules = read_rules(r)?;
    let n = r.count(30)?;
    let items = (0..n)
        .map(|_| {
            Ok(PlaylistItem {
                segment_id: r.u32()?,
                item_id: r.u32()?,
                children: r.u32()?,
                kind: r.u32()?,
                loop_count: r.u16()? as i16,
                loop_min: r.u16()? as i16,
                loop_max: r.u16()? as i16,
                weight: r.u32()?,
                avoid_repeat_count: r.u16()?,
                use_weight: r.u8()?,
                shuffle: r.u8()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(MusicPlaylist { base, rules, items })
}

pub(super) fn write_playlist(w: &mut Vec<u8>, p: &MusicPlaylist) {
    write_music_node(w, &p.base);
    write_rules(w, &p.rules);
    w.put_u32(p.items.len() as u32);
    for i in &p.items {
        w.put_u32(i.segment_id);
        w.put_u32(i.item_id);
        w.put_u32(i.children);
        w.put_u32(i.kind);
        w.put_u16(i.loop_count as u16);
        w.put_u16(i.loop_min as u16);
        w.put_u16(i.loop_max as u16);
        w.put_u32(i.weight);
        w.put_u16(i.avoid_repeat_count);
        w.put_u8(i.use_weight);
        w.put_u8(i.shuffle);
    }
}
