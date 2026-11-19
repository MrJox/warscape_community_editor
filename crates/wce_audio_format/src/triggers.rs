//! `audio/sound_animation_triggers.csv`: the animation sound triggers. A `SOUND_TRIGGER` entry of a `.snd.meta`
//! (`wce_formats::anim_meta`) holds a 0-based data-row index into this table; the row names the sound events it fires in battle and on
//! the campaign map (`sound_event_name_battle`, `..._campaign`, their `_end` events) and its conditions (ground, entity, armour,
//! gender, weapon, shield...). The file is kept as its bytes; [`Triggers::rows`] reads the columns (plain CSV, double quotes for
//! fields with commas).

use crate::{FormatError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Triggers {
    /// The file as read; [`Triggers::write`] gives it back.
    pub bytes: Vec<u8>,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn split_line(line: &str) -> Vec<String> {
    let (mut fields, mut field, mut quoted, mut chars) = (vec![], String::new(), false, line.chars().peekable());
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }

    fields.push(field);
    fields
}

impl Triggers {
    /// Reads the table.
    ///
    /// # Errors
    ///
    /// Fails when the file is not UTF-8 or has no header.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes).map_err(|_| FormatError::Invalid("not UTF-8".into()))?;
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let header = split_line(lines.next().ok_or_else(|| FormatError::Invalid("no header".into()))?);
        let rows = lines.map(split_line).collect();
        Ok(Self { bytes: bytes.to_vec(), header, rows })
    }

    pub fn write(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// A column's index by its header name.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|h| h == name)
    }

    /// A trigger's field by its index (as a `.snd.meta` holds it) and column name.
    pub fn get(&self, trigger: usize, column: &str) -> Option<&str> {
        self.rows.get(trigger)?.get(self.column(column)?).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rows() {
        let csv = b"name,sound_event_name_battle,sound_event_name_campaign\r\nempty_slot,,\r\n\"50_Bird, Wing\",battle_bird_wing_flaps,campaign_bird_wing_flaps\r\n";
        let t = Triggers::read(csv).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.get(1, "name"), Some("50_Bird, Wing"));
        assert_eq!(t.get(1, "sound_event_name_battle"), Some("battle_bird_wing_flaps"));
        assert_eq!(t.write(), csv);
    }
}
