//! The scene's sound while it plays in the editor (docs/plans/cinematic.md, step 4): the clip's sound and music tracks mixed as the
//! movie export mixes them (`movie_export::mixdown`, on a worker, again when the project or the clip changes), played from the
//! playhead through the Audio tool's player, following scrubs; while it plays, the sound drives the playhead. Scripted runs are
//! silent (they don't listen, and the clock is theirs).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_audio::decode::Pcm;
use wce_audio::library::SoundLibrary;
use wce_audio_tool::player::Player;
use wce_cinematic::movie_export::mixdown;
use wce_cinematic::scene::Project;
use wce_vfs::files::Files;

/// What a mix is of: the document's version and the clip.
type MixKey = (u64, u32);

#[derive(Default)]
pub struct SceneAudio {
    player: Player,
    key: Option<MixKey>,
    mixing: Option<(MixKey, Receiver<Pcm>)>,
    pcm: Option<Arc<Pcm>>,
    sounding: bool,
    last_time: f64,
}

impl SceneAudio {
    /// Every frame. `time` is the playhead; returns the time the sound has reached while it plays (the playhead follows it).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        version: u64,
        project: &Project,
        clip: u32,
        playing: bool,
        time: f64,
        library: Option<&Arc<SoundLibrary>>,
        files: Option<&Arc<dyn Files>>,
    ) -> Option<f64> {
        let key = (version, clip);
        if self.key != Some(key)
            && self.mixing.as_ref().is_none_or(|(k, _)| *k != key)
            && let (Some(lib), Some(files), Some(c)) = (library, files, project.clip(clip))
        {
            let (lib, files, project, c) = (lib.clone(), files.clone(), project.clone(), c.clone());
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let mut missing = vec![];
                let _ = tx.send(mixdown(&project, &c, &lib, files.as_ref(), &mut missing));
            });
            self.mixing = Some((key, rx));
        }
        if let Some((k, rx)) = &self.mixing
            && let Ok(pcm) = rx.try_recv()
        {
            self.key = Some(*k);
            self.pcm = Some(Arc::new(pcm));
            self.mixing = None;
            if self.sounding {
                // A new mix while playing (an edit): carry on from here with it.
                self.start(time);
            }
        }

        self.player.tick();
        if playing && !self.sounding {
            self.start(time);
        } else if !playing && self.sounding {
            self.player.stop();
            self.sounding = false;
        } else if playing && (time - self.last_time).abs() > 0.25 {
            // A scrub while playing.
            self.start(time);
        }
        let heard =
            (self.sounding && self.player.playing() && self.player.loaded()).then(|| self.player.position() as f64 * self.player.length());
        self.last_time = heard.unwrap_or(time);
        heard
    }

    fn start(&mut self, time: f64) {
        self.sounding = true;
        let Some(pcm) = self.pcm.clone() else { return };
        let length = pcm.seconds().max(0.001);
        if time < length && self.player.play(pcm, "scene", None).is_ok() {
            self.player.seek((time / length) as f32);
        }
        self.last_time = time;
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.sounding = false;
    }
}
