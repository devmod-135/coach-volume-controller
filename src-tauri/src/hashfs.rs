//! Minimal read-only reader for SCS HashFS v2 archives (.scs, ETS2/ATS 1.50+).

use crate::tr;
use flate2::read::ZlibDecoder;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

// CityHash64 (v1.1), used by SCS HashFS to hash entry paths.
// Copyright (c) 2011 Google, Inc.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

const K0: u64 = 0xC3A5C85C97CB3127;
const K1: u64 = 0xB492B66FBE98F273;
const K2: u64 = 0x9AE16A3B2F90404F;
const K3: u64 = 0xC949D7C7509E6557;

fn f64_(s: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(s[i..i + 8].try_into().unwrap())
}
fn f32_(s: &[u8], i: usize) -> u64 {
    u32::from_le_bytes(s[i..i + 4].try_into().unwrap()) as u64
}
fn rot(v: u64, n: u32) -> u64 {
    if n == 0 { v } else { v.rotate_right(n) }
}
fn smix(v: u64) -> u64 {
    v ^ (v >> 47)
}
fn h16(u: u64, v: u64) -> u64 {
    let mul = 0x9DDFEA08EB382D69u64;
    let mut a = (u ^ v).wrapping_mul(mul);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(mul);
    b ^= b >> 47;
    b.wrapping_mul(mul)
}
fn h0to16(s: &[u8]) -> u64 {
    let n = s.len();
    if n > 8 {
        let a = f64_(s, 0);
        let b = f64_(s, n - 8);
        return h16(a, rot(b.wrapping_add(n as u64), n as u32)) ^ b;
    }
    if n >= 4 {
        let a = f32_(s, 0);
        return h16((n as u64).wrapping_add(a << 3), f32_(s, n - 4));
    }
    if n > 0 {
        let (a, b, c) = (s[0] as u32, s[n >> 1] as u32, s[n - 1] as u32);
        let y = a.wrapping_add(b << 8);
        let z = (n as u32).wrapping_add(c << 2);
        return smix((y as u64).wrapping_mul(K2) ^ (z as u64).wrapping_mul(K3)).wrapping_mul(K2);
    }
    K2
}
fn h17to32(s: &[u8]) -> u64 {
    let n = s.len();
    let a = f64_(s, 0).wrapping_mul(K1);
    let b = f64_(s, 8);
    let c = f64_(s, n - 8).wrapping_mul(K2);
    let d = f64_(s, n - 16).wrapping_mul(K0);
    h16(
        rot(a.wrapping_sub(b), 43).wrapping_add(rot(c, 30)).wrapping_add(d),
        a.wrapping_add(rot(b ^ K3, 20)).wrapping_sub(c).wrapping_add(n as u64),
    )
}
fn h33to64(s: &[u8]) -> u64 {
    let n = s.len();
    let z = f64_(s, 24);
    let mut a = f64_(s, 0).wrapping_add((n as u64).wrapping_add(f64_(s, n - 16)).wrapping_mul(K0));
    let mut b = rot(a.wrapping_add(z), 52);
    let mut c = rot(a, 37);
    a = a.wrapping_add(f64_(s, 8));
    c = c.wrapping_add(rot(a, 7));
    a = a.wrapping_add(f64_(s, 16));
    let vf = a.wrapping_add(z);
    let vs = b.wrapping_add(rot(a, 31)).wrapping_add(c);
    a = f64_(s, 16).wrapping_add(f64_(s, n - 32));
    let z = f64_(s, n - 8);
    b = rot(a.wrapping_add(z), 52);
    c = rot(a, 37);
    a = a.wrapping_add(f64_(s, n - 24));
    c = c.wrapping_add(rot(a, 7));
    a = a.wrapping_add(f64_(s, n - 16));
    let wf = a.wrapping_add(z);
    let ws = b.wrapping_add(rot(a, 31)).wrapping_add(c);
    let r = smix(vf.wrapping_add(ws).wrapping_mul(K2).wrapping_add(wf.wrapping_add(vs).wrapping_mul(K0)));
    smix(r.wrapping_mul(K0).wrapping_add(vs)).wrapping_mul(K2)
}
fn weak(s: &[u8], pos: usize, a: u64, b: u64) -> (u64, u64) {
    let (w, x, y, z) = (f64_(s, pos), f64_(s, pos + 8), f64_(s, pos + 16), f64_(s, pos + 24));
    let mut a = a.wrapping_add(w);
    let mut b = rot(b.wrapping_add(a).wrapping_add(z), 21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(rot(a, 44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

/// CityHash64 (v1.1), as used by SCS HashFS.
pub fn cityhash64(s: &[u8]) -> u64 {
    let n = s.len();
    if n <= 16 {
        return h0to16(s);
    }
    if n <= 32 {
        return h17to32(s);
    }
    if n <= 64 {
        return h33to64(s);
    }
    let mut x = f64_(s, n - 40);
    let mut y = f64_(s, n - 16).wrapping_add(f64_(s, n - 56));
    let mut z = h16(f64_(s, n - 48).wrapping_add(n as u64), f64_(s, n - 24));
    let mut v = weak(s, n - 64, n as u64, z);
    let mut w = weak(s, n - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(f64_(s, 0));
    let mut left = (n - 1) & !63;
    let mut pos = 0;
    loop {
        x = rot(x.wrapping_add(y).wrapping_add(v.0).wrapping_add(f64_(s, pos + 8)), 37).wrapping_mul(K1);
        y = rot(y.wrapping_add(v.1).wrapping_add(f64_(s, pos + 48)), 42).wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(f64_(s, pos + 40));
        z = rot(z.wrapping_add(w.0), 33).wrapping_mul(K1);
        v = weak(s, pos, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak(s, pos + 32, z.wrapping_add(w.1), y.wrapping_add(f64_(s, pos + 16)));
        std::mem::swap(&mut z, &mut x);
        pos += 64;
        left -= 64;
        if left == 0 {
            break;
        }
    }
    h16(
        h16(v.0, w.0).wrapping_add(smix(y).wrapping_mul(K1)).wrapping_add(z),
        h16(v.1, w.1).wrapping_add(x),
    )
}

struct Entry {
    meta_idx: u32,
    meta_cnt: u16,
    is_dir: bool,
}

pub struct HashFS {
    file: Mutex<File>,
    salt: u16,
    meta: Vec<u32>,
    entries: HashMap<u64, Entry>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

fn inflate(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    ZlibDecoder::new(raw).read_to_end(&mut out).map_err(|e| tr!("압축 해제 실패: {e}", "Decompression failed: {e}"))?;
    Ok(out)
}

impl HashFS {
    pub fn open(path: &str) -> Result<Self, String> {
        let mut f = File::open(path).map_err(|e| format!("{path}: {e}"))?;
        let mut head = [0u8; 0x40];
        f.read_exact(&mut head).map_err(|e| format!("{path}: {e}"))?;
        if &head[0..4] != b"SCS#" {
            return Err(tr!("{path}: SCS 아카이브가 아닙니다", "{path}: not an SCS archive"));
        }
        let ver = u16_at(&head, 4);
        if ver != 2 {
            return Err(tr!("{path}: HashFS v{ver} 는 지원하지 않습니다 (v2 만 지원)", "{path}: HashFS v{ver} is not supported (v2 only)"));
        }
        if &head[8..12] != b"CITY" {
            return Err(tr!("{path}: 알 수 없는 해시 방식", "{path}: unknown hash method"));
        }
        let salt = u16_at(&head, 6);
        let n_entries = u32_at(&head, 12) as usize;
        let et_size = u32_at(&head, 16) as usize;
        let mt_size = u32_at(&head, 24) as usize;
        let et_off = u64_at(&head, 28);
        let mt_off = u64_at(&head, 36);

        let mut read_block = |off: u64, size: usize| -> Result<Vec<u8>, String> {
            f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; size];
            f.read_exact(&mut buf).map_err(|e| e.to_string())?;
            inflate(&buf)
        };
        let et = read_block(et_off, et_size)?;
        let mt = read_block(mt_off, mt_size)?;
        let meta = mt.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let mut entries = HashMap::with_capacity(n_entries);
        for i in 0..n_entries {
            let o = i * 16;
            if o + 16 > et.len() {
                break;
            }
            entries.insert(
                u64_at(&et, o),
                Entry { meta_idx: u32_at(&et, o + 8), meta_cnt: u16_at(&et, o + 12), is_dir: u16_at(&et, o + 14) & 1 != 0 },
            );
        }
        Ok(Self { file: Mutex::new(f), salt, meta, entries })
    }

    fn hash(&self, path: &str) -> u64 {
        let p = path.trim_matches('/');
        if self.salt != 0 {
            cityhash64(format!("{}{}", self.salt, p).as_bytes())
        } else {
            cityhash64(p.as_bytes())
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.entries.contains_key(&self.hash(path))
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.entries.get(&self.hash(path)).map_or(false, |e| e.is_dir)
    }

    /// (compressed size, size, compression flag, offset)
    fn plain_meta(&self, e: &Entry) -> Result<(usize, usize, u32, u64), String> {
        for i in 0..e.meta_cnt as usize {
            let it = *self.meta.get(e.meta_idx as usize + i).ok_or_else(|| tr!("메타데이터 범위 오류", "Metadata out of range"))?;
            let (idx, typ) = ((it & 0x00FF_FFFF) as usize, it >> 24);
            if typ == 0x80 || typ == 0x81 {
                let w = self.meta.get(idx..idx + 4).ok_or_else(|| tr!("메타데이터 범위 오류", "Metadata out of range"))?;
                return Ok(((w[0] & 0x0FFF_FFFF) as usize, (w[1] & 0x0FFF_FFFF) as usize, w[0] >> 28, w[3] as u64 * 16));
            }
        }
        Err(tr!("평문 메타데이터가 없는 항목입니다 (텍스처 등은 지원하지 않음)", "Entry has no plain metadata (textures etc. are not supported)"))
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let e = self.entries.get(&self.hash(path)).ok_or_else(|| tr!("파일 없음: {path}", "File not found: {path}"))?;
        let (comp_size, size, flag, offset) = self.plain_meta(e)?;
        let mut raw = vec![0u8; comp_size];
        {
            let mut f = self.file.lock().unwrap();
            f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
            f.read_exact(&mut raw).map_err(|e| e.to_string())?;
        }
        if flag == 1 || (flag == 0 && comp_size != size) {
            return inflate(&raw);
        }
        if flag != 0 {
            return Err(tr!("지원하지 않는 압축 방식 {flag}: {path}", "Unsupported compression {flag}: {path}"));
        }
        Ok(raw)
    }

    /// Returns (subdirs, files).
    pub fn listdir(&self, path: &str) -> Result<(Vec<String>, Vec<String>), String> {
        let data = self.read(path)?;
        if data.len() < 4 {
            return Ok((vec![], vec![]));
        }
        let count = u32_at(&data, 0) as usize;
        let lens = data.get(4..4 + count).ok_or_else(|| tr!("디렉터리 형식 오류", "Bad directory listing"))?;
        let mut pos = 4 + count;
        let (mut dirs, mut files) = (vec![], vec![]);
        for &ln in lens {
            let ln = ln as usize;
            let name = String::from_utf8_lossy(data.get(pos..pos + ln).ok_or_else(|| tr!("디렉터리 형식 오류", "Bad directory listing"))?).into_owned();
            pos += ln;
            match name.strip_prefix('/') {
                Some(d) => dirs.push(d.to_string()),
                None => files.push(name),
            }
        }
        Ok((dirs, files))
    }
}
