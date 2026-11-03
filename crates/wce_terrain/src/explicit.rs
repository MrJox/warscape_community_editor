//! `explicit_tiles.txt`: tiles placed by hand, one per line, `x,y,tile path,rotation degrees` (BOB's fill places them first,
//! docs/re/battle_maps.md). Kept line by line with each line's own ending, so an unedited file writes back as it was and an edit rewrites
//! only its line. Lines that do not parse are kept as they are.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplicitTile {
    pub x: i32,
    pub y: i32,

    /// `terrain\tiles\battle\<set>\<tile>\<variation>` or a tile's path (BOB draws a variation then).
    pub path: String,
    pub rotation: i32,
}

impl ExplicitTile {
    fn parse(line: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(line).ok()?;
        let mut it = s.split(',');
        let (x, y, path, rotation) = (it.next()?, it.next()?, it.next()?, it.next()?);
        if it.next().is_some() {
            return None;
        }

        Some(Self { x: x.trim().parse().ok()?, y: y.trim().parse().ok()?, path: path.trim().to_owned(), rotation: rotation.trim().parse().ok()? })
    }

    fn text(&self) -> Vec<u8> {
        format!("{},{},{},{}", self.x, self.y, self.path, self.rotation).into_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    raw: Vec<u8>,
    eol: Vec<u8>,
    tile: Option<ExplicitTile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplicitTiles {
    lines: Vec<Line>,
}

impl ExplicitTiles {
    pub fn parse(b: &[u8]) -> Self {
        let mut lines = vec![];
        let mut start = 0;
        while start < b.len() {
            let end = b[start..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| start + p + 1);
            let line = &b[start..end];
            let body = line.len() - line.iter().rev().take_while(|&&c| c == b'\n' || c == b'\r').count();
            lines.push(Line { raw: line[..body].to_vec(), eol: line[body..].to_vec(), tile: ExplicitTile::parse(&line[..body]) });
            start = end;
        }

        Self { lines }
    }

    pub fn write(&self) -> Vec<u8> {
        self.lines.iter().flat_map(|l| l.raw.iter().chain(&l.eol).copied()).collect()
    }

    /// The lines BOB's reader rejects (1-based): those that are no `x,y,path,rotation`, and the empty line after a last line ending.
    /// One fails BOB's `Tilemap` ("explicit_tile.txt error line N", "Conversion of terrain failed").
    pub fn bad_lines(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.lines.iter().enumerate().filter(|(_, l)| l.tile.is_none()).map(|(i, _)| i + 1).collect();
        if self.lines.last().is_some_and(|l| !l.eol.is_empty()) {
            out.push(self.lines.len() + 1);
        }

        out
    }

    /// The tiles, in the file's order.
    pub fn tiles(&self) -> impl Iterator<Item = &ExplicitTile> {
        self.lines.iter().filter_map(|l| l.tile.as_ref())
    }

    /// Replaces the `i`-th tile; false when there is none.
    pub fn set(&mut self, i: usize, tile: ExplicitTile) -> bool {
        let Some(l) = self.lines.iter_mut().filter(|l| l.tile.is_some()).nth(i) else { return false };
        l.raw = tile.text();
        l.tile = Some(tile);
        true
    }

    /// Adds a tile at the end, with the file's line ending (CRLF for a new file, as the kit's). The file keeps how it ends: the kit's
    /// end without a line ending after the last line, and BOB's reader fails on an empty last line ("explicit_tile.txt error line 3").
    pub fn push(&mut self, tile: ExplicitTile) {
        let eol = self.lines.iter().rev().find(|l| !l.eol.is_empty()).map_or_else(|| b"\r\n".to_vec(), |l| l.eol.clone());
        let after = match self.lines.last_mut() {
            Some(last) if last.eol.is_empty() => {
                last.eol.clone_from(&eol);
                vec![]
            }

            Some(_) => eol,
            None => vec![],
        };

        self.lines.push(Line { raw: tile.text(), eol: after, tile: Some(tile) });
    }

    /// Removes the `i`-th tile and its line; false when there is none. The last line gone, the one before ends the file as it did.
    pub fn remove(&mut self, i: usize) -> bool {
        let Some(k) = self.lines.iter().enumerate().filter(|(_, l)| l.tile.is_some()).nth(i).map(|(k, _)| k) else { return false };
        let gone = self.lines.remove(k);
        if k == self.lines.len()
            && let Some(last) = self.lines.last_mut()
        {
            last.eol = gone.eol;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_edit_one_line() {
        let text = b"616,289,terrain\\tiles\\battle\\dense_forest_rugged\\8x8\\aa,270\r\n608,287,x\\y,90\r\nnot a tile\r\n239,193,a,180";
        let mut f = ExplicitTiles::parse(text);
        assert_eq!(f.write(), text);
        assert_eq!(f.tiles().count(), 3);
        assert_eq!(f.bad_lines(), vec![3]);
        assert_eq!(ExplicitTiles::parse(b"1,1,a,0\r\n").bad_lines(), vec![2]);
        assert!(f.set(1, ExplicitTile { x: 1, y: 2, path: "p".to_owned(), rotation: 0 }));
        assert_eq!(f.write(), b"616,289,terrain\\tiles\\battle\\dense_forest_rugged\\8x8\\aa,270\r\n1,2,p,0\r\nnot a tile\r\n239,193,a,180".to_vec());
        f.push(ExplicitTile { x: 3, y: 4, path: "q".to_owned(), rotation: 90 });
        assert!(f.write().ends_with(b"239,193,a,180\r\n3,4,q,90"), "no line ending after the last, as the file had none");
        assert!(f.remove(3));
        assert!(f.write().ends_with(b"239,193,a,180"));

        let mut new = ExplicitTiles::default();
        new.push(ExplicitTile { x: 1, y: 1, path: "a".to_owned(), rotation: 0 });
        new.push(ExplicitTile { x: 2, y: 2, path: "b".to_owned(), rotation: 90 });
        assert_eq!(new.write(), b"1,1,a,0\r\n2,2,b,90".to_vec());
    }
}
