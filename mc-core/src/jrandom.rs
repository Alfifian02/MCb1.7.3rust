//! Port bit-exact dari java.util.Random (LCG 48-bit).
//! Wajib identik agar seed dunia menghasilkan terrain yang sama dengan b1.7.3.

const MULT: i64 = 0x5DEECE66D;
const ADD: i64 = 0xB;
const MASK: i64 = (1i64 << 48) - 1;

#[derive(Clone)]
pub struct JRandom {
    seed: i64,
    next_next_gaussian: f64,
    have_next_next_gaussian: bool,
}

impl JRandom {
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed ^ MULT) & MASK,
            next_next_gaussian: 0.0,
            have_next_next_gaussian: false,
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULT) & MASK;
        self.have_next_next_gaussian = false;
    }

    #[inline]
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULT).wrapping_add(ADD) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// Setara nextInt(bound); bound harus > 0.
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if bound & (-bound) == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            // overflow i32 disengaja, sama seperti Java
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        ((self.next(32) as i64) << 32).wrapping_add(self.next(32) as i64)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = (self.next(26) as i64) << 27;
        let lo = self.next(27) as i64;
        (hi + lo) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    pub fn next_gaussian(&mut self) -> f64 {
        if self.have_next_next_gaussian {
            self.have_next_next_gaussian = false;
            return self.next_next_gaussian;
        }
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                // Java memakai StrictMath; f64::ln/sqrt cukup dekat, verifikasi di tes golden
                let m = (-2.0 * s.ln() / s).sqrt();
                self.next_next_gaussian = v2 * m;
                self.have_next_next_gaussian = true;
                return v1 * m;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_java_seed_42() {
        // Java: new Random(42).nextInt() == -1170105035
        let mut r = JRandom::new(42);
        assert_eq!(r.next_int(), -1170105035);
    }
}
