//! Dependency-free PNG encoder (8/16-bit, RGB or RGBA, stored deflate).
use std::sync::OnceLock;

fn crc_table() -> &'static [u32; 256] {
    static T: OnceLock<[u32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

fn crc32(parts: &[&[u8]]) -> u32 {
    let t = crc_table();
    let mut c = 0xffff_ffffu32;
    for p in parts {
        for &b in *p {
            c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xffff_ffff
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &d in data {
        a = (a + d as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

/// Encode RGBA float pixels (0..1). `depth` must be 8 or 16 (anything else becomes 8).
pub fn encode(w: usize, h: usize, px: &[[f32; 4]], depth: u8) -> Vec<u8> {
    let depth = if depth == 16 { 16 } else { 8 };
    let opaque = px.iter().all(|p| p[3] >= 0.9999);
    let ch = if opaque { 3 } else { 4 };
    let bps = (depth / 8) as usize;
    let mut raw = Vec::with_capacity(h * (1 + w * ch * bps));
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            let p = px.get(y * w + x).copied().unwrap_or([0.0; 4]);
            for c in p.iter().take(ch) {
                let v = c.clamp(0.0, 1.0);
                if depth == 16 {
                    raw.extend_from_slice(&((v * 65535.0 + 0.5) as u16).to_be_bytes());
                } else {
                    raw.push((v * 255.0 + 0.5) as u8);
                }
            }
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(b) = blocks.next() {
        z.push(if blocks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[depth, if opaque { 2 } else { 6 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

// ======================= decoder =======================
use crate::tex::Tex;

struct BitR<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    cnt: u32,
}

impl<'a> BitR<'a> {
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        while self.cnt < n {
            let b = *self.d.get(self.pos).ok_or_else(|| "unexpected end of compressed data".to_string())?;
            self.bit |= (b as u32) << self.cnt;
            self.pos += 1;
            self.cnt += 8;
        }
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.cnt -= n;
        Ok(v)
    }
}

struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}

fn build(lengths: &[u8]) -> Huff {
    let mut count = [0u16; 16];
    for &l in lengths {
        count[l as usize & 15] += 1;
    }
    let mut offs = [0u16; 16];
    for i in 1..15 {
        offs[i + 1] = offs[i] + count[i];
    }
    let mut symbol = vec![0u16; lengths.len()];
    for (s, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbol[offs[l as usize & 15] as usize] = s as u16;
            offs[l as usize & 15] += 1;
        }
    }
    Huff { count, symbol }
}

fn decode_sym(br: &mut BitR, h: &Huff) -> Result<u16, String> {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for len in 1..16 {
        code |= br.bits(1)? as i32;
        let count = h.count[len] as i32;
        if code - count < first {
            return h.symbol.get((index + (code - first)) as usize).copied().ok_or_else(|| "bad huffman code".to_string());
        }
        index += count;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    Err("bad huffman code".into())
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn codes(br: &mut BitR, out: &mut Vec<u8>, lh: &Huff, dh: &Huff) -> Result<(), String> {
    loop {
        let sym = decode_sym(br, lh)? as usize;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let s = sym - 257;
            if s >= 29 {
                return Err("bad length symbol".into());
            }
            let len = LBASE[s] as usize + br.bits(LEXT[s] as u32)? as usize;
            let ds = decode_sym(br, dh)? as usize;
            if ds >= 30 {
                return Err("bad distance symbol".into());
            }
            let dist = DBASE[ds] as usize + br.bits(DEXT[ds] as u32)? as usize;
            if dist > out.len() {
                return Err("distance too far back".into());
            }
            for _ in 0..len {
                let b = out[out.len() - dist];
                out.push(b);
            }
        }
        if out.len() > (1 << 30) {
            return Err("decompressed data too large".into());
        }
    }
}

pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut br = BitR { d: data, pos: 0, bit: 0, cnt: 0 };
    let mut out = Vec::new();
    loop {
        let last = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                br.bit = 0;
                br.cnt = 0;
                let p = br.pos;
                if p + 4 > data.len() {
                    return Err("truncated stored block".into());
                }
                let len = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
                let nlen = u16::from_le_bytes([data[p + 2], data[p + 3]]) as usize;
                if len != (!nlen & 0xffff) || p + 4 + len > data.len() {
                    return Err("bad stored block".into());
                }
                out.extend_from_slice(&data[p + 4..p + 4 + len]);
                br.pos = p + 4 + len;
            }
            1 => {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() {
                    *v = if i < 144 { 8 } else if i < 256 { 9 } else if i < 280 { 7 } else { 8 };
                }
                codes(&mut br, &mut out, &build(&l), &build(&[5u8; 30]))?;
            }
            2 => {
                let nlen = br.bits(5)? as usize + 257;
                let ndist = br.bits(5)? as usize + 1;
                let ncode = br.bits(4)? as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return Err("bad dynamic header".into());
                }
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut lengths = [0u8; 320];
                for &o in ORDER.iter().take(ncode) {
                    lengths[o] = br.bits(3)? as u8;
                }
                let lc = build(&lengths[..19]);
                let mut idx = 0;
                let total = nlen + ndist;
                let mut ll = [0u8; 320];
                while idx < total {
                    let sym = decode_sym(&mut br, &lc)?;
                    if sym < 16 {
                        ll[idx] = sym as u8;
                        idx += 1;
                    } else {
                        let (prev, rep) = match sym {
                            16 => {
                                if idx == 0 {
                                    return Err("bad repeat".into());
                                }
                                (ll[idx - 1], 3 + br.bits(2)? as usize)
                            }
                            17 => (0, 3 + br.bits(3)? as usize),
                            _ => (0, 11 + br.bits(7)? as usize),
                        };
                        if idx + rep > total {
                            return Err("too many lengths".into());
                        }
                        for _ in 0..rep {
                            ll[idx] = prev;
                            idx += 1;
                        }
                    }
                }
                codes(&mut br, &mut out, &build(&ll[..nlen]), &build(&ll[nlen..total]))?;
            }
            _ => return Err("bad block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i32, b as i32, c as i32);
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decode a non-interlaced 8/16-bit PNG (gray, gray+alpha, RGB, RGBA, 8-bit palette).
pub fn decode(b: &[u8]) -> Result<Tex, String> {
    if b.len() < 8 || &b[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("not a PNG file".into());
    }
    let mut pos = 8;
    let (mut w, mut h, mut depth, mut ctype, mut inter) = (0usize, 0usize, 0u8, 0u8, 0u8);
    let mut plte: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    while pos + 8 <= b.len() {
        let len = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
        let kind = &b[pos + 4..pos + 8];
        let end = pos + 8 + len;
        if end + 4 > b.len() {
            return Err("truncated chunk".into());
        }
        let data = &b[pos + 8..end];
        match kind {
            b"IHDR" if data.len() >= 13 => {
                w = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
                h = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
                depth = data[8];
                ctype = data[9];
                inter = data[12];
            }
            b"PLTE" => plte = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos = end + 4;
    }
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err("bad image size".into());
    }
    if inter != 0 {
        return Err("interlaced PNG is not supported".into());
    }
    if depth != 8 && depth != 16 {
        return Err(format!("{depth}-bit PNG is not supported (use 8 or 16)"));
    }
    if ctype == 3 && depth != 8 {
        return Err("only 8-bit palette PNGs are supported".into());
    }
    let ch = match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("unknown PNG colour type".into()),
    };
    let bpp = ch * depth as usize / 8;
    let stride = w * bpp;
    if idat.len() < 6 {
        return Err("missing image data".into());
    }
    let raw = inflate(&idat[2..])?;
    if raw.len() < h * (stride + 1) {
        return Err("image data too short".into());
    }
    let mut img = vec![0u8; h * stride];
    for y in 0..h {
        let f = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for i in 0..stride {
            let a = if i >= bpp { img[y * stride + i - bpp] } else { 0 };
            let up = if y > 0 { img[(y - 1) * stride + i] } else { 0 };
            let c = if y > 0 && i >= bpp { img[(y - 1) * stride + i - bpp] } else { 0 };
            let x = src[i];
            img[y * stride + i] = match f {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(up),
                3 => x.wrapping_add(((a as u16 + up as u16) / 2) as u8),
                4 => x.wrapping_add(paeth(a, up, c)),
                _ => return Err("bad PNG filter".into()),
            };
        }
    }
    let rd = |y: usize, x: usize, c: usize| -> f32 {
        if depth == 16 {
            let o = y * stride + (x * ch + c) * 2;
            u16::from_be_bytes([img[o], img[o + 1]]) as f32 / 65535.0
        } else {
            img[y * stride + x * ch + c] as f32 / 255.0
        }
    };
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            px.push(match ctype {
                0 => {
                    let g = rd(y, x, 0);
                    [g, g, g, 1.0]
                }
                4 => {
                    let g = rd(y, x, 0);
                    [g, g, g, rd(y, x, 1)]
                }
                2 => [rd(y, x, 0), rd(y, x, 1), rd(y, x, 2), 1.0],
                6 => [rd(y, x, 0), rd(y, x, 1), rd(y, x, 2), rd(y, x, 3)],
                _ => {
                    let i = img[y * stride + x] as usize * 3;
                    if i + 2 >= plte.len() {
                        return Err("palette index out of range".into());
                    }
                    [plte[i] as f32 / 255.0, plte[i + 1] as f32 / 255.0, plte[i + 2] as f32 / 255.0, 1.0]
                }
            });
        }
    }
    Ok(Tex { w, h, px })
}
