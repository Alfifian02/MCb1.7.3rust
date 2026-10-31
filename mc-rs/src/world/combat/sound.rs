//! C7: which recording each moment of a weapon plays, and at what pitch (`ModSounds.of` / `ModSounds.pitch`). **NOT b1.7.3.**
//! **UNVERIFIED: written without compiling or running anything.**
//!
//! Pure, like the mod's own table: names and numbers only, so a host test can open it. The recordings are the mod's own
//! (`assets/sound/extra/combat/`, key `combat.<name>`; the pools of 2 or 3 files are one key, as in `sounds.json`).
//!
//! Deviations, all one line each to change:
//! * The mod borrows vanilla 1.21's `entity.player.attack.weak` (a hit or a miss with no art of its own: the pickaxe, the bare hand) and
//!   `attack.strong` (a charged hit with none). Neither exists in b1.7.3's assets, so a plain hit and miss of those are **silent**
//!   (b1.7.3's punch was silent too: the mob's own cry plays) and their charged hit borrows `sword_charge_hit` at the mod's 0.8.
//! * Not ported: the pack override (`WeaponPackSounds`, YAML packs are dropped), the mace's anvil and core layers and the shovel's
//!   lantern layer (vanilla recordings, and no mace or shovel special here), the streak's note-block ladder (`STACK_STEP`: b1.7.3 has
//!   `note.pling`, but the mod sends it to the one player and a `Snd` here has no private channel; the streak's orbs on the HUD say it),
//!   the arcabuz's sounds (C8), the sweeping edge and the orb (enchantment and shovel special, dropped).

use super::profile::Profile;

/// `ModSounds.Kind`, the moments a Cortes weapon makes a sound for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The swing connected.
    Hit,
    /// The swing connected after a charge.
    ChargedHit,
    /// The swing found nothing.
    Miss,
    /// The hold passed the minimum (`MIN_CHARGE_HOLD`): once per charge.
    Charge,
    /// The guard broke.
    GuardBroken,
    /// The sweep (`MultiHit`) went off.
    Special,
    /// The lunge started.
    Lunge,
    /// The shotel's parry turned a hit back.
    Parry,
}

pub const ALL: [Kind; 8] = [Kind::Hit, Kind::ChargedHit, Kind::Miss, Kind::Charge, Kind::GuardBroken, Kind::Special, Kind::Lunge, Kind::Parry];

/// A recording by its `SoundPool` key, and the pitch it plays at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cue {
    pub name: &'static str,
    pub pitch: f32,
}

/// `SwingInputHandler`: the charge cue plays at 0.7 volume (the rest at 1.0).
pub const CHARGE_VOLUME: f32 = 0.7;
/// `ModSounds`: the shotel is sharper on the sword's three recordings, the mace lower on the hammer's, the lunge far outside the range,
/// the hoe's sweep and miss and the shovel's miss at their own pitch, a borrowed charged hit a fifth down (`VANILLA_CHARGED_PITCH`).
const SABRE: f32 = 1.2;
const MACE: f32 = 0.5;
const LUNGE: f32 = 2.0;
const HOE_SWEEP: f32 = 0.7;
const HOE_MISS: f32 = 1.2;
const SHOVEL_MISS: f32 = 0.8;
const BORROWED_CHARGED: f32 = 0.8;
/// The engine's own clamp on a played pitch (`SoundEngine`: [0.5, 2.0]); `Audio` has none, so `of` applies it.
pub const MIN_PITCH: f32 = 0.5;
pub const MAX_PITCH: f32 = 2.0;

/// `ModSounds.HIT`: the weapons with recordings of their own (the sabre borrows the sword's, the mace the hammer's).
fn hit(p: Profile) -> Option<&'static str> {
    Some(match p {
        Profile::Sword | Profile::Sabre => "combat.sword_hit",
        Profile::Axe => "combat.axe_hit",
        Profile::Dagger => "combat.daga_hit",
        Profile::Ropera => "combat.ropera_hit",
        Profile::Halberd => "combat.alabarda_hit",
        Profile::Warhammer | Profile::Mace => "combat.martillo_hit",
        _ => return None,
    })
}

/// `ModSounds.CHARGED_HIT`.
fn charged(p: Profile) -> Option<&'static str> {
    Some(match p {
        Profile::Sword | Profile::Sabre => "combat.sword_charge_hit",
        Profile::Axe => "combat.axe_charge_hit",
        Profile::Halberd => "combat.alabarda_charged_hit",
        Profile::Warhammer | Profile::Mace => "combat.martillo_charged_hit",
        _ => return None,
    })
}

/// `ModSounds.MISS`; the shovel and the hoe borrow the sword's.
fn miss(p: Profile) -> Option<&'static str> {
    Some(match p {
        Profile::Sword | Profile::Sabre | Profile::Shovel | Profile::Hoe => "combat.sword_miss",
        Profile::Axe => "combat.axe_miss",
        Profile::Dagger => "combat.daga_miss",
        Profile::Ropera => "combat.ropera_miss",
        Profile::Halberd => "combat.alabarda_miss",
        Profile::Warhammer | Profile::Mace => "combat.martillo_miss",
        _ => return None,
    })
}

/// The cue of a moment for a profile, or `None` when it is silent. The pitch is already inside the engine's clamp.
pub fn of(p: Profile, kind: Kind) -> Option<Cue> {
    let (name, pitch) = match kind {
        Kind::Hit => (hit(p)?, scaled(p, 1.0)),
        Kind::Miss => (miss(p)?, match p {
            Profile::Shovel => SHOVEL_MISS,
            Profile::Hoe => HOE_MISS,
            _ => scaled(p, 1.0),
        }),
        Kind::ChargedHit => match charged(p) {
            Some(n) => (n, scaled(p, 1.0)),
            None => ("combat.sword_charge_hit", BORROWED_CHARGED), // vanilla's `attack.strong`, which b1.7.3 lacks
        },
        Kind::Charge => ("combat.charge_generic", 1.0),
        Kind::GuardBroken => ("combat.sword_guardia_rota", 1.0),
        Kind::Special => ("combat.alabarda_wide_swing", if p == Profile::Hoe { HOE_SWEEP } else { 1.0 }),
        Kind::Lunge => ("combat.alabarda_wide_swing", LUNGE),
        Kind::Parry => ("combat.shotel_parry", 1.0),
    };
    Some(Cue { name, pitch: pitch.clamp(MIN_PITCH, MAX_PITCH) })
}

/// The pitch a borrowing weapon is played at on the recordings it borrows (hit, miss and charged hit).
fn scaled(p: Profile, base: f32) -> f32 {
    match p {
        Profile::Sabre => base * SABRE,
        Profile::Mace => base * MACE,
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::combat::profile::ALL as PROFILES;

    // The mod's `ModSoundsTest` / `SoundPoolTest`: every cue names a recording that exists, and the pitches are the table's.
    #[test]
    fn every_cue_names_a_real_recording_at_a_legal_pitch() {
        for p in PROFILES {
            for k in ALL {
                if let Some(c) = of(p, k) {
                    assert!(crate::audio::exists(c.name), "{p:?} {k:?}: no such key {}", c.name);
                    assert!((MIN_PITCH..=MAX_PITCH).contains(&c.pitch), "{p:?} {k:?}");
                }
            }
        }
        assert_eq!(of(Profile::Sabre, Kind::Hit).map(|c| c.pitch), Some(1.2)); // the shotel's sharper sword
        assert_eq!(of(Profile::Mace, Kind::Miss).map(|c| c.pitch), Some(0.5)); // the mace's lower hammer
        assert_eq!(of(Profile::Ropera, Kind::Lunge).map(|c| c.pitch), Some(2.0));
        assert_eq!(of(Profile::Hoe, Kind::Special).map(|c| c.pitch), Some(0.7));
        assert_eq!(of(Profile::Shovel, Kind::Miss).map(|c| c.pitch), Some(0.8));
        assert_eq!(of(Profile::Pickaxe, Kind::Hit), None); // vanilla's weak hit: silent here
        assert_eq!(of(Profile::Pickaxe, Kind::ChargedHit).map(|c| c.pitch), Some(0.8)); // borrowed, a fifth down
    }
}
