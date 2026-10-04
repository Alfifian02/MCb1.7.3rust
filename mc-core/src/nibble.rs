//! Array 4-bit (metadata, skylight, blocklight), padat: 2 nilai per byte.
//! Indeks chunk 16x16x128: (x << 11) | (z << 7) | y  (sama dengan b1.7.3).

#[derive(Clone)]
pub struct NibbleArray {
    pub data: Vec<u8>,
}

impl NibbleArray {
    pub fn new(len: usize) -> Self {
        Self { data: vec![0; len >> 1] }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
    }

    #[inline]
    fn index(x: usize, y: usize, z: usize) -> usize {
        (x << 11) | (z << 7) | y
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        let i = Self::index(x, y, z);
        let b = self.data[i >> 1];
        if i & 1 == 0 { b & 0x0F } else { (b >> 4) & 0x0F }
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, v: u8) {
        let i = Self::index(x, y, z);
        let b = &mut self.data[i >> 1];
        if i & 1 == 0 {
            *b = (*b & 0xF0) | (v & 0x0F);
        } else {
            *b = (*b & 0x0F) | ((v & 0x0F) << 4);
        }
    }
}
