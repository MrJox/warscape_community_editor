//! What the Inspector shows for a property: its group, a readable label and what it does. The XML name is in every tooltip.

/// The groups of an emitter's properties, in the Inspector's order.
pub const GROUPS: [&str; 7] = ["Emission", "Particles", "Spawn shape", "Motion", "Culling", "Rigid models", "Other"];

/// The group an emitter property is shown in.
pub fn group(name: &str) -> &'static str {
    let starts = |p: &[&str]| p.iter().any(|p| name.starts_with(p));
    if matches!(name, "ParticlesPerSecond" | "ParticlesMax") {
        "Emission"
    } else if starts(&["Rigid"]) {
        "Rigid models"
    } else if starts(&["Cull", "ParticleCull"]) {
        "Culling"
    } else if starts(&["Spawn", "Emitter", "EmissionPoint", "AllowEmitFromMesh", "UseSurfaceNormal", "Face", "Mode"])
        && !starts(&["SpawnSpeed", "SpawnSpread", "SpawnSpacing"])
    {
        "Spawn shape"
    } else if starts(&["Particle", "RandomOrientation", "Spin", "ActAsRibbon"]) {
        "Particles"
    } else if starts(&[
        "SpawnSpeed",
        "SpawnSpread",
        "SpreadMethod",
        "Gravity",
        "Deceleration",
        "Rotational",
        "Central",
        "Turbulence",
        "Effected",
        "Wind",
        "VectorField",
        "Mass",
    ]) {
        "Motion"
    } else if starts(&[
        "Enable",
        "Visible",
        "Emission",
        "Density",
        "Burst",
        "InfiniteBursts",
        "StartTime",
        "LifeTime",
        "InfiniteLife",
        "EventEnable",
        "Recycle",
        "SpawnSpacing",
        "RandomSeed",
    ]) {
        "Emission"
    } else {
        "Other"
    }
}

/// `ParticleScaleStart` -> `Particle scale start`, `pivot_x` -> `Pivot x`.
pub fn label(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let chars = name.chars().collect::<Vec<_>>();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            out.push(' ');
            continue;
        }

        let word_start = i > 0 && c.is_uppercase() && (chars[i - 1].is_lowercase() || chars.get(i + 1).is_some_and(|n| n.is_lowercase()));
        if word_start && !out.ends_with(' ') {
            out.push(' ');
        }

        if out.is_empty() {
            out.extend(c.to_uppercase());
        } else if word_start {
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }

    out
}

/// What a property does, as the editor's particle runtime reads it (docs/re/particles.md); empty when it is not described.
pub fn meaning(name: &str) -> &'static str {
    match name {
        "Enable" => "The emitter plays at all (false: skipped when the effect loads).",
        "Visible" => "The emitter's particles are drawn.",
        "ParticlesPerSecond" => "The emission rate (min, max): particles a second, lerped per emitter instance.",
        "EmissionRateCurve" => "The emission rate over the emitter's life (curves of the rate's min and max).",
        "EmissionLoopAnim" => "The emission rate curve loops over the emitter's life instead of playing once.",
        "LifeTime" => "How long the emitter emits (seconds, min to max), unless Infinite life.",
        "InfiniteLife" => "The emitter emits forever.",
        "StartTime" => "The delay before the emitter starts (seconds, min to max).",
        "Bursts" | "InfiniteBursts" | "BurstInterval" | "BurstIntervalLength" => {
            "Burst emission: how many bursts, how far apart, how long."
        }
        "ParticleLifeTime" => "How long each particle lives (seconds, min to max), plus the reference's lifetime bias.",
        "ParticleScaleStart" | "ParticleScaleMid" | "ParticleScaleEnd" => {
            "A particle's size (width, height in metres) at its birth, at the mid point and at its death."
        }
        "ParticleScaleMidPointTime" => "When the mid size applies, 0 (birth) to 1 (death).",
        "ParticleScaleUseSingleScale" => "Width and height are one value (x).",
        "ParticleScaleVariation" => "Random variation of the size per particle.",
        "SpawnPosition" => "Where particles are born, in metres from the effect's origin (turned with it).",
        "SpawnAreaBox" => "A box particles are born in: its size per axis (x, y, z min and max curves over the emitter's life), centred.",
        "SpawnAreaCurve" => {
            "A ring particles are born on: its inner and outer radius (metres, min and max curves over the emitter's life)."
        }
        "SpawnArc" => "The ring's arc (degrees, from x to y): 0 to 360 is the whole ring.",
        "SpawnAreaLoopAnim" => "The spawn area's curves loop over the emitter's life.",
        "EmitterRotation" => "Turns the spawn area and directions (degrees about x, then y, then z).",
        "SpawnSpeed" => "Speed at birth (metres a second, min to max).",
        "SpawnSpread" => "With Spread method: the direction's elevation above the horizon (degrees, min to max); 90 is straight up.",
        "SpreadMethod" => "True: particles leave in the Spawn spread cone. False: straight away from the spawn position.",
        "GravityAmount" => "Gravity on the particles (min to max).",
        "Deceleration" | "DecelerationAmount" | "DecelerationStartTime" => "The particles slow down from a time on, by an amount.",
        "EffectedByWind" | "EffectedByWindFactor" => "The battle's wind pushes the particles (times the factor).",
        "WindVarianceFactor" => "How much the wind's gusts vary per particle.",
        "TurbulenceStart" | "TurbulenceEnd" => "Random motion at the particle's birth and death.",
        "CullDistances" => "Distances from the camera (metres): w is where the emitter stops spawning.",
        "ParticleCullDistances" => "Distances from the camera (metres) over which particles fade out.",
        "CullCompletely" => "Distance beyond which the effect is not drawn at all.",
        "ParticlesFollowEmitter" => "Particles move with the emitter after their birth.",
        "AllowEmitFromMesh" => "Placed on a building with a mesh (a VOLUMETRIC_EFFECT), particles are born on its triangles.",
        "UseSurfaceNormal" => "Particles born on a mesh leave along the triangle's normal.",
        "RandomOrientation" | "ParticleOrientation" => "The particle's starting orientation: random, or as given.",
        "Spin" | "ParticleSpinAxis" => "How fast particles spin (radians a second) and about which axis.",
        "RigidModelFile" => "The model each rigid particle draws (rigidmodels/; point_lights/ models are lights).",
        "RigidsPerBurst" | "RigidLifeTime" => "How many rigid models a burst throws, and how long each lives.",
        "start_colour" | "mid_colour" | "end_colour" => {
            "The particle's colour at its birth, mid point and death (min to max: a random blend)."
        }
        "lerp_time" => "When the mid value applies over the particle's life (min, max).",
        "initial_alpha" | "mid_alpha_a" | "mid_alpha_b" | "end_alpha" => "The particle's opacity at birth, the two mid points, death.",
        "number_of_cells_x" | "number_of_cells_y" | "cell" | "randomize_cell" => {
            "A texture of cells (a flipbook): its grid and the cell drawn."
        }
        "diffuse" | "normal" => "A texture of the particles (a file under the effect's texture folder).",
        "additive_blend" | "additive_blend_mul" => "The particles add light (fire, glows) instead of covering what is behind them.",
        "ambient_lighting_mul" | "reverse_lighting" => "How the scene's light falls on the particles.",
        "soft_factor" => "How softly particles fade where they meet the ground and models.",
        "pivot_x" | "pivot_y" => "The point of the particle's quad that sits on its position (0.5, 0.5 the centre).",
        "restriction" => "How the quad faces the camera (0 free, 1 upright, ...).",
        _ => "",
    }
}

/// The tooltip of a property: what it does and its XML name.
pub fn hint(name: &str, extra: &str) -> String {
    let m = meaning(name);
    let mut out = String::new();
    if !m.is_empty() {
        out += m;
        out += "\n\n";
    }

    if !extra.is_empty() {
        out += extra;
        out += "\n";
    }

    out + &format!("XML: {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_groups() {
        assert_eq!(label("ParticleScaleStart"), "Particle scale start");
        assert_eq!(label("pivot_x"), "Pivot x");
        assert_eq!(label("UseSurfaceNormal"), "Use surface normal");
        assert_eq!(label("ParticlesPerSecond"), "Particles per second");
        assert_eq!(group("SpawnSpeed"), "Motion");
        assert_eq!(group("SpawnAreaCurve"), "Spawn shape");
        assert_eq!(group("ParticlesPerSecond"), "Emission");
        assert_eq!(group("ParticleLifeTime"), "Particles");
        assert_eq!(group("LifeTime"), "Emission");
        assert_eq!(group("ParticleCullDistances"), "Culling");
        assert!(GROUPS.contains(&group("Whatever")));
    }
}
