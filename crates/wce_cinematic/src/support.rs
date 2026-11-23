//! What a historical battle can reproduce of each track (docs/plans/cinematic.md, "Battle gap"): the movie reproduces everything;
//! the battle's cutscene Lua plays the camera and the timed events exactly, the units as orders (the engine simulates them), and some
//! things not at all. The editor flags each track with this; the battle exporter drops what is movie only.

use crate::scene::TrackContent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Support {
    /// The battle plays it as keyed.
    Exact,
    /// The battle plays something close: orders, the game's own pathing, on/off instead of a ramp.
    Approximate,
    /// Movies only: the battle export leaves it out.
    MovieOnly,
}

impl Support {
    pub fn label(self) -> &'static str {
        match self {
            Support::Exact => "battle: exact",
            Support::Approximate => "battle: approximate",
            Support::MovieOnly => "movie only",
        }
    }
}

/// A track's support in a battle, and why.
pub fn track_support(content: &TrackContent) -> (Support, &'static str) {
    match content {
        TrackContent::Camera { .. } => (Support::Exact, "baked into cam:move_to segments on the battle's 100 ms ticks"),
        TrackContent::Cuts { .. } => (Support::Exact, "each cut an instant cam:move_to to the camera cut to"),
        TrackContent::Unit { actions, .. } => {
            if actions.iter().any(|a| !matches!(a.slot.as_str(), "AUTO" | "STAND") && crate::scene::slot_family(&a.slot) != "TAUNT") {
                (Support::Approximate, "moves become orders (the game paths them); other animation slots than taunt are movie only")
            } else {
                (Support::Approximate, "moves become orders and teleports: the game paths and animates the men")
            }
        }
        TrackContent::Sound { .. } => (Support::Exact, "new_sfx / play_sound at the key's time"),
        TrackContent::Music { .. } => (Support::Exact, "play_music / stop_music with the dynamic music paused"),
        TrackContent::Subtitles { .. } => (Support::Exact, "scripted_subtitles rows and subtitles:set_text"),
        TrackContent::Vfx { .. } => (Support::MovieOnly, "no battle Lua call places VFX"),
        TrackContent::Fade { .. } => (Support::Approximate, "cam:fade: to and from black over a duration"),
        TrackContent::Letterbox { .. } => (Support::Approximate, "the cinematic bars, on or off"),
        TrackContent::DepthOfField { .. } => (Support::Exact, "cam:change_depth_of_field"),
        TrackContent::Weather { .. } => (Support::MovieOnly, "a battle has one weather, the setup's"),
    }
}
