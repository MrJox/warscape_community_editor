//! Gate of the recipes (docs/plans/audio.md, step 8) on a game's data:
//! 1. the music playlists, each named by the events that play it;
//! 2. an animation frame firing an existing event reuses its trigger row;
//! 3. an animation frame with a new sound makes the whole chain: frame -> trigger row -> new event -> new sound (under an actor-mixer, or alone on the chain's bus) -> loose media;
//! 4. a dialogue event's lines for one voice-over culture (`Roman` of `vo_battle_subculture`, whose values are the DB's
//!    `voiceover_culture`).
//!
//!   audio_recipes_check <game folder> [mod pack...]

use std::path::PathBuf;

use rpfm_lib::schema::Schema;
use wce_audio::anim_sounds;
use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::edit::EditSet;
use wce_audio::encode::{DEFAULT_QUALITY, encode_wem};
use wce_audio::library::SoundLibrary;
use wce_audio::recipes::{self, AnimationSound};
use wce_audio::{sound_db, unit_voices};
use wce_audio_format::fnv;
use wce_audio_format::hirc::Object;
use wce_audio_format::triggers::Triggers;
use wce_formats::anim_meta::Meta;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("audio_recipes_check <game folder> [mod pack...]");
        std::process::exit(2);
    };
    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let lib = SoundLibrary::load(&vfs);
    let mut failures = vec![];

    // 1. Music.
    let playlists = recipes::music_playlists(&lib, &vfs);
    let named = playlists.iter().filter(|(_, events)| !events.is_empty()).count();
    println!("1. {} music playlists, {named} named by events; e.g.:", playlists.len());
    for (id, events) in playlists.iter().take(4) {
        println!("   #{id}: {}", events.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
    }
    if playlists.is_empty() || named != playlists.len() {
        failures.push("music playlists");
    }

    // 2. A frame firing an existing event.
    let anim = vfs.list("animations/", ".snd.meta").into_iter().find(|p| p.contains("boar_charge.snd")).expect("boar_charge").to_owned();
    let frames_before = anim_sounds::frames(&Meta::decode(&vfs.read(&anim).unwrap()).unwrap()).len();
    let mut edits = EditSet::default();
    let row = recipes::animation_sound(&mut edits, &lib, &vfs, &anim, 0.25, AnimationSound::Event("battle_footstep_walk_01".into()))
        .expect("existing");
    let frames = anim_sounds::frames(&Meta::decode(&edits.loose[&anim]).unwrap());
    let ok = frames.len() == frames_before + 1
        && frames.iter().any(|f| f.trigger == row && f.time == 0.25)
        && !edits.loose.contains_key("audio/sound_animation_triggers.csv");
    println!("2. {anim}: a frame at 0.25 s on existing row {row}: {} frames (was {frames_before}), no new row: {ok}", frames.len());
    if !ok {
        failures.push("animation frame, existing event");
    }

    // 3. A frame with a new sound.
    let rate = 44_100u32;
    let samples = (0..rate / 3).map(|n| 0.3 * (std::f32::consts::TAU * 200.0 * n as f32 / rate as f32).sin()).collect();
    let wem = encode_wem(&Pcm { channels: 1, sample_rate: rate, samples }, DEFAULT_QUALITY).expect("encode");
    let mut edits = EditSet::default();
    let sound = AnimationSound::NewSound { wem, like_event: "battle_footstep_walk_01".into(), name: "wce_test_footstep".into() };
    let row = recipes::animation_sound(&mut edits, &lib, &vfs, &anim, 0.3, sound).expect("new sound");
    let csv = Triggers::read(&edits.loose["audio/sound_animation_triggers.csv"]).expect("csv");
    let event = fnv::id("wce_test_footstep");
    let chain = (|| {
        let meta = Meta::decode(&edits.loose[&anim])?;
        anim_sounds::frames(&meta).iter().find(|f| f.trigger == row)?;
        (csv.get(row, "sound_event_name_battle") == Some("wce_test_footstep")).then_some(())?;
        let Some(Object::Event(e)) = edits.object(&lib, event) else { return None };
        let Some(Object::Action(a)) = edits.object(&lib, e.actions[0]) else { return None };
        let Some(Object::Sound(s)) = edits.object(&lib, a.target_id) else { return None };
        let placed = match edits.object(&lib, s.node.parent_id) {
            Some(Object::ActorMixer(m)) => m.children.contains(&a.target_id),
            None => s.node.parent_id == 0 && s.node.override_bus_id != 0,
            _ => false,
        };
        placed.then_some(())?;
        decode_wem(edits.loose.get(&format!("audio/{}.wem", s.source.source_id))?).ok()
    })();
    println!("3. a new sound on row {row}: frame -> row -> wce_test_footstep -> sound placed -> media decodes: {}", chain.is_some());
    if chain.is_none() {
        failures.push("animation frame, new sound");
    }

    // 4. A culture's lines.
    let culture = fnv::id("vo_battle_subculture");
    let roman = fnv::id("Roman");
    let mut found = None;
    'search: for b in &lib.banks {
        for (o, t) in b.bank.objects().iter().zip(&b.typed) {
            if let Some(Object::DialogueEvent(d)) = t
                && d.arguments.iter().any(|(g, _)| *g == culture)
            {
                let lines = recipes::culture_lines(&EditSet::default(), &lib, o.id, culture, roman);
                let values = recipes::group_values(&EditSet::default(), &lib, o.id, culture);
                if !lines.is_empty() {
                    found = Some((lib.label(o.id, 15), lines.len(), values.iter().map(|v| lib.label(*v, 0)).collect::<Vec<_>>()));
                    break 'search;
                }
            }
        }
    }
    println!("4. Roman lines: {found:?}");
    if found.is_none() {
        failures.push("culture lines");
    }

    // 5. Unit voices: the orders tree, a unit's culture and actors, a new selection line under each of them, the unit's row.
    let cultures = unit_voices::cultures(&EditSet::default(), &lib);
    let barbarian = unit_voices::culture_of(&lib, &cultures, "barbarian_christian");
    let actors = barbarian.and_then(|b| cultures.iter().find(|c| c.culture == b)).map(|c| unit_voices::family_actors(&lib, c, false));
    let actors = actors.unwrap_or_default();
    let mut edits = EditSet::default();
    let wem = encode_wem(&Pcm { channels: 1, sample_rate: 44_100, samples: vec![0.1; 22_050] }, DEFAULT_QUALITY).expect("encode");
    let switches: Vec<u32> = actors.iter().map(|(_, s)| *s).collect();
    let added = edits.add_order_line(&lib, &switches, "selected_wce_test", fnv::id("selected_archers"), &wem);
    let lines_ok = added.as_ref().is_ok_and(|value| {
        switches.iter().all(|s| {
            let lines = unit_voices::lines(&edits, &lib, *s);
            lines.iter().any(|(v, line)| {
                *v == *value
                    && matches!(edits.object(&lib, *line), Some(Object::RandomSequence(r)) if r.children.len() == 1
                        && matches!(edits.object(&lib, r.children[0]), Some(Object::Sound(s)) if edits.loose.contains_key(&format!("audio/{}.wem", s.source.source_id))))
            })
        })
    });
    let schema = wce_foundation::settings::rpfm_schema_path(vfs.profile()).and_then(|p| Schema::load(&p, None).ok());
    let unit = schema.as_ref().and_then(|schema| {
        let units = sound_db::table_rows(schema, &vfs, "land_units_tables");
        let key = units.first()?.get("key")?.clone();
        let bytes = sound_db::set_cell(schema, &vfs, None, "land_units_tables", "key", &key, "selection_vo", "selected_wce_test").ok()?;
        let rows = sound_db::rows_of(schema, "land_units_tables", &bytes);
        (rows.len() == 1 && rows[0].get("selection_vo").map(String::as_str) == Some("selected_wce_test")).then_some(key)
    });
    println!(
        "5. unit voices: {} cultures; barbarian_christian -> {:?}, {} foot actors; a new line under each: {lines_ok}; a unit's row \
         rewritten: {unit:?}",
        cultures.len(),
        barbarian.map(|b| lib.label(b, 0)),
        actors.len()
    );
    if cultures.len() < 4 || actors.len() != 12 || !lines_ok || unit.is_none() {
        failures.push("unit voices");
    }

    for f in &failures {
        println!("FAILED: {f}");
    }
    std::process::exit(if failures.is_empty() { 0 } else { 1 });
}
