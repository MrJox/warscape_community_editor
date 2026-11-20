//! VP8 through libvpx (third_party/libvpx, built by build.rs) and `shim.c`, which hides libvpx's structs behind plain arguments.

use std::ffi::{CStr, c_char, c_int};

#[repr(C)]
struct Planes {
    y: *const u8,
    u: *const u8,
    v: *const u8,
    y_stride: c_int,
    u_stride: c_int,
    v_stride: c_int,
    width: c_int,
    height: c_int,
}

#[repr(C)]
struct RawSettings {
    width: c_int,
    height: c_int,
    fps_num: c_int,
    fps_den: c_int,
    target_kbps: c_int,
    cq_level: c_int,
    min_q: c_int,
    max_q: c_int,
    cpu_used: c_int,
    threads: c_int,
    keyframe_max: c_int,
    lag_in_frames: c_int,
    deadline_best: c_int,
}

#[repr(C)]
struct RawDecoder {
    _private: [u8; 0],
}

#[repr(C)]
struct RawEncoder {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn wce_vp8_dec_new(threads: c_int) -> *mut RawDecoder;
    fn wce_vp8_dec_decode(d: *mut RawDecoder, data: *const u8, len: usize) -> c_int;
    fn wce_vp8_dec_frame(d: *mut RawDecoder, out: *mut Planes) -> c_int;
    fn wce_vp8_dec_error(d: *mut RawDecoder) -> *const c_char;
    fn wce_vp8_dec_free(d: *mut RawDecoder);

    fn wce_vp8_enc_new(s: *const RawSettings) -> *mut RawEncoder;
    fn wce_vp8_enc_encode(
        e: *mut RawEncoder,
        y: *const u8,
        u: *const u8,
        v: *const u8,
        y_stride: c_int,
        uv_stride: c_int,
        pts: i64,
        force_key: c_int,
    ) -> c_int;
    fn wce_vp8_enc_packet(e: *mut RawEncoder, data: *mut *const u8, len: *mut usize, pts: *mut i64, is_key: *mut c_int) -> c_int;
    fn wce_vp8_enc_error(e: *mut RawEncoder) -> *const c_char;
    fn wce_vp8_enc_free(e: *mut RawEncoder);
}

#[derive(Debug, thiserror::Error)]
pub enum Vp8Error {
    #[error("libvpx could not start the VP8 {0}")]
    Init(&'static str),
    #[error("VP8 decoding failed: {0}")]
    Decode(String),
    #[error("VP8 encoding failed: {0}")]
    Encode(String),
    #[error("the picture is {got_w} x {got_h}, the encoder takes {want_w} x {want_h}")]
    Size { got_w: u32, got_h: u32, want_w: u32, want_h: u32 },
}

fn message(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: libvpx returns a NUL-terminated static or context-owned string.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// A picture in I420: full-size Y, U and V at half width and height (rounded up), tightly packed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct I420 {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl I420 {
    pub fn chroma_size(width: u32, height: u32) -> (u32, u32) {
        (width.div_ceil(2), height.div_ceil(2))
    }

    /// A black picture (video range: Y 16, chroma 128).
    pub fn black(width: u32, height: u32) -> Self {
        let (cw, ch) = Self::chroma_size(width, height);
        Self { width, height, y: vec![16; (width * height) as usize], u: vec![128; (cw * ch) as usize], v: vec![128; (cw * ch) as usize] }
    }
}

fn copy_plane(src: *const u8, stride: c_int, width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height) as usize);
    for row in 0..height as usize {
        // SAFETY: libvpx's plane holds `height` rows of `stride` bytes, each with at least `width` pixels.
        let line = unsafe { std::slice::from_raw_parts(src.add(row * stride as usize), width as usize) };
        out.extend_from_slice(line);
    }
    out
}

/// A VP8 decoder: frames in, pictures out, in order.
pub struct Decoder {
    raw: *mut RawDecoder,
}

// SAFETY: the decoder context is used from one thread at a time (&mut self).
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(threads: u32) -> Result<Self, Vp8Error> {
        // SAFETY: plain constructor; null on failure.
        let raw = unsafe { wce_vp8_dec_new(threads.max(1) as c_int) };
        if raw.is_null() {
            return Err(Vp8Error::Init("decoder"));
        }
        Ok(Self { raw })
    }

    /// Decodes one compressed frame; the picture it shows, if any (an alt-ref frame shows none).
    pub fn decode(&mut self, frame: &[u8]) -> Result<Option<I420>, Vp8Error> {
        // SAFETY: `frame` outlives the call; the decoder copies what it keeps.
        let r = unsafe { wce_vp8_dec_decode(self.raw, frame.as_ptr(), frame.len()) };
        if r != 0 {
            // SAFETY: the context is alive.
            return Err(Vp8Error::Decode(message(unsafe { wce_vp8_dec_error(self.raw) })));
        }
        let mut p = Planes {
            y: std::ptr::null(),
            u: std::ptr::null(),
            v: std::ptr::null(),
            y_stride: 0,
            u_stride: 0,
            v_stride: 0,
            width: 0,
            height: 0,
        };
        // SAFETY: fills `p` with planes valid until the next decode; copied right away.
        if unsafe { wce_vp8_dec_frame(self.raw, &mut p) } == 0 {
            return Ok(None);
        }
        let (w, h) = (p.width as u32, p.height as u32);
        let (cw, ch) = I420::chroma_size(w, h);
        Ok(Some(I420 {
            width: w,
            height: h,
            y: copy_plane(p.y, p.y_stride, w, h),
            u: copy_plane(p.u, p.u_stride, cw, ch),
            v: copy_plane(p.v, p.v_stride, cw, ch),
        }))
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: owned, freed once.
        unsafe { wce_vp8_dec_free(self.raw) }
    }
}

/// How a movie is encoded. The defaults are for the game's 1080p movies: vanilla's run at about 5.5 Mbit/s.
#[derive(Clone, Debug, PartialEq)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The rate's cap in kbit/s (constrained quality), or the target with `cq_level` None.
    pub target_kbps: u32,
    /// Constrained quality level, 0 to 63 (lower is better); None for plain VBR.
    pub cq_level: Option<u32>,
    pub min_q: u32,
    pub max_q: u32,
    /// libvpx's speed for the good-quality deadline, 0 (slowest, best) to 16.
    pub cpu_used: i32,
    pub threads: u32,
    pub keyframe_max: u32,
    pub lag_in_frames: u32,
    pub best_quality: bool,
}

impl EncoderSettings {
    pub fn movie(width: u32, height: u32, fps: u32) -> Self {
        Self {
            width,
            height,
            fps,
            target_kbps: 8000,
            cq_level: Some(10),
            min_q: 4,
            max_q: 48,
            cpu_used: 1,
            threads: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4).min(16),
            keyframe_max: fps * 10,
            lag_in_frames: 25,
            best_quality: false,
        }
    }
}

/// One compressed frame from the encoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub pts: i64,
    pub is_key: bool,
}

/// A VP8 encoder: pictures in, compressed frames out (with look-ahead, they come later than their pictures: `finish` flushes).
pub struct Encoder {
    raw: *mut RawEncoder,
    width: u32,
    height: u32,
    next_pts: i64,
}

// SAFETY: the encoder context is used from one thread at a time (&mut self).
unsafe impl Send for Encoder {}

impl Encoder {
    pub fn new(s: &EncoderSettings) -> Result<Self, Vp8Error> {
        let raw_settings = RawSettings {
            width: s.width as c_int,
            height: s.height as c_int,
            fps_num: s.fps as c_int,
            fps_den: 1,
            target_kbps: s.target_kbps as c_int,
            cq_level: s.cq_level.map(|q| q.min(63) as c_int).unwrap_or(-1),
            min_q: s.min_q.min(63) as c_int,
            max_q: s.max_q.min(63) as c_int,
            cpu_used: s.cpu_used,
            threads: s.threads.max(1) as c_int,
            keyframe_max: s.keyframe_max.max(1) as c_int,
            lag_in_frames: s.lag_in_frames.min(25) as c_int,
            deadline_best: s.best_quality as c_int,
        };
        // SAFETY: the settings live through the call.
        let raw = unsafe { wce_vp8_enc_new(&raw_settings) };
        if raw.is_null() {
            return Err(Vp8Error::Init("encoder"));
        }
        Ok(Self { raw, width: s.width, height: s.height, next_pts: 0 })
    }

    fn drain(&mut self, out: &mut Vec<EncodedFrame>) {
        loop {
            let (mut data, mut len, mut pts, mut key) = (std::ptr::null(), 0usize, 0i64, 0);
            // SAFETY: the packet stays valid until the next encode; copied right away.
            if unsafe { wce_vp8_enc_packet(self.raw, &mut data, &mut len, &mut pts, &mut key) } == 0 {
                break;
            }
            // SAFETY: libvpx's buffer of `len` bytes.
            let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
            out.push(EncodedFrame { data: bytes, pts, is_key: key != 0 });
        }
    }

    /// Encodes the next picture (one frame later than the last); the compressed frames ready so far.
    pub fn encode(&mut self, picture: &I420, force_key: bool) -> Result<Vec<EncodedFrame>, Vp8Error> {
        if picture.width != self.width || picture.height != self.height {
            return Err(Vp8Error::Size { got_w: picture.width, got_h: picture.height, want_w: self.width, want_h: self.height });
        }
        let (cw, _) = I420::chroma_size(self.width, self.height);
        // SAFETY: the planes are tightly packed at the encoder's size and live through the call (libvpx copies into its look-ahead).
        let r = unsafe {
            wce_vp8_enc_encode(
                self.raw,
                picture.y.as_ptr(),
                picture.u.as_ptr(),
                picture.v.as_ptr(),
                self.width as c_int,
                cw as c_int,
                self.next_pts,
                force_key as c_int,
            )
        };
        if r != 0 {
            // SAFETY: the context is alive.
            return Err(Vp8Error::Encode(message(unsafe { wce_vp8_enc_error(self.raw) })));
        }
        self.next_pts += 1;
        let mut out = vec![];
        self.drain(&mut out);
        Ok(out)
    }

    /// Flushes the look-ahead: the remaining compressed frames.
    pub fn finish(mut self) -> Result<Vec<EncodedFrame>, Vp8Error> {
        let mut out = vec![];
        loop {
            // SAFETY: a null picture flushes.
            let r = unsafe { wce_vp8_enc_encode(self.raw, std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, 0, self.next_pts, 0) };
            if r != 0 {
                // SAFETY: the context is alive.
                return Err(Vp8Error::Encode(message(unsafe { wce_vp8_enc_error(self.raw) })));
            }
            let before = out.len();
            self.drain(&mut out);
            if out.len() == before {
                break;
            }
        }
        Ok(out)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: owned, freed once.
        unsafe { wce_vp8_enc_free(self.raw) }
    }
}
