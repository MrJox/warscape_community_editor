# Wwise sound files (`.wem`)

Read and written by `wce_audio_format::wem`; gate `audio_formats_check`. Checked 2026-11-19: all 44,802 WEMs of Attila + TDD v1.2.0
and 42,371 of Rome 2 are Wwise Vorbis and written back byte for byte, and every one's codebook hash is the FNV-1 of its setup packet.

## Container

RIFF `WAVE`: `fmt ` (0x42 bytes), `data`, and in some files `JUNK` + `akd `, or `LIST` + `cue ` (+ `smpl` for loops). Odd chunks
are padded to even.

`fmt `: u16 codec (0xFFFF Wwise Vorbis), u16 channels, u32 rate, u32 average bytes per second (data size x rate / frames), u16
block align 0, u16 bits 0, u16 extension size 0x30, u16 (0 from Wwise 2014.1 for 16-bit sources; 16 or 24 in some multichannel
vanilla files: metadata), u32 channel mask (mono 0x4, stereo 0x3, quad 0x603, 5 channels 0x607, 5.1 0x60F), then the 0x2A-byte
Vorbis header:

| Offset | Field | Value |
|---|---|---|
| 0x00 | total PCM frames | |
| 0x04 | loop start packet offset | the setup packet's size with its 2-byte header: the first audio packet, relative to the end of the seek table |
| 0x08 | loop end packet offset | data size minus the seek table |
| 0x0C | u16 loop begin extra, u16 loop end extra | 0, last granule extra |
| 0x10 | seek table size | 0 (Wwise's Voice preset), 0x80 (Music preset) |
| 0x14 | Vorbis data offset | seek table + setup packet |
| 0x18 | u16 max packet size, u16 last granule extra | largest audio packet; samples decoded past the frame count |
| 0x1C, 0x20 | decoder alloc size, x64 alloc size | fixed per setup (the same for any channel count) |
| 0x24 | codebook hash | FNV-1 32 (no lower-casing, `fnv::bytes`) of the setup packet's payload |
| 0x28, 0x29 | blocksize exponents | 8/11 in most files |

`data`: [seek table], the setup packet (u16 size + payload), then audio packets (u16 size + payload; no granule).

## Wwise Vorbis

A Vorbis stream with Wwise's packing (what ww2ogg undoes; the encoder does the reverse):

- The setup packet drops the `vorbis` header, the time section, the floor types (always 1), the mappings' types, the modes' window
  and transform types and the framing bit, and writes residue types in 2 bits. Codebooks are 10-bit IDs into Wwise's built-in
  library (aoTuV 6.03's books, 598 entries; ww2ogg's `packed_codebooks_aoTuV_603.bin`). Vanilla setups use 18 to 44 codebooks, IDs
  up to 462; all of vanilla uses 23 distinct setups.
- 26 codebook contents are in the library twice. Wwise uses the copies that keep a quality level's block of IDs contiguous: choose
  the assignment minimising the sum of each ID's distance to its nearest other ID (ties: lowest IDs). This reproduces all 18
  distinct setups of vanilla and Wwise 2014.1.3's output.
- Audio packets drop the packet-type bit and, for long blocks, the previous/next window flags (a decoder recomputes them from the
  neighbouring packets' modes). A packet is ceil((bits - 1 - 2 for long blocks) / 8) bytes of the encoder's exact bit count.

## Setups by quality, channels and rate (oracle grid, 2026-11-19)

Wwise 2014.1.3 converting test WAVs for every Vorbis quality 0 to 10, 1, 2, 4, 5 and 6 channels and 8 rates (11,025 to 48,000 Hz):

- One setup per quality and channel layout, **whatever the sample rate**: Wwise chooses the Vorbis setup as for 44.1 kHz and writes
  the real rate in the header (so most vanilla 28 kHz mono files share setup 997642109, aoTuV's quality 0 at 44.1 kHz).
- aoTuV at quality Q/10 and 44.1 kHz gives Wwise's setup for qualities 0 and 2 to 10 with 1 to 5 channels. Quality 1 differs (Wwise's
  899852801 is not any aoTuV quality from -0.2 to 0.15: its aoTuV build differs in that band).
- More than 2 channels: Wwise uses the uncoupled setup, the mono one (6-channel quality 4 = mono quality 4 = 3017712712). Stock aoTuV
  picks its 5.1 template for 6 channels, whose codebooks are not in Wwise's library.
- The decoder alloc sizes depend only on the setup (the same for every channel count); the x64 size is the 32-bit one plus 8 bytes per
  codebook plus 144 (92 for the 512/512 blocksize setups). The sizes are kept in a table (`wce_audio::setup_allocs`, 35 setups: the
  grid's and vanilla's); a formula from the codebooks came close (about 4 bytes per used entry) but not exact.

## Encoding (docs/plans/audio.md, step 1)

aoTuV (libvorbis 1.3.x with aoTuV's encoder) produces Wwise's own setups: at quality 0.4 the setup of Wwise's Voice preset byte
for byte, at 0.0 vanilla's most common mono setup. Re-encoding 7 vanilla files (mono to 5.1): the same setup as Wwise's, size 0.1 to
3.1 % smaller, SNR equal or higher; not bit-identical (Wwise's encoder is a modified aoTuV). An aoTuV-encoded WEM plays in the game
(2026-11-19, the main menu's ambience stream replaced by a mod pack).
