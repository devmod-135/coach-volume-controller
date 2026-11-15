//! Read/patch per-event volume inside FMOD Studio .bank files (as shipped with ETS2/ATS).
//!
//! Only the event's mixer fader values (stored in dB as float32) are changed, in place,
//! so the bank size and the Vorbis audio data stay byte-identical.
//!
//! Layout used (FMOD Studio 2.x bank, RIFF "FEV "):
//!   LIST PROJ
//!     LIST EVTS / LIST EVNT / EVTB : event guid(16) | 16 zero | timeline guid | input bus guid | master track guid | ...
//!     LIST IBSS / LIST IBUS / IBSB(guid..) + BUS   : event input bus  (routes into the game mixer)
//!     LIST MBSS / LIST MBUS / MBSB(guid..) + BUS   : event master track
//!   BUS chunk contains "08 00 <f32 volume_dB> <f32> 01 00 ff ff ff 7f".

use crate::tr;
use std::collections::HashMap;

pub const FADER_MAX_DB: f64 = 10.0; // FMOD Studio fader range is -80 .. +10 dB
pub const FADER_MIN_DB: f64 = -80.0;

pub fn guid_from_bytes(b: &[u8]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

#[derive(Clone, Debug)]
pub struct BankEvent {
    pub guid: String,
    pub name: String,
    master_offset: usize,
    pub master_db: f64,
    input_offset: usize,
    pub input_db: f64,
}

impl BankEvent {
    pub fn total_db(&self) -> f64 {
        self.master_db + self.input_db
    }
}

pub struct Bank {
    pub data: Vec<u8>,
    pub events: Vec<BankEvent>,
}

fn read_f32(d: &[u8], off: usize) -> f64 {
    f32::from_le_bytes(d[off..off + 4].try_into().unwrap()) as f64
}

/// Offsets of the volume float for every "08 00 <f32> <f32> 01 00 ff ff ff 7f" in d[start..end]
/// (non-overlapping, like re.finditer).
fn find_volumes(d: &[u8], start: usize, end: usize) -> Vec<usize> {
    const TAIL: [u8; 6] = [0x01, 0x00, 0xff, 0xff, 0xff, 0x7f];
    let mut out = vec![];
    let mut p = start;
    while p + 16 <= end {
        if d[p] == 0x08 && d[p + 1] == 0x00 && d[p + 10..p + 16] == TAIL {
            out.push(p + 2);
            p += 16;
        } else {
            p += 1;
        }
    }
    out
}

impl Bank {
    pub fn from_bytes(raw: &[u8], names: &HashMap<String, String>) -> Result<Self, String> {
        let data = raw.to_vec();
        if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"FEV " {
            return Err(tr!("FMOD Studio bank(RIFF/FEV) 형식이 아닙니다", "Not an FMOD Studio bank (RIFF/FEV)"));
        }
        let mut buses: HashMap<[u8; 16], (&'static str, usize)> = HashMap::new();
        let mut evtbs: Vec<Vec<u8>> = vec![];
        let mut cur: Option<(&'static str, [u8; 16])> = None;

        fn walk(
            d: &[u8],
            mut off: usize,
            end: usize,
            buses: &mut HashMap<[u8; 16], (&'static str, usize)>,
            evtbs: &mut Vec<Vec<u8>>,
            cur: &mut Option<(&'static str, [u8; 16])>,
        ) {
            let end = end.min(d.len());
            while off + 8 <= end {
                let cid = &d[off..off + 4];
                let sz = u32::from_le_bytes(d[off + 4..off + 8].try_into().unwrap()) as usize;
                let body_end = (off + 8 + sz).min(d.len());
                match cid {
                    b"RIFF" | b"LIST" => walk(d, off + 12, off + 8 + sz, buses, evtbs, cur),
                    b"IBSB" | b"MBSB" | b"GBSB" | b"RBSB" => {
                        if off + 24 <= d.len() {
                            let kind = match cid {
                                b"IBSB" => "IBSB",
                                b"MBSB" => "MBSB",
                                b"GBSB" => "GBSB",
                                _ => "RBSB",
                            };
                            *cur = Some((kind, d[off + 8..off + 24].try_into().unwrap()));
                        }
                    }
                    b"BUS " if cur.is_some() => {
                        let m = find_volumes(d, off + 8, body_end);
                        let (kind, guid) = cur.take().unwrap();
                        if m.len() == 1 {
                            buses.insert(guid, (kind, m[0]));
                        }
                    }
                    b"EVTB" => evtbs.push(d[off + 8..body_end].to_vec()),
                    _ => {}
                }
                off += 8 + sz + (sz & 1);
            }
        }
        walk(&data, 0, data.len(), &mut buses, &mut evtbs, &mut cur);

        let mut events = vec![];
        for e in &evtbs {
            if e.len() < 80 {
                continue;
            }
            let g = guid_from_bytes(&e[0..16]);
            let ib = buses.get::<[u8; 16]>(&e[48..64].try_into().unwrap());
            let mb = buses.get::<[u8; 16]>(&e[64..80].try_into().unwrap());
            let (Some(ib), Some(mb)) = (ib, mb) else { continue };
            if ib.0 != "IBSB" || mb.0 != "MBSB" {
                continue; // unknown layout: leave untouched
            }
            events.push(BankEvent {
                name: names.get(&g).cloned().unwrap_or_else(|| format!("{{{g}}}")),
                guid: g,
                master_offset: mb.1,
                master_db: read_f32(&data, mb.1),
                input_offset: ib.1,
                input_db: read_f32(&data, ib.1),
            });
        }
        Ok(Self { data, events })
    }

    pub fn event(&self, name_or_guid: &str) -> Option<&BankEvent> {
        let g = name_or_guid.trim_matches(|c| c == '{' || c == '}');
        self.events.iter().find(|e| e.name == name_or_guid || e.guid == g)
    }

    /// Adds gain_db to the event. Returns the gain actually applied (after fader clamps).
    ///
    /// Gain goes to the master track first; whatever exceeds +10 dB there spills
    /// to the input bus, so up to ~+20 dB is possible on a 0 dB event.
    pub fn apply_gain(&mut self, name: &str, gain_db: f64) -> Option<f64> {
        let idx = self.events.iter().position(|e| e.name == name)?;
        let ev = &mut self.events[idx];
        let (m0, i0) = (ev.master_db, ev.input_db);
        let want = m0 + i0 + gain_db;
        let m = (m0 + gain_db).clamp(FADER_MIN_DB, FADER_MAX_DB);
        let i = (want - m).clamp(FADER_MIN_DB, FADER_MAX_DB);
        self.data[ev.master_offset..ev.master_offset + 4].copy_from_slice(&(m as f32).to_le_bytes());
        self.data[ev.input_offset..ev.input_offset + 4].copy_from_slice(&(i as f32).to_le_bytes());
        ev.master_db = m;
        ev.input_db = i;
        Some((m + i) - (m0 + i0))
    }
}

/// '.bank.guids' lines: '{guid} event:/path'  -> {guid: 'event:/path'}
pub fn parse_guids_file(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let l = line.trim_start();
        let Some(rest) = l.strip_prefix('{') else { continue };
        if rest.len() < 38 || !rest.is_char_boundary(36) || rest.as_bytes()[36] != b'}' {
            continue;
        }
        let guid = &rest[..36];
        if !guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            continue;
        }
        let tail = &rest[37..];
        if !tail.starts_with(char::is_whitespace) {
            continue;
        }
        let name = tail.trim();
        if !name.is_empty() {
            out.insert(guid.to_lowercase(), name.to_string());
        }
    }
    out
}
