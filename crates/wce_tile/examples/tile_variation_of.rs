//! `tile_variation_of <tile.wce> <variation> <out>`: one variation's tile.wce out of a combined one (`variations::to_variation`), to
//! compare what an edit did to each variation (docs/plans/tile_variations.md).

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let [_, file, variation, out] = &args[..] else {
        eprintln!("usage: tile_variation_of <tile.wce> <variation> <out>");
        std::process::exit(2);
    };

    let text = std::fs::read(file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let one = wce_tile::variations::to_variation(&text, variation).unwrap_or_else(|e| panic!("{file} {variation}: {e}"));
    std::fs::write(out, one).unwrap_or_else(|e| panic!("{out}: {e}"));
}
