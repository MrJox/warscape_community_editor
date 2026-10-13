//! Tests of the kit's image formats. The PNG hashes are the C++ encoder's output for the same pixels (`image_dump` compares the two
//! over every image of the kit and the caches).

use wce_formats::image_containers::{DdsFile, TgaFile, TiffFile};
use wce_formats::png_file::PngFile;

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xCBF2_9CE4_8422_2325, |h, &c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3))
}

#[test]
fn png_encoder_matches_the_cpp_one() {
    let golden = [
        (0, 8, 214, 0x4ba1_ef85_16d3_8f98), (0, 16, 362, 0xd5b4_48c2_c44c_11d6), (2, 8, 338, 0x0984_3714_67c8_1d58),
        (2, 16, 519, 0x775d_376b_33ec_56d9), (6, 8, 394, 0x92eb_05d2_fc96_4efa), (6, 16, 533, 0x2f39_f58a_49cc_38a8),
    ];

    for (color_type, bit_depth, size, hash) in golden {
        let mut png = PngFile { width: 37, height: 23, bit_depth, color_type, idat_chunk_size: 100, ..Default::default() };
        let n = 37 * 23 * png.bytes_per_pixel();
        png.pixels = (0..n).map(|i| ((i * 7 + (i / 37) * 13 + (i * i) % 11) & 0xFF) as u8).collect();
        let encoded = png.encode().unwrap();
        assert_eq!((encoded.len(), fnv(&encoded)), (size, hash), "colour type {color_type}, depth {bit_depth}");

        let parsed = PngFile::parse(&encoded).unwrap();
        assert_eq!((parsed.pixels.as_slice(), parsed.idat_chunk_size, parsed.has_ancillary), (png.pixels.as_slice(), 100, false));
        assert_eq!(parsed.encode().unwrap(), encoded);
        assert_eq!(parsed.write().unwrap(), encoded);
    }

    assert!(PngFile::parse(b"not a png").is_err());
}

#[test]
fn containers_round_trip() {
    let payload = (0..4 * 3 * 4).map(|i| i as u8).collect::<Vec<_>>();
    let dds = DdsFile::make_r32f(4, 3, payload.clone());
    let bytes = dds.write();
    let parsed = DdsFile::parse(&bytes).unwrap();
    assert!(parsed.is_r32f_template());
    assert_eq!((parsed.width(), parsed.height(), parsed.fourcc(), parsed.payload), (4, 3, 114, payload));

    let mut dx10 = bytes.clone();
    dx10[84..88].copy_from_slice(b"DX10");
    dx10.splice(128..128, [1u8; 20]);
    let parsed = DdsFile::parse(&dx10).unwrap();
    assert!(parsed.has_dx10 && !parsed.is_r32f_template());
    assert_eq!(parsed.write(), dx10);

    let tiff = [TiffFile::ted_header(2, 1), vec![7; 2 * 32]].concat();
    let parsed = TiffFile::parse(&tiff).unwrap();
    assert!(parsed.ted_layout && parsed.header_is_regenerable());
    assert_eq!((parsed.width, parsed.height, parsed.samples_per_pixel, parsed.bits_per_sample, parsed.strip_offset), (2, 1, 16, 16, 242));
    assert_eq!(parsed.write(), tiff);
    assert_eq!(TiffFile::ted_header(2, 1).len(), 242);

    let mut tga = vec![0u8; 18];
    tga[2] = 2;
    tga[12] = 5;
    tga.extend_from_slice(&[1, 2, 3]);
    let parsed = TgaFile::parse(&tga).unwrap();
    assert_eq!((parsed.width(), parsed.write()), (5, tga.clone()));
    tga[2] = 9;
    assert!(TgaFile::parse(&tga).is_err());
}
