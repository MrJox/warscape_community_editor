//! Malformed input for the binary readers: truncated, corrupted and hostile bytes give an error (or the reader's documented empty result),
//! never a panic, an endless loop or an allocation the data does not back. Every input is built here: a minimal valid file first (with the
//! crate's writer where it has one), checked to read, then every truncation, hostile values written over every field, and a seeded random
//! mutation loop. No game file is read.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use wce_formats::anim::Animation;
use wce_formats::cas2::Cas2File;
use wce_formats::chmf::{ChmfFile, ChmfRegion, decode_block, encode_region};
use wce_formats::dds_decode::{dds_decode_mip, dds_info};
use wce_formats::image_containers::{DdsFile, TgaFile, TiffFile};
use wce_formats::markers::{KeyPart, MarkerGroup, MarkersFile, record_from_fields};
use wce_formats::models_naval::{
    Bobby, Camera, CollisionFace, CustomPosition, DamagePart, Deck, DeckCorner, DeckPiece, EfLine, ModelsNaval, Pipe, ShipLogic, decode,
    encode,
};
use wce_formats::png_file::PngFile;

/// A xorshift64 generator: the same mutations on every run, without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// What a count, size or offset field is set to.
fn hostile_u32s(len: usize) -> [u32; 9] {
    [0, 1, 0x7F, 0xFFFF, i32::MAX as u32, 0x8000_0000, u32::MAX, len as u32, len as u32 + 1]
}

/// Runs `check` on `input`; a panic names the input, so a failure can be replayed.
fn run(check: &impl Fn(&[u8]), input: &[u8]) {
    if catch_unwind(AssertUnwindSafe(|| check(input))).is_err() {
        panic!("the reader panicked on {} bytes: {input:02x?}", input.len());
    }
}

/// Every truncation of `valid`, then hostile u32 and u16 values written over every byte offset, then `rounds` random mutations.
fn torture(valid: &[u8], seed: u64, rounds: usize, check: impl Fn(&[u8])) {
    for n in 0..valid.len() {
        run(&check, &valid[..n]);
    }

    let mut m = valid.to_vec();
    for at in 0..valid.len() {
        for v in hostile_u32s(valid.len()) {
            m.copy_from_slice(valid);
            for (i, b) in v.to_le_bytes().into_iter().enumerate() {
                if let Some(slot) = m.get_mut(at + i) {
                    *slot = b;
                }
            }

            run(&check, &m);
        }

        for v in [0x7FFFu16, 0x8000, 0xFFFF] {
            m.copy_from_slice(valid);
            for (i, b) in v.to_le_bytes().into_iter().enumerate() {
                if let Some(slot) = m.get_mut(at + i) {
                    *slot = b;
                }
            }

            run(&check, &m);
        }
    }

    let mut rng = Rng(seed);
    for _ in 0..rounds {
        let mut m = valid.to_vec();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(m.len());
            match rng.below(6) {
                0 if !m.is_empty() => m[at] ^= 1 << rng.below(8),
                1 if !m.is_empty() => m[at] = rng.next() as u8,
                2 => {
                    let v = hostile_u32s(m.len())[rng.below(9)].to_le_bytes();
                    for (i, b) in v.into_iter().enumerate() {
                        if let Some(slot) = m.get_mut(at + i) {
                            *slot = b;
                        }
                    }
                }
                3 => m.truncate(at),
                4 => m.insert(at, rng.next() as u8),
                _ if !m.is_empty() => {
                    m.remove(at);
                }
                _ => {}
            }
        }

        run(&check, &m);
    }
}

/// `bytes` with a little-endian u32 written at `at`.
fn with_u32(bytes: &[u8], at: usize, v: u32) -> Vec<u8> {
    let mut b = bytes.to_vec();
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    b
}

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(b: &mut Vec<u8>, v: f32) {
    b.extend_from_slice(&v.to_le_bytes());
}

// CHMF.

/// Two regions: 32 x 16 samples (a palette block and a raw one), and 16 x 16 of one value.
fn chmf(version: u32) -> (ChmfFile, Vec<u16>) {
    let samples = (0..32 * 16u32)
        .map(|i| {
            let (x, y) = (i % 32, i / 32);
            if x < 16 { ((x + y) % 5 * 1000) as u16 } else { (i.wrapping_mul(7919) % 65536) as u16 }
        })
        .collect::<Vec<_>>();
    let (tag, offsets, data) = encode_region(32, 16, &samples);
    let (flat_tag, flat_offsets, flat_data) = encode_region(16, 16, &[777; 256]);
    let file = ChmfFile {
        version,
        width: 48,
        height: 16,
        cells_x: 16,
        cells_y: 16,
        min: -5.0,
        max: 120.0,
        spacing: 1,
        regions: vec![
            ChmfRegion { rect: [0, 0, 2, 1], tag, offsets, data },
            ChmfRegion { rect: [2, 0, 3, 1], tag: flat_tag, offsets: flat_offsets, data: flat_data },
        ],
    };

    (file, samples)
}

fn check_chmf(b: &[u8]) {
    let Ok(f) = ChmfFile::parse(b) else { return };
    assert_eq!(f.write(), b, "a CHMF that reads writes back byte for byte");
    for i in 0..f.regions.len() {
        if let Ok((w, h, s)) = f.region_samples(i) {
            assert_eq!(s.len(), w as usize * h as usize);
        }
    }
}

#[test]
fn chmf_survives_truncation_hostile_fields_and_mutation() {
    for version in [4, 5] {
        let (file, samples) = chmf(version);
        let bytes = file.write();
        let back = ChmfFile::parse(&bytes).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.region_samples(0).unwrap(), (32, 16, samples));
        assert_eq!(back.region_samples(1).unwrap(), (16, 16, vec![777; 256]));
        for n in 0..bytes.len() {
            assert!(ChmfFile::parse(&bytes[..n]).is_err(), "a CHMF cut to {n} bytes");
        }

        torture(&bytes, 0x9E37_79B9_7F4A_7C15 + u64::from(version), 3000, check_chmf);
    }
}

#[test]
fn chmf_hostile_header_fields_are_errors() {
    let bytes = chmf(5).0.write();
    let mut magic = bytes.clone();
    magic[0] ^= 0x20;
    assert!(ChmfFile::parse(&magic).is_err(), "magic");
    for version in [0, 3, 6, u32::MAX] {
        assert!(ChmfFile::parse(&with_u32(&bytes, 4, version)).is_err(), "version {version}");
    }

    // The region count is at 36 (after the spacing), the rectangles at 40, the first region's head (tag, block count, data size) at 72.
    for v in [3, 0xFFFF, u32::MAX] {
        assert!(ChmfFile::parse(&with_u32(&bytes, 36, v)).is_err(), "region count {v}");
        assert!(ChmfFile::parse(&with_u32(&bytes, 76, v)).is_err(), "block count {v}");
        assert!(ChmfFile::parse(&with_u32(&bytes, 80, v)).is_err(), "data size {v}");
    }

    // A block offset past the data, or a region rectangle too large to be one, reads but does not decode.
    let f = ChmfFile::parse(&with_u32(&bytes, 84 + 4, u32::MAX)).unwrap();
    assert!(f.region_samples(0).is_err(), "block offset");
    for (at, v) in [(48, u32::MAX), (48, 0x0100_0000), (40, 5)] {
        let f = ChmfFile::parse(&with_u32(&bytes, at, v)).unwrap();
        assert!(f.region_samples(0).is_err(), "rectangle field at {at} = {v}");
    }

    let mut rng = Rng(7);
    let mut out = vec![];
    for _ in 0..20_000 {
        let block = (0..rng.below(80)).map(|_| rng.next() as u8).collect::<Vec<_>>();
        out.clear();
        if decode_block(&block, 256, &mut out).is_ok() {
            assert_eq!(out.len(), 256);
        }
    }
}

// PNG.

fn png(color_type: u8, bit_depth: u8) -> Vec<u8> {
    let mut png = PngFile { width: 5, height: 3, bit_depth, color_type, idat_chunk_size: 16, ..Default::default() };
    let n = 5 * 3 * png.bytes_per_pixel();
    png.pixels = (0..n).map(|i| (i * 37 % 251) as u8).collect();
    let bytes = png.encode().unwrap();
    assert_eq!(PngFile::parse(&bytes).unwrap().pixels, png.pixels);
    bytes
}

fn check_png(b: &[u8]) {
    if let Ok(p) = PngFile::parse(b) {
        assert_eq!(p.pixels.len(), p.width as usize * p.height as usize * p.bytes_per_pixel());
    }
}

#[test]
fn png_survives_truncation_hostile_fields_and_mutation() {
    torture(&png(0, 16), 11, 3000, check_png);
    torture(&png(6, 8), 12, 3000, check_png);
}

#[test]
fn png_hostile_header_fields_are_errors() {
    let bytes = png(0, 16);
    let mut signature = bytes.clone();
    signature[1] = b'Q';
    assert!(PngFile::parse(&signature).is_err(), "signature");

    // The IHDR chunk's length is at 8, its type at 12, width and height at 16 and 20 (big endian), then depth, colour type, interlace at 28.
    let be = |at: usize, v: u32| {
        let mut b = bytes.clone();
        b[at..at + 4].copy_from_slice(&v.to_be_bytes());
        b
    };
    for v in [u32::MAX, 0x8000_0000, 1 << 20] {
        assert!(PngFile::parse(&be(16, v)).is_err(), "width {v}");
        assert!(PngFile::parse(&be(20, v)).is_err(), "height {v}");
        assert!(PngFile::parse(&be(8, v)).is_err(), "IHDR length {v}");
    }

    let mut b = be(16, u32::MAX);
    b[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(PngFile::parse(&b).is_err(), "width and height");
    let mut b = bytes.clone();
    b[15] = b'X';
    assert!(PngFile::parse(&b).is_err(), "no IHDR");
    for (at, v, what) in [(24, 4, "depth 4"), (24, 0, "depth 0"), (25, 3, "palette"), (25, 9, "colour type 9"), (28, 1, "interlaced")] {
        let mut b = bytes.clone();
        b[at] = v;
        assert!(PngFile::parse(&b).is_err(), "{what}");
    }
}

// TGA, TIFF and the DDS container.

fn tga() -> Vec<u8> {
    let mut b = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 32, 8];
    b.extend((0..16).map(|i| i as u8));
    b
}

fn tiff() -> Vec<u8> {
    let mut b = TiffFile::ted_header(4, 2);
    b.extend((0..4 * 2 * 32).map(|i| i as u8));
    b
}

fn check_containers(b: &[u8]) {
    if let Ok(t) = TgaFile::parse(b) {
        assert_eq!(t.write(), b);
    }

    if let Ok(t) = TiffFile::parse(b) {
        assert_eq!((t.write().as_slice(), t.header.len()), (b, t.strip_offset as usize));
    }

    if let Ok(d) = DdsFile::parse(b) {
        assert_eq!(d.write(), b);
    }
}

#[test]
fn image_containers_survive_truncation_hostile_fields_and_mutation() {
    let (tga, tiff) = (tga(), tiff());
    assert_eq!((TgaFile::parse(&tga).unwrap().width(), TgaFile::parse(&tga).unwrap().bits_per_pixel()), (2, 32));
    let t = TiffFile::parse(&tiff).unwrap();
    assert!(t.header_is_regenerable() && (t.width, t.height) == (4, 2));
    let dds = dds(*b"DXT5", 8, 8, 2, 80);
    assert_eq!(DdsFile::parse(&dds).unwrap().payload.len(), 80);
    for (i, b) in [tga, tiff, dds].iter().enumerate() {
        torture(b, 21 + i as u64, 3000, check_containers);
    }
}

#[test]
fn image_container_hostile_fields_are_errors() {
    let tga = tga();
    for n in 0..18 {
        assert!(TgaFile::parse(&tga[..n]).is_err(), "a TGA cut to {n} bytes");
    }

    for kind in [0, 9, 11, 255] {
        let mut b = tga.clone();
        b[2] = kind;
        assert!(TgaFile::parse(&b).is_err(), "TGA type {kind}");
    }

    // TIFF: the directory offset at 4, its entry count at 8, the entries from 10 (tag, kind, count, value); 258 (bits per sample) is the
    // fourth, with 16 values out of line; 273 (the strip offset) the seventh.
    let tiff = tiff();
    let mut magic = tiff.clone();
    magic[0] = b'M';
    assert!(TiffFile::parse(&magic).is_err(), "TIFF magic");
    for v in [u32::MAX, 0x8000_0000, tiff.len() as u32] {
        assert!(TiffFile::parse(&with_u32(&tiff, 4, v)).is_err(), "directory offset {v}");
        assert!(TiffFile::parse(&with_u32(&tiff, 10 + 12 * 3 + 8, v)).is_err(), "bits per sample offset {v}");
    }

    for v in [u32::MAX, 0x8000_0000, tiff.len() as u32 + 1, 0] {
        assert!(TiffFile::parse(&with_u32(&tiff, 10 + 12 * 6 + 8, v)).is_err(), "strip offset {v}");
    }

    let mut many = tiff.clone();
    many[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    assert!(TiffFile::parse(&many).is_err(), "65535 entries");

    let dds = dds(*b"DXT1", 4, 4, 1, 8);
    assert!(DdsFile::parse(&dds[..127]).is_err(), "a DDS cut in its header");
    assert!(DdsFile::parse(&with_u32(&dds, 4, 0)).is_err(), "DDS header size");
    let mut dx10 = with_u32(&dds, 84, u32::from_le_bytes(*b"DX10"));
    dx10.truncate(140);
    assert!(DdsFile::parse(&dx10).is_err(), "a DDS cut in its DX10 header");
}

// DDS decoding.

/// A DDS of `w` x `h` with `mips` levels: a fourcc (`DXT1`, `DXT3`, `DXT5`), or `BGRA` / `RGBA` for 32-bit RGB; `payload` bytes of pixels.
fn dds(format: [u8; 4], w: u32, h: u32, mips: u32, payload: usize) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[..4].copy_from_slice(b"DDS ");
    let mut set = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    set(4, 124);
    set(12, h);
    set(16, w);
    set(28, mips);
    set(76, 32);
    match &format {
        b"BGRA" | b"RGBA" => {
            set(80, 0x41);
            set(88, 32);
            let (r, bl) = if &format == b"BGRA" { (0x00FF_0000, 0xFF) } else { (0xFF, 0x00FF_0000) };
            set(92, r);
            set(96, 0xFF00);
            set(100, bl);
            set(104, 0xFF00_0000);
        }
        _ => {
            set(80, 4);
            b[84..88].copy_from_slice(&format);
        }
    }

    b.extend((0..payload).map(|i| (i * 29 % 256) as u8));
    b
}

/// The formats, each with a size, its mips and their payload size.
fn dds_files() -> Vec<Vec<u8>> {
    vec![
        dds(*b"DXT1", 8, 8, 2, 4 * 8 + 8),
        dds(*b"DXT3", 8, 4, 2, 2 * 16 + 16),
        dds(*b"DXT5", 8, 8, 2, 4 * 16 + 16),
        dds(*b"BGRA", 4, 2, 2, 4 * 2 * 4 + 2 * 4),
        dds(*b"RGBA", 3, 3, 1, 3 * 3 * 4),
    ]
}

fn check_dds_decode(b: &[u8]) {
    let info = dds_info(b);
    assert!(info.mips >= 1);
    for mip in [0, 1, 2, 99] {
        if let Ok(img) = dds_decode_mip(b, mip) {
            assert_eq!(img.px.len(), img.w as usize * img.h as usize * 4, "mip {mip} of {} x {}", img.w, img.h);
            if img.w > 0 && img.h > 0 {
                assert_eq!(img.at(img.w - 1, img.h - 1).len(), 4);
            }
        }
    }
}

#[test]
fn dds_decode_survives_truncation_hostile_fields_and_mutation() {
    for (i, b) in dds_files().iter().enumerate() {
        let info = dds_info(b);
        let last = dds_decode_mip(b, 99).unwrap();
        let expected = if info.mips == 1 { (info.width, info.height) } else { ((info.width / 2).max(1), (info.height / 2).max(1)) };
        assert_eq!((last.w, last.h), expected);
        for n in 0..b.len() {
            assert!(dds_decode_mip(&b[..n], 99).is_err(), "{} cut to {n} bytes", info.format);
        }

        torture(b, 31 + i as u64, 2000, check_dds_decode);
    }
}

#[test]
fn dds_decode_hostile_header_fields_are_errors() {
    for b in dds_files() {
        let mut magic = b.clone();
        magic[3] = b'X';
        assert!(dds_decode_mip(&magic, 0).is_err(), "magic");
        for v in [u32::MAX, 0x8000_0000, 1 << 20] {
            assert!(dds_decode_mip(&with_u32(&b, 12, v), 0).is_err(), "height {v}");
            assert!(dds_decode_mip(&with_u32(&b, 16, v), 0).is_err(), "width {v}");
        }
    }

    assert!(dds_decode_mip(&dds(*b"DXT2", 4, 4, 1, 16), 0).is_err(), "an unsupported fourcc");
}

#[test]
fn dds_decode_sizes_that_overflow_are_errors() {
    for (format, side) in [(*b"DXT5", u32::MAX), (*b"DXT3", u32::MAX), (*b"BGRA", 0x8000_0000), (*b"RGBA", 0x8000_0000)] {
        let b = dds(format, side, side, 1, 64);
        run(&check_dds_decode, &b);
        assert!(dds_decode_mip(&b, 0).is_err(), "{} x {side}", String::from_utf8_lossy(&format));
    }
}

#[test]
fn dds_decode_a_huge_mip_count_is_quick() {
    let b = dds(*b"DXT1", 8, 8, u32::MAX, 40);
    let start = Instant::now();
    assert!(dds_decode_mip(&b, u32::MAX).is_err());
    assert!(start.elapsed() < Duration::from_secs(1), "{:?}", start.elapsed());
}

// Markers.

fn markers(class: &str, version: u32, records: Vec<Vec<u8>>, groups: Option<Vec<MarkerGroup>>) -> Vec<u8> {
    let f = MarkersFile {
        repository: "BASE_MARKER_REPOSITORY".into(),
        repository_id: 0x9D9BB,
        repository_u32: 1,
        repository_u16: 1,
        class_id: 0x00B7_E42E,
        class_name: class.into(),
        version,
        records,
        groups,
    };
    let bytes = f.write();
    assert_eq!(MarkersFile::parse(&bytes).unwrap(), f);
    bytes
}

/// Where a markers file's record count is (`BASE_MARKER_REPOSITORY`, its id, u32, u16, the class id, the name, the id again, the version).
fn markers_count_at(class: &str) -> usize {
    2 + 2 * 22 + 16 + 4 + 2 + 16 + 2 + 2 * class.len() + 16 + 4
}

fn point_light() -> Vec<u8> {
    let mut rec = vec![];
    for v in [10.0f32, 2.0, 30.0, 4.0, 1.0, 0.5, 0.25, 1.5] {
        put_f32(&mut rec, v);
    }

    rec.push(1);
    rec.extend_from_slice(&8u16.to_le_bytes());
    rec.extend_from_slice(b"LAT_NONE");
    rec.extend_from_slice(&[0; 16]);
    rec.extend_from_slice(&12u16.to_le_bytes());
    rec.extend_from_slice(b"WPLFT_SMOOTH");
    let groups = vec![MarkerGroup { key: vec![KeyPart::Wide("Point light".encode_utf16().collect())], indices: vec![0, 1] }];
    markers("POINT_LIGHT_MARKER", 3, vec![rec.clone(), rec], Some(groups))
}

fn polylines() -> Vec<u8> {
    let mut rec = vec![];
    put_u32(&mut rec, 2);
    for n in [2u32, 3] {
        put_u32(&mut rec, n);
        for i in 0..n * 2 {
            put_f32(&mut rec, i as f32);
        }
    }

    let groups = vec![MarkerGroup { key: vec![KeyPart::U32(4), KeyPart::U32(9)], indices: vec![0] }];
    markers("AI_HINT_POLYLINE_LIST_MARKER", 1, vec![rec], Some(groups))
}

fn separator() -> Vec<u8> {
    let mut rec = vec![];
    put_u32(&mut rec, 2);
    for v in [1.0f32, 2.0, 3.0, 4.0] {
        put_f32(&mut rec, v);
    }

    let groups = vec![MarkerGroup { key: vec![KeyPart::Narrow(b"wall".to_vec())], indices: vec![0, 0] }];
    markers("AI_SEPARATOR_MARKER", 1, vec![rec], Some(groups))
}

fn prop_decal() -> Vec<u8> {
    let mut rec = vec![0u8; 64];
    rec.extend_from_slice(&[0, 1, 0, 0, 0, 1]);
    put_u32(&mut rec, 15);
    rec.extend_from_slice(&[0, 1, 1, 1, 1]);
    put_f32(&mut rec, 0.1);
    rec.push(0);
    for v in [2.0f32, 3.0, -2.0] {
        put_f32(&mut rec, v);
    }

    markers("PROP_MARKER", 9, vec![rec], None)
}

fn check_markers(b: &[u8]) {
    let Ok(f) = MarkersFile::parse(b) else { return };
    for i in 0..f.records.len() {
        let fields = f.fields(i).expect("a record that read has fields");
        assert_eq!(record_from_fields(&f.class_name, f.version, &fields).unwrap(), f.records[i]);
    }
}

#[test]
fn markers_survive_truncation_hostile_fields_and_mutation() {
    for (i, b) in [point_light(), polylines(), separator(), prop_decal()].iter().enumerate() {
        torture(b, 41 + i as u64, 2000, check_markers);
    }
}

#[test]
fn markers_hostile_fields_are_errors() {
    let b = point_light();
    let mut name = b.clone();
    name[2] = b'X';
    assert!(MarkersFile::parse(&name).is_err(), "repository name");
    let mut id = b.clone();
    id[markers_count_at("POINT_LIGHT_MARKER") - 8] ^= 1;
    assert!(MarkersFile::parse(&id).is_err(), "class id mismatch");
    let mut zeros = b.clone();
    zeros[2 + 2 * 22] = 1;
    assert!(MarkersFile::parse(&zeros).is_err(), "an id without its 12 zero bytes");
    let at = markers_count_at("POINT_LIGHT_MARKER");
    for v in [3, 0xFFFF, i32::MAX as u32, u32::MAX] {
        assert!(MarkersFile::parse(&with_u32(&b, at, v)).is_err(), "record count {v}");
    }

    // The polyline list record: its list count right after the record count, the first list's point count after that.
    let b = polylines();
    let at = markers_count_at("AI_HINT_POLYLINE_LIST_MARKER") + 4;
    for v in [0xFFFF, i32::MAX as u32, u32::MAX] {
        assert!(MarkersFile::parse(&with_u32(&b, at, v)).is_err(), "polyline count {v}");
        assert!(MarkersFile::parse(&with_u32(&b, at + 4, v)).is_err(), "point count {v}");
        assert!(MarkersFile::parse(&with_u32(&b, b.len() - 8, v)).is_err(), "group index count {v}");
    }

    let mut b = point_light();
    let n = b.len();
    b[n - 8 - 4 - 22 - 2..n - 8 - 4 - 22].copy_from_slice(&0xFFFFu16.to_le_bytes());
    assert!(MarkersFile::parse(&b).is_err(), "a group key string of 65535 units");
}

// CAS2.

struct Cs2(Vec<u8>);

impl Cs2 {
    fn u32(&mut self, v: u32) -> &mut Self {
        put_u32(&mut self.0, v);
        self
    }

    fn f32s(&mut self, v: &[f32]) -> &mut Self {
        v.iter().for_each(|&f| put_f32(&mut self.0, f));
        self
    }

    fn s(&mut self, s: &str) -> &mut Self {
        let units = s.encode_utf16().collect::<Vec<_>>();
        self.0.extend_from_slice(&(units.len() as u16).to_le_bytes());
        units.iter().for_each(|u| self.0.extend_from_slice(&u.to_le_bytes()));
        self
    }

    /// A track of one key of `width` floats.
    fn track(&mut self, kind: u32, width: usize) -> &mut Self {
        self.u32(1).u32(kind).u32(0).f32s(&[0.0]).u32(1).f32s(&[0.0]).u32(1).f32s(&vec![0.5; width])
    }

    /// Metadata: one string attribute, one integer track, no float, vec3 or vec4 ones.
    fn metadata(&mut self) -> &mut Self {
        self.u32(1).s("key").u32(2).s("a").s("b");
        self.u32(1).s("int").track(8, 1).u32(0).u32(0).u32(0)
    }

    fn chunk(out: &mut Vec<u8>, kind: u32, payload: &Cs2) {
        put_u32(out, payload.0.len() as u32 + 8);
        put_u32(out, kind);
        out.extend_from_slice(&payload.0);
    }
}

/// A version 1.15 CS2: the header, a mesh of three vertices and one triangle, a scene of two nodes, and a chunk of another type.
fn cas2() -> Vec<u8> {
    let mut out = vec![0x01, 0x32, 0x53, 0x43];
    let mut header = Cs2(vec![]);
    header.f32s(&[1.15]).s("max").s("exporter");
    Cs2::chunk(&mut out, 0, &header);

    let mut mesh = Cs2(vec![]);
    mesh.s("wall").s("").s("").u32(0).metadata();
    mesh.u32(1).u32(11).u32(0).f32s(&[1.0]).u32(1).f32s(&[2.0]).u32(1).f32s(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    mesh.u32(1).u32(5).u32(0);
    mesh.u32(3);
    for i in 0..3 {
        mesh.f32s(&[i as f32, 0.0, 0.0]).f32s(&[0.0, 1.0, 0.0]).f32s(&[1.0; 4]).u32(1).f32s(&[0.5, 0.5, 0.0]).u32(0);
    }

    mesh.u32(1).u32(1).u32(0).u32(1).u32(2).u32(7).u32(0);
    Cs2::chunk(&mut out, 7, &mesh);

    let mut other = Cs2(vec![]);
    other.u32(0xDEAD);
    Cs2::chunk(&mut out, 3, &other);

    let mut scene = Cs2(vec![]);
    scene.u32(2);
    for (name, parent) in [("root", -1i32), ("child", 0)] {
        scene.s(name).u32(parent as u32).track(3, 3).track(6, 4).track(3, 3).s("").metadata();
    }

    Cs2::chunk(&mut out, 12, &scene);
    out
}

fn check_cas2(b: &[u8]) {
    let Ok(f) = Cas2File::parse(b) else { return };
    for m in &f.meshes {
        let n = m.positions.len();
        assert_eq!((m.normals.len(), m.colours.len(), m.uvs.len()), (n, n, n));
        assert_eq!(m.groups.iter().map(|g| g.0).sum::<usize>(), m.faces.len());
    }
}

#[test]
fn cas2_survives_truncation_hostile_fields_and_mutation() {
    let b = cas2();
    let f = Cas2File::parse(&b).unwrap();
    assert_eq!((f.version, f.meshes.len(), f.nodes.len()), (1.15, 1, 2));
    assert_eq!((f.meshes[0].name.as_str(), f.meshes[0].positions.len(), f.meshes[0].faces.as_slice()), ("wall", 3, &[[0, 1, 2]][..]));
    assert_eq!((f.nodes[1].name.as_str(), f.nodes[1].parent, f.nodes[1].rotation), ("child", 0, Some([0.5; 4])));
    torture(&b, 51, 3000, check_cas2);
}

#[test]
fn cas2_hostile_fields_are_errors() {
    let b = cas2();
    let mut magic = b.clone();
    magic[0] = 0;
    assert!(Cas2File::parse(&magic).is_err(), "magic");

    // The header chunk is at 4; the mesh chunk follows it.
    let mesh_at = 4 + u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    for v in [0, 7, u32::MAX, i32::MAX as u32, b.len() as u32] {
        assert!(Cas2File::parse(&with_u32(&b, 4, v)).is_err(), "header chunk size {v}");
        assert!(Cas2File::parse(&with_u32(&b, mesh_at, v)).is_err(), "mesh chunk size {v}");
    }

    let mut name = b.clone();
    name[mesh_at + 8..mesh_at + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    assert!(Cas2File::parse(&name).is_err(), "a mesh name of 65535 units");

    // The mesh: its name (2 + 8), two empty strings, a u32, then the metadata's string attribute count.
    let meta_at = mesh_at + 8 + 10 + 2 + 2 + 4;
    for v in [0xFFFF, i32::MAX as u32, u32::MAX] {
        assert!(Cas2File::parse(&with_u32(&b, meta_at, v)).is_err(), "string attribute count {v}");
    }

    // The metadata's integer track: the attribute's name (key, 2 + 6), its list (4 + two strings of 2 + 2), the track list's count (4),
    // the track's name (2 + 6), its present flag (4), then its type.
    let track_type_at = meta_at + 4 + 8 + 4 + 8 + 4 + 8 + 4;
    assert!(Cas2File::parse(&with_u32(&b, track_type_at, 3)).is_err(), "a vec3 track where an integer one goes");
}

// models_naval.

fn models_naval() -> ModelsNaval {
    let ship = ShipLogic {
        key: "att_shp_test".into(),
        rigging_file: r"VariantMeshes\_VariantModels\ships\test\logic.xml.rigging".into(),
        destruction_file: Some(String::new()),
        display_folder: r"VariantMeshes\_VariantModels\ships\test".into(),
        oars: ["four_l3".into(), "four_l2".into(), "four_l1".into()],
        selection_indicator_shape: "capsule".into(),
        cameras: vec![Camera { name: "navalcam_01".into(), matrix: [0.5; 16] }],
        bobbies: vec![Bobby {
            id: 0,
            mass: 440.5,
            radius: 1.0,
            position: [1.0, 2.0, 3.0],
            breach_radius: 0.8,
            breach_offset: [0.0, 0.83, 0.0],
            connections: vec![1, 2],
        }],
        decks: vec![Deck {
            name: "deck01".into(),
            index: 1,
            pieces: vec![DeckPiece {
                id: 1,
                corners: vec![DeckCorner { position: [0.0, 1.8, 0.0], hard: false }, DeckCorner { position: [1.0, 1.8, 0.0], hard: true }],
                plane: [0.0, 1.0, 0.0, -1.8],
            }],
        }],
        pipes: vec![Pipe { id: 31, unknown: 2, start_deck_piece: -1, end_deck_piece: 12, points: vec![[1.0; 3], [2.0; 3], [3.0; 3]] }],
        damage_parts: vec![DamagePart { name: "bow_01".into(), states: vec!["battle_sail".into()], panels: vec![vec![[1.0; 3]; 3]] }],
        collision: vec![CollisionFace { name: "c".into(), index: 1, vertices: [[0.0; 3], [1.0; 3], [2.0; 3]] }],
        ef_lines: vec![EfLine { name: "EFLine001".into(), purpose: 23, ends: [[0.0; 3], [1.0; 3]], direction: [-1.0, 0.0, 0.0] }],
        custom_positions: vec![CustomPosition {
            name: "oar_left_l3".into(),
            kind: 8,
            unknown: 1,
            position: [4.5, 1.3, -7.4],
            up: [0.0, 1.0, 0.0],
        }],
    };

    let mut other = ship.clone();
    other.destruction_file = None;
    other.pipes.clear();
    ModelsNaval { guid: Some("3b7807d8".into()), version: Some(11), marker: 1, rows: vec![ship, other] }
}

fn check_models_naval(b: &[u8]) {
    if let Ok(t) = decode(b) {
        assert_eq!(encode(&t).unwrap(), b, "a table that decodes encodes back byte for byte");
    }
}

#[test]
fn models_naval_survives_truncation_hostile_fields_and_mutation() {
    let table = models_naval();
    let b = encode(&table).unwrap();
    assert_eq!(decode(&b).unwrap(), table);
    for n in 0..b.len() {
        assert!(decode(&b[..n]).is_err(), "a table cut to {n} bytes");
    }

    torture(&b, 61, 3000, check_models_naval);
}

#[test]
fn models_naval_hostile_counts_are_errors() {
    let b = encode(&models_naval()).unwrap();

    // The GUID mark and its 8 units (4 + 2 + 16), the version mark and the version (8), the marker byte: the row count is at 31.
    for v in [3, 0xFFFF, i32::MAX as u32, u32::MAX] {
        assert!(decode(&with_u32(&b, 31, v)).is_err(), "row count {v}");
    }

    let camera_count_at = b.windows(7).position(|w| w == b"capsule").unwrap() + 7;
    for v in [2, i32::MAX as u32, u32::MAX] {
        assert!(decode(&with_u32(&b, camera_count_at, v)).is_err(), "camera count {v}");
    }

    let mut flag = b.clone();
    let destruction_at = b.windows(7).position(|w| w == b"rigging").unwrap() + 7;
    flag[destruction_at] = 2;
    assert!(decode(&flag).is_err(), "a bool of 2");
}

// Animations.

/// One bone, two frames, version 5 (the layout `anim.rs`'s tests use).
fn anim_5() -> Vec<u8> {
    let mut b = vec![];
    [5u32, 0].iter().for_each(|v| put_u32(&mut b, *v));
    put_f32(&mut b, 10.0);
    b.extend(1u16.to_le_bytes());
    b.push(b'b');
    put_f32(&mut b, 0.1);
    put_u32(&mut b, 2);
    for (name, parent) in [(b'x', -1i32), (b'y', 0)] {
        b.extend(1u16.to_le_bytes());
        b.push(name);
        put_u32(&mut b, parent as u32);
    }

    // Bone x has both tracks, bone y a rotation only.
    for v in [0i32, -1, 0, 1] {
        put_u32(&mut b, v as u32);
    }

    [1u32, 2, 2].iter().for_each(|v| put_u32(&mut b, *v));
    for (t, q) in [([0f32, 0.0, 0.0], [0i16, 0, 0, 32767]), ([2.0, 4.0, 0.0], [0, 32767, 0, 0])] {
        t.iter().for_each(|v| put_f32(&mut b, *v));
        for _ in 0..2 {
            q.iter().for_each(|v| b.extend(v.to_le_bytes()));
        }
    }

    b
}

/// One bone, two frames, version 4.
fn anim_4() -> Vec<u8> {
    let wide = |b: &mut Vec<u8>, s: &str| {
        b.extend((s.len() as u16).to_le_bytes());
        s.encode_utf16().for_each(|c| b.extend(c.to_le_bytes()));
    };

    let mut b = vec![];
    [4u32, 1].iter().for_each(|v| put_u32(&mut b, *v));
    put_f32(&mut b, 20.0);
    wide(&mut b, "man");
    put_f32(&mut b, 0.05);
    put_u32(&mut b, 1);
    wide(&mut b, "x");
    put_u32(&mut b, u32::MAX);
    [1u32, 1, 1, 1, 2].iter().for_each(|v| put_u32(&mut b, *v));
    for x in [1f32, 2.0] {
        [x, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0].iter().for_each(|v| put_f32(&mut b, *v));
    }

    b
}

fn check_anim(b: &[u8]) {
    // A bounded frame count: an unbounded one is `animation_frame_counts_are_bounded_by_the_data`'s (ignored) case.
    for max_frames in [1, 64] {
        let Some(a) = Animation::decode(b, max_frames) else { continue };
        let n = a.bones.len();
        assert_eq!((a.parents.len(), a.tracked.len()), (n, n));
        assert!(a.frames.len() <= max_frames && a.frames.iter().all(|f| f.len() == n));
        let mut c = a.clone();
        c.on_skeleton(&a);
        for t in [-1.0, 0.0, 0.05, 1e9, f32::NAN] {
            assert!(a.sample(t).len() == n || a.frames.is_empty());
        }

        let _ = a.duration();
    }
}

#[test]
fn animations_survive_truncation_hostile_fields_and_mutation() {
    let a = Animation::decode(&anim_5(), usize::MAX).unwrap();
    assert_eq!((a.bones.len(), a.frames.len(), a.tracked.clone()), (2, 2, vec![(true, true), (false, true)]));
    assert_eq!(Animation::decode(&anim_4(), usize::MAX).unwrap().frames.len(), 2);
    torture(&anim_5(), 71, 3000, check_anim);
    torture(&anim_4(), 72, 3000, check_anim);
}

#[test]
fn animation_hostile_fields_are_none() {
    for b in [anim_5(), anim_4()] {
        for v in [3, 6, u32::MAX] {
            assert!(Animation::decode(&with_u32(&b, 0, v), usize::MAX).is_none(), "version {v}");
        }

        let mut name = b.clone();
        name[12..14].copy_from_slice(&0xFFFFu16.to_le_bytes());
        assert!(Animation::decode(&name, usize::MAX).is_none(), "a skeleton name of 65535 characters");
    }

    // The bone count: after the skeleton name ("b", 3 bytes; "man", 8) and the end time.
    for (b, at) in [(anim_5(), 12 + 3 + 4), (anim_4(), 12 + 8 + 4)] {
        for v in [3, 0xFFFF, i32::MAX as u32, u32::MAX] {
            assert!(Animation::decode(&with_u32(&b, at, v), usize::MAX).is_none(), "bone count {v}");
        }
    }

    // Version 5's track counts and frame count follow the two track tables (bone count, two bones of 3 + 4 bytes, 16 bytes of tables).
    let b = anim_5();
    let counts_at = 12 + 3 + 4 + 4 + 14 + 16;
    for (i, v) in [(0, u32::MAX), (1, u32::MAX), (2, u32::MAX), (0, 0x4000_0000), (2, i32::MAX as u32)] {
        assert!(Animation::decode(&with_u32(&b, counts_at + 4 * i, v), usize::MAX).is_none(), "count {i} = {v}");
    }
}

#[test]
fn animation_frame_counts_are_bounded_by_the_data() {
    // Version 5, no bones, no tracks, u32::MAX frames.
    let mut v5 = vec![];
    [5u32, 0, 0, 0, 0, 0, 0, u32::MAX].iter().for_each(|v| put_u32(&mut v5, *v));
    v5.splice(12..12, [0u8, 0]);

    // Version 4, no bones (the two bit sets of no words), u32::MAX frames.
    let mut v4 = vec![];
    [4u32, 0, 0].iter().for_each(|v| put_u32(&mut v4, *v));
    v4.extend([0, 0]);
    [0u32, 0, 0, 0, u32::MAX].iter().for_each(|v| put_u32(&mut v4, *v));

    // A million frames as the limit keeps a failure at megabytes; the callers that pass usize::MAX would ask for about 100 GB.
    for b in [v5, v4] {
        let frames = Animation::decode(&b, 1_000_000).map_or(0, |a| a.frames.len());
        assert!(frames <= b.len(), "{} bytes gave {frames} frames", b.len());
    }
}
