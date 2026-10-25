pub mod chunk;
pub mod climate;
pub mod chunks;
pub mod physics;
pub mod biome;
pub mod gen;
pub mod pick;
pub mod sky;
pub mod atmos;
pub mod dig;
pub mod items;
pub mod mobs;
pub mod names;
pub mod craft;
pub mod chest;
pub mod gear;
pub mod vitals;
pub mod effects;
pub mod save;
pub mod ticks;
pub mod redstone;
pub mod decor;
pub mod path;
pub mod rail;
pub mod boat;
pub mod far;
pub mod lod;
pub mod minecart;
pub mod portal;
pub mod sprint;
pub mod stamina;
pub mod pack;

/// Lowers the calling thread's scheduling priority (`nice` 0..19; 19 = runs only on what the others leave). Linux and Android keep the
/// nice value per THREAD (`PRIO_PROCESS` with `who` 0 is the calling task, not the process), so each background thread calls this first
/// and the render thread, which never does, wins every contended core. Raising a nice value needs no privilege. A failure is ignored: the
/// thread just keeps its priority. ponytail: no `libc` dependency for one symbol that bionic and glibc both export.
pub fn nice(level: i32) {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        extern "C" {
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        }
        // SAFETY: plain syscall wrapper, no pointers.
        unsafe { setpriority(0, 0, level) };
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    let _ = level;
}
