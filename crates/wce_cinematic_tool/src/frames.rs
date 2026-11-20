//! A movie's pictures by frame index: decodes forward from the last picture, or from the key frame before the one asked for when
//! that is behind or far ahead. Used directly (scripted runs: the same frame every run) or on a worker thread (live playback).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use wce_video::{Decoder, Movie, yuv};

/// Further ahead than this, a seek starts again from the key frame before the target.
const FORWARD_LIMIT: usize = 90;

/// A decoded picture, RGBA rows.
#[derive(Clone)]
pub struct Picture {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

pub struct FrameSource {
    movie: Arc<Movie>,
    decoder: Option<Decoder>,
    /// The last frame decoded, and the last picture shown (an alt-ref frame shows none).
    at: Option<usize>,
    last: Option<Picture>,
}

impl FrameSource {
    pub fn new(movie: Arc<Movie>) -> Self {
        Self { movie, decoder: None, at: None, last: None }
    }

    /// The picture of frame `index` (clamped to the movie).
    pub fn picture(&mut self, index: usize) -> Result<Picture, String> {
        let count = self.movie.frame_count();
        if count == 0 {
            return Err("the movie has no frames".into());
        }
        let index = index.min(count - 1);
        if let Some(p) = &self.last
            && self.at == Some(index)
        {
            return Ok(Picture { index, ..p.clone() });
        }
        let start = match self.at {
            Some(at) if at < index && index - at <= FORWARD_LIMIT && self.decoder.is_some() => at + 1,
            _ => {
                self.decoder = Some(Decoder::new(2).map_err(|e| e.to_string())?);
                self.last = None;
                self.movie.key_frame_before(index)
            }
        };
        let decoder = self.decoder.as_mut().expect("made above");
        let mut shown = None;
        for i in start..=index {
            let frame = self.movie.frame(i).map_err(|e| e.to_string())?;
            if let Some(p) = decoder.decode(frame).map_err(|e| e.to_string())? {
                shown = Some(p);
            }
            self.at = Some(i);
        }
        if let Some(p) = shown {
            self.last = Some(Picture { index, width: p.width, height: p.height, rgba: Arc::new(yuv::to_rgba(&p)) });
        }
        self.last.clone().map(|p| Picture { index, ..p }).ok_or_else(|| format!("frame {index} shows no picture"))
    }
}

/// A worker decoding on its own thread; only the newest request counts.
pub struct FrameWorker {
    requests: Sender<usize>,
    pictures: Receiver<Result<Picture, String>>,
}

impl FrameWorker {
    pub fn start(movie: Arc<Movie>) -> Self {
        let (req_tx, req_rx) = channel::<usize>();
        let (pic_tx, pic_rx) = channel();
        std::thread::spawn(move || {
            let mut source = FrameSource::new(movie);
            while let Ok(mut index) = req_rx.recv() {
                while let Ok(newer) = req_rx.try_recv() {
                    index = newer;
                }
                if pic_tx.send(source.picture(index)).is_err() {
                    break;
                }
            }
        });
        Self { requests: req_tx, pictures: pic_rx }
    }

    pub fn request(&self, index: usize) {
        let _ = self.requests.send(index);
    }

    /// The newest picture ready, if any.
    pub fn poll(&self) -> Option<Result<Picture, String>> {
        let mut out = None;
        while let Ok(p) = self.pictures.try_recv() {
            out = Some(p);
        }
        out
    }
}
