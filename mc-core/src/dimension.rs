//! Perbedaan WorldProvider yang memengaruhi logika: ada/tidaknya langit, sudut langit, tabel kecerahan.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Surface,
    Hell,
}

impl Dimension {
    pub fn has_no_sky(self) -> bool {
        matches!(self, Dimension::Hell)
    }

    /// Java: WorldProvider.calculateCelestialAngle(time, partial)
    pub fn celestial_angle(self, time: i64, partial: f32) -> f32 {
        match self {
            Dimension::Hell => 0.5,
            Dimension::Surface => {
                let t = (time % 24000) as i32;
                let mut a = (t as f32 + partial) / 24000.0f32 - 0.25f32;
                if a < 0.0 {
                    a += 1.0;
                }
                if a > 1.0 {
                    a -= 1.0;
                }
                let base = a;
                let curved = 1.0f32 - (((a as f64 * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
                base + (curved - base) / 3.0f32
            }
        }
    }

    /// Java: generateLightBrightnessTable (Nether: batas bawah 0.1, permukaan: 0.05)
    pub fn light_brightness_table(self) -> [f32; 16] {
        let floor = match self {
            Dimension::Surface => 0.05f32,
            Dimension::Hell => 0.1f32,
        };
        let mut t = [0f32; 16];
        for (i, v) in t.iter_mut().enumerate() {
            let inv = 1.0f32 - i as f32 / 15.0f32;
            *v = (1.0f32 - inv) / (inv * 3.0f32 + 1.0f32) * (1.0f32 - floor) + floor;
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabel_kecerahan_ujung() {
        let t = Dimension::Surface.light_brightness_table();
        assert!((t[0] - 0.05).abs() < 1e-6);
        assert!((t[15] - 1.0).abs() < 1e-6);
        assert!(Dimension::Hell.light_brightness_table()[0] > t[0]);
    }

    #[test]
    fn sudut_langit() {
        // Siang tepat (waktu 6000 = tengah hari) mendekati 0.0 setelah kurva; Nether selalu 0.5
        let a = Dimension::Surface.celestial_angle(0, 0.0);
        assert!(a >= 0.0 && a <= 1.0);
        assert_eq!(Dimension::Hell.celestial_angle(12345, 0.3), 0.5);
        assert_eq!(Dimension::Surface.celestial_angle(24000, 0.0), Dimension::Surface.celestial_angle(0, 0.0));
    }
}
