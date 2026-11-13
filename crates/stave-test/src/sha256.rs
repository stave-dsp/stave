//! SHA-256 (FIPS 180-4), for naming program material, takes and the
//! reference corpus (DSP.md R22) by content. Small enough to own.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// A streaming SHA-256.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    pending: Vec<u8>,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            pending: Vec::with_capacity(64),
            length: 0,
        }
    }
}

impl Sha256 {
    pub fn update(&mut self, mut bytes: &[u8]) {
        self.length += bytes.len() as u64;
        if !self.pending.is_empty() {
            let take = (64 - self.pending.len()).min(bytes.len());
            self.pending.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.pending.len() == 64 {
                let block: [u8; 64] = self.pending[..].try_into().expect("64 bytes");
                self.compress(&block);
                self.pending.clear();
            }
        }
        let mut chunks = bytes.chunks_exact(64);
        for c in &mut chunks {
            self.compress(c.try_into().expect("64 bytes"));
        }
        self.pending.extend_from_slice(chunks.remainder());
    }

    /// The digest as lower-case hex.
    pub fn hex(mut self) -> String {
        let bits = self.length.wrapping_mul(8);
        let mut tail = std::mem::take(&mut self.pending);
        tail.push(0x80);
        while tail.len() % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bits.to_be_bytes());
        for c in tail.chunks_exact(64) {
            self.compress(c.try_into().expect("64 bytes"));
        }
        self.state.iter().map(|w| format!("{w:08x}")).collect()
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for (k, wi) in K.iter().zip(w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// The digest of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    let mut h = Sha256::default();
    h.update(bytes);
    h.hex()
}

/// The digest of planar audio, frame-interleaved little-endian `f32`
/// (so the same samples always name the same material).
pub fn of_audio(channels: &[Vec<f32>]) -> String {
    let mut h = Sha256::default();
    let frames = channels.iter().map(Vec::len).min().unwrap_or(0);
    let mut buf = Vec::with_capacity(4096);
    for f in 0..frames {
        for c in channels {
            buf.extend_from_slice(&c[f].to_le_bytes());
        }
        if buf.len() >= 4096 {
            h.update(&buf);
            buf.clear();
        }
    }
    h.update(&buf);
    h.hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fips_vectors() {
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut h = Sha256::default();
        for c in data.chunks(37) {
            h.update(c);
        }
        assert_eq!(h.hex(), hex(&data));
    }
}
