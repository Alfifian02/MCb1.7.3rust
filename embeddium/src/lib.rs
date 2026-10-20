//! Rust port of the core of Embeddium (a Sodium 0.5.8 fork, LGPL-3.0): the part of its terrain renderer that is not
//! OpenGL, Minecraft or Forge glue. std only, no `unsafe`, no threads: the caller decides where it runs.
//!
//! - [`vis`]: section connectivity (`VisibilityEncoding` + a flood fill that builds it).
//! - [`cull`]: `OcclusionCuller`, the breadth-first walk over sections that hides whatever sits behind solid terrain.
//! - [`arena`]: `GlBufferArena`'s allocator (best fit, coalescing, compacting growth) as plain bookkeeping.
//! - [`vertex`]: `CompactChunkVertex` cut to 16 bytes, the terrain vertex.
//! - [`quad_index`]: `SharedQuadIndexBuffer`, the one index buffer every quad mesh draws with.
//!
//! Left out on purpose: `RenderSection` / region bookkeeping (the host owns its chunks), the GL layer (the host's wgpu
//! replaces it: no multi-draw, no persistent mapping, nothing a GLES 3.0 or Vulkan phone might lack), the options screen,
//! Forge lighting and the mod-compat events.

pub mod arena;
pub mod cull;
pub mod quad_index;
pub mod vertex;
pub mod vis;
