use wce_formats::lf_region_png::{MAX_SAMPLES, decode, encode};
use wce_formats::png_file::PngFile;

fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(payload);
    let crc =
        unsafe { libz_sys::crc32(0, bytes[4..].as_ptr(), (bytes.len() - 4) as libz_sys::uInt) }
            as u32;
    bytes.extend(crc.to_be_bytes());
    bytes
}

#[test]
fn all_u16_values_roundtrip_without_normalization_and_top_row_first() {
    let values: Vec<u16> = (0..=65535).collect();
    let png = encode(256, 256, &values).unwrap();
    let decoded = decode(&png).unwrap();
    assert_eq!((decoded.width, decoded.height), (256, 256));
    assert_eq!(decoded.samples, values);
    let png = encode(3, 2, &[0x1234, 1, 65535, 40000, 256, 0]).unwrap();
    assert_eq!(
        PngFile::parse(&png).unwrap().pixels,
        [0x12, 0x34, 0, 1, 255, 255, 0x9c, 0x40, 1, 0, 0, 0]
    );
}

#[test]
fn rejects_conversion_formats_including_alpha_and_interlace() {
    for (depth, color, pixels) in [
        (8, 0, vec![4]),
        (16, 2, vec![0; 6]),
        (16, 4, vec![0; 4]),
        (8, 6, vec![0; 4]),
    ] {
        let png = PngFile {
            width: 1,
            height: 1,
            bit_depth: depth,
            color_type: color,
            pixels,
            ..Default::default()
        }
        .encode()
        .unwrap();
        assert!(decode(&png).is_err());
    }
    let good = encode(1, 1, &[5]).unwrap();
    let mut header = good[16..29].to_vec();
    header[12] = 1;
    let mut png = good[..8].to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(&good[33..]);
    assert!(decode(&png).unwrap_err().0.contains("non-interlaced"));
}

#[test]
fn validates_limits_before_decoding_or_allocating_pixels() {
    assert!(encode(0, 1, &[]).is_err());
    assert!(encode(1, 0, &[]).is_err());
    assert!(encode(2, 2, &[0; 3]).is_err());
    let good = encode(1, 1, &[0]).unwrap();
    for (w, h) in [(u32::MAX, u32::MAX), (1025, 1024), (0, 1)] {
        let mut header = good[16..29].to_vec();
        header[..4].copy_from_slice(&w.to_be_bytes());
        header[4..8].copy_from_slice(&h.to_be_bytes());
        let mut png = good[..8].to_vec();
        png.extend(chunk(b"IHDR", &header));
        png.extend(&good[33..]);
        assert!(decode(&png).unwrap_err().0.contains("samples"));
    }
    let at_limit = vec![0x3456; MAX_SAMPLES];
    assert_eq!(
        decode(&encode(1024, 1024, &at_limit).unwrap())
            .unwrap()
            .samples,
        at_limit
    );
}

#[test]
fn malformed_chunks_crcs_and_streams_are_errors_not_panics() {
    let png = encode(2, 1, &[1, 65535]).unwrap();
    for n in 0..png.len() {
        assert!(decode(&png[..n]).is_err(), "truncation {n}");
    }
    let mut bad = png.clone();
    bad[40] ^= 1;
    assert!(decode(&bad).is_err());
    let mut bad = png.clone();
    bad.extend(b"trailing");
    assert!(decode(&bad).is_err());
    let mut bad = png[..33].to_vec();
    bad.extend(chunk(b"IDAT", &[1, 2, 3]));
    bad.extend(chunk(b"IEND", &[]));
    assert!(decode(&bad).is_err());
    for extra in [
        chunk(b"IHDR", &png[16..29]),
        chunk(b"PLTE", &[0, 0, 0]),
        chunk(b"tRNS", &[0, 0]),
    ] {
        let mut bad = png[..33].to_vec();
        bad.extend(extra);
        bad.extend(&png[33..]);
        assert!(decode(&bad).is_err());
    }
}

#[test]
fn ancillary_metadata_never_changes_numeric_samples() {
    let good = encode(2, 1, &[123, 45678]).unwrap();
    let mut png = good[..33].to_vec();
    png.extend(chunk(b"gAMA", &100000u32.to_be_bytes()));
    png.extend(chunk(b"tEXt", b"Description\0raw LF units"));
    png.extend(&good[33..]);
    assert_eq!(decode(&png).unwrap().samples, [123, 45678]);
}
