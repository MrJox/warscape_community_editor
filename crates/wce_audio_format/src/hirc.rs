//! Typed views of the hierarchy's objects (bank version 112). A [`crate::bank::HircObject`] keeps its body as bytes; [`parse`] reads
//! it into an [`Object`] and [`Object::write`] gives the body back, byte for byte for every object the gate has seen. Kinds not typed
//! yet ([`parse`] returns `None`) stay bytes. Layouts: docs/formats/wwise_bank.md.

use crate::io::{Put, Reader};
use crate::{FormatError, Result};

/// A typed hierarchy object (its ID is the [`crate::bank::HircObject`]'s).
#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    State(State),
    Sound(Sound),
    Action(Action),
    Event(Event),
    RandomSequence(RandomSequence),
    Switch(Switch),
    ActorMixer(ActorMixer),
    Blend(Blend),
    /// Kinds 8 (bus) and 20 (auxiliary bus).
    Bus(Bus),
    Attenuation(Attenuation),
    DialogueEvent(DialogueEvent),
    /// Kinds 18 (effect share set) and 19 (effect).
    Effect(Effect),
    MusicSegment(MusicSegment),
    MusicTrack(MusicTrack),
    MusicSwitch(MusicSwitch),
    MusicPlaylist(MusicPlaylist),
}

mod music;
pub use music::{
    ClipAutomation, Marker, Meter, MusicNode, MusicPlaylist, MusicSegment, MusicSwitch, MusicTrack, PlaylistItem, Stinger, TrackClip,
    TrackSwitch, TransitionRule,
};

/// Kinds 8 and 20: a mixing bus (`aux` for an auxiliary bus).
#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub aux: bool,
    pub parent_bus_id: u32,
    /// Absent in a bus that is only its parent's ID.
    pub params: Option<BusParams>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BusParams {
    pub props: Vec<(u8, f32)>,
    pub positioning: Positioning,
    pub advanced_bits: u8,
    pub max_instances: u16,
    pub channel_config: u32,
    pub hdr_bits: u8,
    pub recovery_time: i32,
    pub max_duck_volume: f32,
    pub ducks: Vec<Duck>,
    pub effects: Vec<EffectSlot>,
    /// Bypass bits of the effects; written only when there are effects.
    pub effects_bypass: u8,
    pub override_attachment: u8,
    pub effect_0_id: u32,
    pub effect_0_is_share_set: u8,
    pub rtpcs: Vec<Rtpc>,
    pub state_groups: Vec<StateGroupRef>,
}

/// A bus ducking another while it plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duck {
    pub bus_id: u32,
    pub volume: f32,
    pub fade_out_time: i32,
    pub fade_in_time: i32,
    pub fade_curve: u8,
    pub target_prop: u8,
}

/// Kind 14: distance attenuation: a cone, curves, and which curve each property uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Attenuation {
    pub cone_enabled: u8,
    /// Inside angle, outside angle, outside volume, low-pass, high-pass; present when the cone is enabled.
    pub cone: Option<[f32; 5]>,
    /// Per property (volume, aux volumes, low-pass, spread...), the curve's index or 255 for none.
    pub curve_to_use: [u8; 7],
    /// (scaling, points).
    pub curves: Vec<(u8, Vec<CurvePoint>)>,
    pub rtpcs: Option<Vec<Rtpc>>,
}

/// Kind 15: a dialogue event: picks a sound from a decision tree over state and switch groups.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogueEvent {
    pub probability: u8,
    /// The groups the tree decides on, in depth order: (group ID, group type).
    pub arguments: Vec<(u32, u8)>,
    pub mode: u8,
    /// The tree's nodes, breadth first, 12 bytes each: (key, children (first u16, count u16) or the audio node's ID at a leaf,
    /// weight, probability).
    pub nodes: Vec<TreeNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub key: u32,
    pub value: u32,
    pub weight: u16,
    pub probability: u16,
}

/// Kinds 18 and 19: an effect plug-in's parameters (`share_set` for kind 18).
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub share_set: bool,
    pub plugin_id: u32,
    pub params: Vec<u8>,
    /// (index, media source ID).
    pub media: Vec<(u8, u32)>,
    pub rtpcs: Option<Vec<Rtpc>>,
    pub rtpc_init_flags: Option<u16>,
}

/// Kind 1: a state's property overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub props: Vec<(u8, f32)>,
}

/// Kind 2: one sound and where its media is.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub source: Source,
    pub node: NodeBase,
}

/// A sound's media.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    /// The codec or source plugin (0x40001 Vorbis; the low nibble 2 or 5 means the plugin has parameters).
    pub plugin_id: u32,
    /// 0 in the bank's `DATA`, 1 prefetched and streamed, 2 streamed (a loose `audio/<id>.wem`).
    pub stream_type: u8,
    pub source_id: u32,
    /// The bank holding the media, or the media's ID for a streamed file.
    pub file_id: u32,
    /// Present unless streamed (`stream_type` 2).
    pub file_offset: Option<u32>,
    pub in_memory_size: u32,
    pub source_bits: u8,
    /// A plugin's parameter block (with its u32 size), for plugins that have one.
    pub plugin_params: Option<Vec<u8>>,
}

/// Kind 3: an action an event runs (play, stop, set state...). Only its type and target are typed; the parameters that follow
/// depend on the type and are kept as bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// High byte the action (0x04 play, 0x01 stop, 0x12 set state...), low byte its scope.
    pub action_type: u16,
    pub target_id: u32,
    pub target_is_bus: u8,
    pub params: Vec<u8>,
}

/// Kind 4: an event: the actions it runs, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub actions: Vec<u32>,
}

/// Kind 5: plays one of its children at random, or in sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct RandomSequence {
    pub node: NodeBase,
    pub loop_count: u16,
    pub loop_mod_min: u16,
    pub loop_mod_max: u16,
    pub transition_time: f32,
    pub transition_time_mod_min: f32,
    pub transition_time_mod_max: f32,
    pub avoid_repeat_count: u16,
    pub transition_mode: u8,
    pub random_mode: u8,
    /// 0 sequence, 1 random.
    pub mode: u8,
    pub bits: u8,
    pub children: Vec<u32>,
    /// (child, weight in 1/1000 %: 50,000 is the default).
    pub playlist: Vec<(u32, i32)>,
}

/// Kind 6: plays the children assigned to the current value of a switch or state group.
#[derive(Clone, Debug, PartialEq)]
pub struct Switch {
    pub node: NodeBase,
    /// 0 switch group, 1 state group.
    pub group_type: u8,
    pub group_id: u32,
    pub default_switch: u32,
    pub continuous_validation: u8,
    pub children: Vec<u32>,
    /// (switch value, the children it plays).
    pub assignments: Vec<(u32, Vec<u32>)>,
    pub child_params: Vec<SwitchChildParams>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchChildParams {
    pub node_id: u32,
    pub bits_0: u8,
    pub bits_1: u8,
    pub fade_out_time: f32,
    pub fade_in_time: f32,
}

/// Kind 7: groups children to share properties.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorMixer {
    pub node: NodeBase,
    pub children: Vec<u32>,
}

/// Kind 9: plays all its children together, in layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Blend {
    pub node: NodeBase,
    pub children: Vec<u32>,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: u32,
    pub rtpcs: Vec<Rtpc>,
    pub crossfade_rtpc_id: u32,
    pub crossfade_rtpc_type: u8,
    /// (child, its crossfade curve: (from, to, interpolation)).
    pub associations: Vec<(u32, Vec<CurvePoint>)>,
}

/// The parameters every sound, container and actor-mixer shares.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeBase {
    pub override_effects: u8,
    pub effects: Vec<EffectSlot>,
    /// Bypass bits of the effects; written only when there are effects.
    pub effects_bypass: u8,
    pub override_attachment: u8,
    pub override_bus_id: u32,
    /// The parent in the hierarchy (an actor-mixer or container), 0 for none.
    pub parent_id: u32,
    pub priority_bits: u8,
    /// (property, value): volume, pitch, low-pass... (docs/formats/wwise_bank.md lists the property IDs).
    pub props: Vec<(u8, f32)>,
    /// (property, min, max): randomised ranges.
    pub ranged_props: Vec<(u8, f32, f32)>,
    pub positioning: Positioning,
    pub aux_bits: u8,
    /// Present when the aux bits' bit 3 is set.
    pub aux_buses: Option<[u32; 4]>,
    pub advanced: AdvancedSettings,
    pub state_groups: Vec<StateGroupRef>,
    pub rtpcs: Vec<Rtpc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectSlot {
    pub index: u8,
    pub effect_id: u32,
    pub is_share_set: u8,
    pub rendered: u8,
}

/// 2D/3D positioning. The 3D block is present when bits 0 and 3 are set; its path block when the 3D bits' low two bits are not 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Positioning {
    pub bits: u8,
    pub three_d: Option<Positioning3d>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Positioning3d {
    pub bits: u8,
    pub attenuation_id: u32,
    pub path: Option<Path>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub mode: u8,
    pub transition_time: i32,
    /// (x, y, z, duration).
    pub vertices: Vec<(f32, f32, f32, i32)>,
    /// (first vertex, vertex count) per playlist item, then a random range (x, y, z) per item.
    pub items: Vec<(u32, u32)>,
    pub ranges: Vec<(f32, f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedSettings {
    pub bits: u8,
    pub virtual_queue_behavior: u8,
    pub max_instances: u16,
    pub below_threshold_behavior: u8,
    pub hdr_bits: u8,
}

/// A state group the node reacts to: (state, the state's property object) pairs.
#[derive(Clone, Debug, PartialEq)]
pub struct StateGroupRef {
    pub group_id: u32,
    pub sync_type: u8,
    pub states: Vec<(u32, u32)>,
}

/// A game parameter driving a property along a curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Rtpc {
    pub rtpc_id: u32,
    pub rtpc_type: u8,
    pub accumulation: u8,
    pub param_id: u8,
    pub curve_id: u32,
    pub scaling: u8,
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub from: f32,
    pub to: f32,
    pub interpolation: u32,
}

/// The bank version the typed layouts are for (Attila; Rome 2's patch banks).
pub const VERSION: u32 = 112;

/// Reads an object's body (after its ID) of a bank of the given version. `None`: the version or the kind is not typed (yet).
pub fn parse(bank_version: u32, kind: u8, body: &[u8]) -> Option<Result<Object>> {
    if bank_version != VERSION {
        return None;
    }

    let mut r = Reader::new(body);
    let object = match kind {
        1 => read_state(&mut r).map(Object::State),
        2 => read_sound(&mut r).map(Object::Sound),
        3 => read_action(&mut r).map(Object::Action),
        4 => ids32(&mut r).map(|actions| Object::Event(Event { actions })),
        5 => read_random_sequence(&mut r).map(Object::RandomSequence),
        6 => read_switch(&mut r).map(Object::Switch),
        7 => read_actor_mixer(&mut r).map(Object::ActorMixer),
        9 => read_blend(&mut r).map(Object::Blend),
        8 | 20 => read_bus(&mut r, kind == 20).map(Object::Bus),
        14 => read_attenuation(&mut r).map(Object::Attenuation),
        15 => read_dialogue_event(&mut r).map(Object::DialogueEvent),
        18 | 19 => read_effect(&mut r, kind == 18).map(Object::Effect),
        10 => music::read_segment(&mut r).map(Object::MusicSegment),
        11 => music::read_track(&mut r).map(Object::MusicTrack),
        12 => music::read_switch(&mut r).map(Object::MusicSwitch),
        13 => music::read_playlist(&mut r).map(Object::MusicPlaylist),
        _ => return None,
    };

    Some(
        object.and_then(
            |o| {
                if r.left() == 0 { Ok(o) } else { Err(FormatError::Invalid(format!("{} bytes left after the object", r.left()))) }
            },
        ),
    )
}

impl Object {
    /// The hierarchy kind this object is written as.
    pub fn kind(&self) -> u8 {
        match self {
            Object::State(_) => 1,
            Object::Sound(_) => 2,
            Object::Action(_) => 3,
            Object::Event(_) => 4,
            Object::RandomSequence(_) => 5,
            Object::Switch(_) => 6,
            Object::ActorMixer(_) => 7,
            Object::Blend(_) => 9,
            Object::Bus(b) => {
                if b.aux {
                    20
                } else {
                    8
                }
            }
            Object::Attenuation(_) => 14,
            Object::DialogueEvent(_) => 15,
            Object::MusicSegment(_) => 10,
            Object::MusicTrack(_) => 11,
            Object::MusicSwitch(_) => 12,
            Object::MusicPlaylist(_) => 13,
            Object::Effect(e) => {
                if e.share_set {
                    18
                } else {
                    19
                }
            }
        }
    }

    /// The node parameters, for the kinds that have them.
    pub fn node(&self) -> Option<&NodeBase> {
        match self {
            Object::Sound(o) => Some(&o.node),
            Object::RandomSequence(o) => Some(&o.node),
            Object::Switch(o) => Some(&o.node),
            Object::ActorMixer(o) => Some(&o.node),
            Object::Blend(o) => Some(&o.node),
            Object::MusicSegment(o) => Some(&o.base.node),
            Object::MusicTrack(o) => Some(&o.node),
            Object::MusicSwitch(o) => Some(&o.base.node),
            Object::MusicPlaylist(o) => Some(&o.base.node),
            _ => None,
        }
    }

    /// The object's children in the hierarchy (containers and actor-mixers).
    pub fn children(&self) -> &[u32] {
        match self {
            Object::RandomSequence(o) => &o.children,
            Object::Switch(o) => &o.children,
            Object::ActorMixer(o) => &o.children,
            Object::Blend(o) => &o.children,
            Object::MusicSegment(o) => &o.base.children,
            Object::MusicSwitch(o) => &o.base.children,
            Object::MusicPlaylist(o) => &o.base.children,
            _ => &[],
        }
    }

    /// Writes the body (after the ID).
    pub fn write(&self) -> Vec<u8> {
        let mut w = vec![];
        match self {
            Object::State(o) => put_props(&mut w, &o.props),
            Object::Sound(o) => {
                write_source(&mut w, &o.source);
                write_node(&mut w, &o.node);
            }
            Object::Action(o) => {
                w.put_u16(o.action_type);
                w.put_u32(o.target_id);
                w.put_u8(o.target_is_bus);
                w.extend_from_slice(&o.params);
            }
            Object::Event(o) => put_ids32(&mut w, &o.actions),
            Object::RandomSequence(o) => {
                write_node(&mut w, &o.node);
                w.put_u16(o.loop_count);
                w.put_u16(o.loop_mod_min);
                w.put_u16(o.loop_mod_max);
                w.put_f32(o.transition_time);
                w.put_f32(o.transition_time_mod_min);
                w.put_f32(o.transition_time_mod_max);
                w.put_u16(o.avoid_repeat_count);
                w.put_u8(o.transition_mode);
                w.put_u8(o.random_mode);
                w.put_u8(o.mode);
                w.put_u8(o.bits);
                put_ids32(&mut w, &o.children);
                w.put_u16(o.playlist.len() as u16);
                for (id, weight) in &o.playlist {
                    w.put_u32(*id);
                    w.put_u32(*weight as u32);
                }
            }
            Object::Switch(o) => {
                write_node(&mut w, &o.node);
                w.put_u8(o.group_type);
                w.put_u32(o.group_id);
                w.put_u32(o.default_switch);
                w.put_u8(o.continuous_validation);
                put_ids32(&mut w, &o.children);
                w.put_u32(o.assignments.len() as u32);
                for (switch, nodes) in &o.assignments {
                    w.put_u32(*switch);
                    put_ids32(&mut w, nodes);
                }
                w.put_u32(o.child_params.len() as u32);
                for p in &o.child_params {
                    w.put_u32(p.node_id);
                    w.put_u8(p.bits_0);
                    w.put_u8(p.bits_1);
                    w.put_f32(p.fade_out_time);
                    w.put_f32(p.fade_in_time);
                }
            }
            Object::ActorMixer(o) => {
                write_node(&mut w, &o.node);
                put_ids32(&mut w, &o.children);
            }
            Object::Blend(o) => {
                write_node(&mut w, &o.node);
                put_ids32(&mut w, &o.children);
                w.put_u32(o.layers.len() as u32);
                for l in &o.layers {
                    w.put_u32(l.id);
                    w.put_u16(l.rtpcs.len() as u16);
                    l.rtpcs.iter().for_each(|rtpc| write_rtpc(&mut w, rtpc));
                    w.put_u32(l.crossfade_rtpc_id);
                    w.put_u8(l.crossfade_rtpc_type);
                    w.put_u32(l.associations.len() as u32);
                    for (child, points) in &l.associations {
                        w.put_u32(*child);
                        w.put_u32(points.len() as u32);
                        points.iter().for_each(|p| put_point(&mut w, p));
                    }
                }
            }
            Object::Bus(o) => write_bus(&mut w, o),
            Object::Attenuation(o) => {
                w.put_u8(o.cone_enabled);
                if let Some(cone) = o.cone {
                    cone.iter().for_each(|v| w.put_f32(*v));
                }
                w.extend_from_slice(&o.curve_to_use);
                w.put_u8(o.curves.len() as u8);
                for (scaling, points) in &o.curves {
                    w.put_u8(*scaling);
                    w.put_u16(points.len() as u16);
                    points.iter().for_each(|p| put_point(&mut w, p));
                }
                if let Some(rtpcs) = &o.rtpcs {
                    put_rtpcs(&mut w, rtpcs);
                }
            }
            Object::DialogueEvent(o) => {
                w.put_u8(o.probability);
                write_tree(&mut w, &o.arguments, o.mode, &o.nodes);
            }
            Object::MusicSegment(o) => music::write_segment(&mut w, o),
            Object::MusicTrack(o) => music::write_track(&mut w, o),
            Object::MusicSwitch(o) => music::write_switch(&mut w, o),
            Object::MusicPlaylist(o) => music::write_playlist(&mut w, o),
            Object::Effect(o) => {
                w.put_u32(o.plugin_id);
                w.put_u32(o.params.len() as u32);
                w.extend_from_slice(&o.params);
                w.put_u8(o.media.len() as u8);
                for (index, source) in &o.media {
                    w.put_u8(*index);
                    w.put_u32(*source);
                }
                if let Some(rtpcs) = &o.rtpcs {
                    put_rtpcs(&mut w, rtpcs);
                }
                if let Some(flags) = o.rtpc_init_flags {
                    w.put_u16(flags);
                }
            }
        }

        w
    }
}

/// The paths of a decision tree (dialogue events, music switches) of `depth` levels: (keys, the leaf's value).
pub fn tree_paths(depth: usize, nodes: &[TreeNode]) -> Vec<(Vec<u32>, u32)> {
    let mut out = vec![];
    let mut stack = vec![(0usize, vec![])];
    while let Some((i, keys)) = stack.pop() {
        let Some(n) = nodes.get(i) else { continue };
        if keys.len() == depth {
            if n.value != 0 {
                out.push((keys, n.value));
            }
            continue;
        }
        let (first, count) = ((n.value & 0xFFFF) as usize, (n.value >> 16) as usize);
        for c in (first..first + count).rev() {
            if let Some(child) = nodes.get(c) {
                let mut k = keys.clone();
                k.push(child.key);
                stack.push((c, k));
            }
        }
    }
    out
}

impl DialogueEvent {
    /// Every path of the tree to a leaf: the keys (state or switch IDs, one per argument; 0 for "any") and the audio node played.
    pub fn paths(&self) -> Vec<(Vec<u32>, u32)> {
        tree_paths(self.arguments.len(), &self.nodes)
    }

    /// The audio nodes the tree's leaves play (the nodes at the tree's full depth), in tree order, without repeats.
    pub fn leaves(&self) -> Vec<u32> {
        let mut out = vec![];
        let mut level = vec![0usize];
        for _ in 0..self.arguments.len() {
            let mut next = vec![];
            for &i in &level {
                let Some(n) = self.nodes.get(i) else { continue };
                let (first, count) = ((n.value & 0xFFFF) as usize, (n.value >> 16) as usize);
                next.extend(first..first + count);
            }
            level = next;
        }

        for i in level {
            if let Some(n) = self.nodes.get(i)
                && n.value != 0
                && !out.contains(&n.value)
            {
                out.push(n.value);
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------------------------------------------- readers

fn ids32(r: &mut Reader) -> Result<Vec<u32>> {
    let n = r.count(4)?;
    (0..n).map(|_| r.u32()).collect()
}

fn put_ids32(w: &mut Vec<u8>, ids: &[u32]) {
    w.put_u32(ids.len() as u32);
    ids.iter().for_each(|id| w.put_u32(*id));
}

fn read_props(r: &mut Reader) -> Result<Vec<(u8, f32)>> {
    let n = r.u8()? as usize;
    let ids = r.take(n)?.to_vec();
    ids.into_iter().map(|id| Ok((id, r.f32()?))).collect()
}

fn put_props(w: &mut Vec<u8>, props: &[(u8, f32)]) {
    w.put_u8(props.len() as u8);
    props.iter().for_each(|(id, _)| w.put_u8(*id));
    props.iter().for_each(|(_, v)| w.put_f32(*v));
}

fn read_state(r: &mut Reader) -> Result<State> {
    Ok(State { props: read_props(r)? })
}

fn read_point(r: &mut Reader) -> Result<CurvePoint> {
    Ok(CurvePoint { from: r.f32()?, to: r.f32()?, interpolation: r.u32()? })
}

fn put_point(w: &mut Vec<u8>, p: &CurvePoint) {
    w.put_f32(p.from);
    w.put_f32(p.to);
    w.put_u32(p.interpolation);
}

fn read_rtpc(r: &mut Reader) -> Result<Rtpc> {
    let (rtpc_id, rtpc_type, accumulation, param_id, curve_id, scaling) = (r.u32()?, r.u8()?, r.u8()?, r.u8()?, r.u32()?, r.u8()?);
    let n = r.u16()? as usize;
    let points = (0..n).map(|_| read_point(r)).collect::<Result<_>>()?;
    Ok(Rtpc { rtpc_id, rtpc_type, accumulation, param_id, curve_id, scaling, points })
}

fn write_rtpc(w: &mut Vec<u8>, rtpc: &Rtpc) {
    w.put_u32(rtpc.rtpc_id);
    w.put_u8(rtpc.rtpc_type);
    w.put_u8(rtpc.accumulation);
    w.put_u8(rtpc.param_id);
    w.put_u32(rtpc.curve_id);
    w.put_u8(rtpc.scaling);
    w.put_u16(rtpc.points.len() as u16);
    rtpc.points.iter().for_each(|p| put_point(w, p));
}

fn read_positioning(r: &mut Reader) -> Result<Positioning> {
    let bits = r.u8()?;
    let three_d = if bits & 1 == 1 && (bits >> 3) & 1 == 1 {
        let bits_3d = r.u8()?;
        let attenuation_id = r.u32()?;
        let path = if bits_3d & 3 != 1 {
            let mode = r.u8()?;
            let transition_time = r.u32()? as i32;
            let n = r.count(16)?;
            let vertices = (0..n).map(|_| Ok((r.f32()?, r.f32()?, r.f32()?, r.u32()? as i32))).collect::<Result<_>>()?;
            let n = r.count(20)?;
            let items = (0..n).map(|_| Ok((r.u32()?, r.u32()?))).collect::<Result<_>>()?;
            let ranges = (0..n).map(|_| Ok((r.f32()?, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
            Some(Path { mode, transition_time, vertices, items, ranges })
        } else {
            None
        };
        Some(Positioning3d { bits: bits_3d, attenuation_id, path })
    } else {
        None
    };

    Ok(Positioning { bits, three_d })
}

fn write_positioning(w: &mut Vec<u8>, p: &Positioning) {
    w.put_u8(p.bits);
    if let Some(t) = &p.three_d {
        w.put_u8(t.bits);
        w.put_u32(t.attenuation_id);
        if let Some(path) = &t.path {
            w.put_u8(path.mode);
            w.put_u32(path.transition_time as u32);
            w.put_u32(path.vertices.len() as u32);
            for (x, y, z, d) in &path.vertices {
                w.put_f32(*x);
                w.put_f32(*y);
                w.put_f32(*z);
                w.put_u32(*d as u32);
            }
            w.put_u32(path.items.len() as u32);
            for (first, count) in &path.items {
                w.put_u32(*first);
                w.put_u32(*count);
            }
            for (x, y, z) in &path.ranges {
                w.put_f32(*x);
                w.put_f32(*y);
                w.put_f32(*z);
            }
        }
    }
}

fn read_node(r: &mut Reader) -> Result<NodeBase> {
    let override_effects = r.u8()?;
    let n_fx = r.u8()? as usize;
    let mut effects_bypass = 0;
    let mut effects = vec![];
    if n_fx > 0 {
        effects_bypass = r.u8()?;
        for _ in 0..n_fx {
            effects.push(EffectSlot { index: r.u8()?, effect_id: r.u32()?, is_share_set: r.u8()?, rendered: r.u8()? });
        }
    }

    let (override_attachment, override_bus_id, parent_id, priority_bits) = (r.u8()?, r.u32()?, r.u32()?, r.u8()?);
    let props = read_props(r)?;
    let n = r.u8()? as usize;
    let ids = r.take(n)?.to_vec();
    let ranged_props = ids.into_iter().map(|id| Ok((id, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
    let positioning = read_positioning(r)?;
    let aux_bits = r.u8()?;
    let aux_buses = if (aux_bits >> 3) & 1 == 1 { Some([r.u32()?, r.u32()?, r.u32()?, r.u32()?]) } else { None };
    let advanced = AdvancedSettings {
        bits: r.u8()?,
        virtual_queue_behavior: r.u8()?,
        max_instances: r.u16()?,
        below_threshold_behavior: r.u8()?,
        hdr_bits: r.u8()?,
    };
    let n = r.count(7)?;
    let state_groups = (0..n)
        .map(|_| {
            let (group_id, sync_type) = (r.u32()?, r.u8()?);
            let count = r.u16()? as usize;
            let states = (0..count).map(|_| Ok((r.u32()?, r.u32()?))).collect::<Result<_>>()?;
            Ok(StateGroupRef { group_id, sync_type, states })
        })
        .collect::<Result<_>>()?;
    let n = r.u16()? as usize;
    let rtpcs = (0..n).map(|_| read_rtpc(r)).collect::<Result<_>>()?;
    Ok(NodeBase {
        override_effects,
        effects,
        effects_bypass,
        override_attachment,
        override_bus_id,
        parent_id,
        priority_bits,
        props,
        ranged_props,
        positioning,
        aux_bits,
        aux_buses,
        advanced,
        state_groups,
        rtpcs,
    })
}

fn write_node(w: &mut Vec<u8>, n: &NodeBase) {
    w.put_u8(n.override_effects);
    w.put_u8(n.effects.len() as u8);
    if !n.effects.is_empty() {
        w.put_u8(n.effects_bypass);
        for e in &n.effects {
            w.put_u8(e.index);
            w.put_u32(e.effect_id);
            w.put_u8(e.is_share_set);
            w.put_u8(e.rendered);
        }
    }

    w.put_u8(n.override_attachment);
    w.put_u32(n.override_bus_id);
    w.put_u32(n.parent_id);
    w.put_u8(n.priority_bits);
    put_props(w, &n.props);
    w.put_u8(n.ranged_props.len() as u8);
    n.ranged_props.iter().for_each(|(id, _, _)| w.put_u8(*id));
    for (_, min, max) in &n.ranged_props {
        w.put_f32(*min);
        w.put_f32(*max);
    }

    write_positioning(w, &n.positioning);
    w.put_u8(n.aux_bits);
    if let Some(buses) = n.aux_buses {
        buses.iter().for_each(|b| w.put_u32(*b));
    }

    let a = &n.advanced;
    w.put_u8(a.bits);
    w.put_u8(a.virtual_queue_behavior);
    w.put_u16(a.max_instances);
    w.put_u8(a.below_threshold_behavior);
    w.put_u8(a.hdr_bits);
    w.put_u32(n.state_groups.len() as u32);
    for g in &n.state_groups {
        w.put_u32(g.group_id);
        w.put_u8(g.sync_type);
        w.put_u16(g.states.len() as u16);
        for (state, props) in &g.states {
            w.put_u32(*state);
            w.put_u32(*props);
        }
    }

    w.put_u16(n.rtpcs.len() as u16);
    n.rtpcs.iter().for_each(|rtpc| write_rtpc(w, rtpc));
}

fn read_source(r: &mut Reader) -> Result<Source> {
    let (plugin_id, stream_type, source_id, file_id) = (r.u32()?, r.u8()?, r.u32()?, r.u32()?);
    let file_offset = if stream_type != 2 { Some(r.u32()?) } else { None };
    let (in_memory_size, source_bits) = (r.u32()?, r.u8()?);
    let plugin_params = if matches!(plugin_id & 0x0F, 2 | 5) {
        let n = r.u32()? as usize;
        Some(r.take(n)?.to_vec())
    } else {
        None
    };

    Ok(Source { plugin_id, stream_type, source_id, file_id, file_offset, in_memory_size, source_bits, plugin_params })
}

fn write_source(w: &mut Vec<u8>, s: &Source) {
    w.put_u32(s.plugin_id);
    w.put_u8(s.stream_type);
    w.put_u32(s.source_id);
    w.put_u32(s.file_id);
    if let Some(offset) = s.file_offset {
        w.put_u32(offset);
    }

    w.put_u32(s.in_memory_size);
    w.put_u8(s.source_bits);
    if let Some(params) = &s.plugin_params {
        w.put_u32(params.len() as u32);
        w.extend_from_slice(params);
    }
}

fn read_sound(r: &mut Reader) -> Result<Sound> {
    Ok(Sound { source: read_source(r)?, node: read_node(r)? })
}

fn read_action(r: &mut Reader) -> Result<Action> {
    Ok(Action { action_type: r.u16()?, target_id: r.u32()?, target_is_bus: r.u8()?, params: r.rest().to_vec() })
}

fn read_random_sequence(r: &mut Reader) -> Result<RandomSequence> {
    let node = read_node(r)?;
    let (loop_count, loop_mod_min, loop_mod_max) = (r.u16()?, r.u16()?, r.u16()?);
    let (transition_time, transition_time_mod_min, transition_time_mod_max) = (r.f32()?, r.f32()?, r.f32()?);
    let avoid_repeat_count = r.u16()?;
    let (transition_mode, random_mode, mode, bits) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
    let children = ids32(r)?;
    let n = r.u16()? as usize;
    let playlist = (0..n).map(|_| Ok((r.u32()?, r.u32()? as i32))).collect::<Result<_>>()?;
    Ok(RandomSequence {
        node,
        loop_count,
        loop_mod_min,
        loop_mod_max,
        transition_time,
        transition_time_mod_min,
        transition_time_mod_max,
        avoid_repeat_count,
        transition_mode,
        random_mode,
        mode,
        bits,
        children,
        playlist,
    })
}

fn read_switch(r: &mut Reader) -> Result<Switch> {
    let node = read_node(r)?;
    let (group_type, group_id, default_switch, continuous_validation) = (r.u8()?, r.u32()?, r.u32()?, r.u8()?);
    let children = ids32(r)?;
    let n = r.count(8)?;
    let assignments = (0..n).map(|_| Ok((r.u32()?, ids32(r)?))).collect::<Result<_>>()?;
    let n = r.count(14)?;
    let child_params = (0..n)
        .map(|_| {
            Ok(SwitchChildParams { node_id: r.u32()?, bits_0: r.u8()?, bits_1: r.u8()?, fade_out_time: r.f32()?, fade_in_time: r.f32()? })
        })
        .collect::<Result<_>>()?;
    Ok(Switch { node, group_type, group_id, default_switch, continuous_validation, children, assignments, child_params })
}

fn read_actor_mixer(r: &mut Reader) -> Result<ActorMixer> {
    Ok(ActorMixer { node: read_node(r)?, children: ids32(r)? })
}

fn read_blend(r: &mut Reader) -> Result<Blend> {
    let node = read_node(r)?;
    let children = ids32(r)?;
    let n = r.count(13)?;
    let layers = (0..n)
        .map(|_| {
            let id = r.u32()?;
            let count = r.u16()? as usize;
            let rtpcs = (0..count).map(|_| read_rtpc(r)).collect::<Result<_>>()?;
            let (crossfade_rtpc_id, crossfade_rtpc_type) = (r.u32()?, r.u8()?);
            let count = r.count(8)?;
            let associations = (0..count)
                .map(|_| {
                    let child = r.u32()?;
                    let points = r.count(12)?;
                    Ok((child, (0..points).map(|_| read_point(r)).collect::<Result<_>>()?))
                })
                .collect::<Result<_>>()?;
            Ok(Layer { id, rtpcs, crossfade_rtpc_id, crossfade_rtpc_type, associations })
        })
        .collect::<Result<_>>()?;
    Ok(Blend { node, children, layers })
}

fn read_rtpcs(r: &mut Reader) -> Result<Vec<Rtpc>> {
    let n = r.u16()? as usize;
    (0..n).map(|_| read_rtpc(r)).collect()
}

fn put_rtpcs(w: &mut Vec<u8>, rtpcs: &[Rtpc]) {
    w.put_u16(rtpcs.len() as u16);
    rtpcs.iter().for_each(|rtpc| write_rtpc(w, rtpc));
}

fn read_state_groups(r: &mut Reader) -> Result<Vec<StateGroupRef>> {
    let n = r.count(7)?;
    (0..n)
        .map(|_| {
            let (group_id, sync_type) = (r.u32()?, r.u8()?);
            let count = r.u16()? as usize;
            let states = (0..count).map(|_| Ok((r.u32()?, r.u32()?))).collect::<Result<_>>()?;
            Ok(StateGroupRef { group_id, sync_type, states })
        })
        .collect()
}

fn put_state_groups(w: &mut Vec<u8>, groups: &[StateGroupRef]) {
    w.put_u32(groups.len() as u32);
    for g in groups {
        w.put_u32(g.group_id);
        w.put_u8(g.sync_type);
        w.put_u16(g.states.len() as u16);
        for (state, props) in &g.states {
            w.put_u32(*state);
            w.put_u32(*props);
        }
    }
}

fn read_effect_slots(r: &mut Reader) -> Result<(Vec<EffectSlot>, u8)> {
    let n = r.u8()? as usize;
    if n == 0 {
        return Ok((vec![], 0));
    }

    let bypass = r.u8()?;
    let slots = (0..n)
        .map(|_| Ok(EffectSlot { index: r.u8()?, effect_id: r.u32()?, is_share_set: r.u8()?, rendered: r.u8()? }))
        .collect::<Result<_>>()?;
    Ok((slots, bypass))
}

fn put_effect_slots(w: &mut Vec<u8>, slots: &[EffectSlot], bypass: u8) {
    w.put_u8(slots.len() as u8);
    if !slots.is_empty() {
        w.put_u8(bypass);
        for e in slots {
            w.put_u8(e.index);
            w.put_u32(e.effect_id);
            w.put_u8(e.is_share_set);
            w.put_u8(e.rendered);
        }
    }
}

fn read_bus(r: &mut Reader, aux: bool) -> Result<Bus> {
    let parent_bus_id = r.u32()?;
    if r.left() == 0 {
        return Ok(Bus { aux, parent_bus_id, params: None });
    }

    let props = read_props(r)?;
    let positioning = read_positioning(r)?;
    let (advanced_bits, max_instances, channel_config, hdr_bits) = (r.u8()?, r.u16()?, r.u32()?, r.u8()?);
    let (recovery_time, max_duck_volume) = (r.u32()? as i32, r.f32()?);
    let n = r.count(18)?;
    let ducks = (0..n)
        .map(|_| {
            Ok(Duck {
                bus_id: r.u32()?,
                volume: r.f32()?,
                fade_out_time: r.u32()? as i32,
                fade_in_time: r.u32()? as i32,
                fade_curve: r.u8()?,
                target_prop: r.u8()?,
            })
        })
        .collect::<Result<_>>()?;
    let (effects, effects_bypass) = read_effect_slots(r)?;
    let (override_attachment, effect_0_id, effect_0_is_share_set) = (r.u8()?, r.u32()?, r.u8()?);
    let rtpcs = read_rtpcs(r)?;
    let state_groups = read_state_groups(r)?;
    let params = BusParams {
        props,
        positioning,
        advanced_bits,
        max_instances,
        channel_config,
        hdr_bits,
        recovery_time,
        max_duck_volume,
        ducks,
        effects,
        effects_bypass,
        override_attachment,
        effect_0_id,
        effect_0_is_share_set,
        rtpcs,
        state_groups,
    };
    Ok(Bus { aux, parent_bus_id, params: Some(params) })
}

fn write_bus(w: &mut Vec<u8>, b: &Bus) {
    w.put_u32(b.parent_bus_id);
    let Some(p) = &b.params else { return };
    put_props(w, &p.props);
    write_positioning(w, &p.positioning);
    w.put_u8(p.advanced_bits);
    w.put_u16(p.max_instances);
    w.put_u32(p.channel_config);
    w.put_u8(p.hdr_bits);
    w.put_u32(p.recovery_time as u32);
    w.put_f32(p.max_duck_volume);
    w.put_u32(p.ducks.len() as u32);
    for d in &p.ducks {
        w.put_u32(d.bus_id);
        w.put_f32(d.volume);
        w.put_u32(d.fade_out_time as u32);
        w.put_u32(d.fade_in_time as u32);
        w.put_u8(d.fade_curve);
        w.put_u8(d.target_prop);
    }
    put_effect_slots(w, &p.effects, p.effects_bypass);
    w.put_u8(p.override_attachment);
    w.put_u32(p.effect_0_id);
    w.put_u8(p.effect_0_is_share_set);
    put_rtpcs(w, &p.rtpcs);
    put_state_groups(w, &p.state_groups);
}

fn read_attenuation(r: &mut Reader) -> Result<Attenuation> {
    let cone_enabled = r.u8()?;
    let cone = if cone_enabled != 0 { Some([r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?]) } else { None };
    let curve_to_use = r.array::<7>()?;
    let n = r.u8()? as usize;
    let curves = (0..n)
        .map(|_| {
            let scaling = r.u8()?;
            let count = r.u16()? as usize;
            Ok((scaling, (0..count).map(|_| read_point(r)).collect::<Result<_>>()?))
        })
        .collect::<Result<_>>()?;
    let rtpcs = if r.left() > 0 { Some(read_rtpcs(r)?) } else { None };
    Ok(Attenuation { cone_enabled, cone, curve_to_use, curves, rtpcs })
}

/// A decision tree (dialogue events, music switches): (arguments, mode, nodes).
type Tree = (Vec<(u32, u8)>, u8, Vec<TreeNode>);

fn read_tree(r: &mut Reader) -> Result<Tree> {
    let depth = r.count(5)?;
    let groups = (0..depth).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
    let kinds = (0..depth).map(|_| r.u8()).collect::<Result<Vec<_>>>()?;
    let tree_size = r.u32()? as usize;
    if !tree_size.is_multiple_of(12) {
        return Err(FormatError::Invalid(format!("a decision tree of {tree_size} bytes is not whole 12-byte nodes")));
    }

    let mode = r.u8()?;
    let nodes = (0..tree_size / 12)
        .map(|_| Ok(TreeNode { key: r.u32()?, value: r.u32()?, weight: r.u16()?, probability: r.u16()? }))
        .collect::<Result<_>>()?;
    Ok((groups.into_iter().zip(kinds).collect(), mode, nodes))
}

fn write_tree(w: &mut Vec<u8>, arguments: &[(u32, u8)], mode: u8, nodes: &[TreeNode]) {
    w.put_u32(arguments.len() as u32);
    arguments.iter().for_each(|(group, _)| w.put_u32(*group));
    arguments.iter().for_each(|(_, kind)| w.put_u8(*kind));
    w.put_u32(nodes.len() as u32 * 12);
    w.put_u8(mode);
    for n in nodes {
        w.put_u32(n.key);
        w.put_u32(n.value);
        w.put_u16(n.weight);
        w.put_u16(n.probability);
    }
}

fn read_dialogue_event(r: &mut Reader) -> Result<DialogueEvent> {
    let probability = r.u8()?;
    let (arguments, mode, nodes) = read_tree(r)?;
    Ok(DialogueEvent { probability, arguments, mode, nodes })
}

fn read_effect(r: &mut Reader, share_set: bool) -> Result<Effect> {
    let plugin_id = r.u32()?;
    let size = r.u32()? as usize;
    let params = r.take(size)?.to_vec();
    let n = r.u8()? as usize;
    let media = (0..n).map(|_| Ok((r.u8()?, r.u32()?))).collect::<Result<_>>()?;
    let rtpcs = if r.left() > 0 { Some(read_rtpcs(r)?) } else { None };
    let rtpc_init_flags = if r.left() >= 2 { Some(r.u16()?) } else { None };
    Ok(Effect { share_set, plugin_id, params, media, rtpcs, rtpc_init_flags })
}
