# Opsi optimisasi build untuk recon_fn (LTO, PGO, -O3 pada hot path raster/2D).
# Semua opsi dilewati pada konfigurasi Debug. Hasil harus diukur di perangkat nyata.

option(SEEDLESS_ENABLE_THIN_LTO "Aktifkan ThinLTO (-flto=thin) untuk recon_fn" ON)
option(SEEDLESS_ENABLE_FULL_LTO "Aktifkan LTO penuh (-flto) untuk recon_fn; menggantikan ThinLTO. Ukur dulu di perangkat." OFF)
option(SEEDLESS_RASTER_O3 "Compile file raster 3D dan GPU 2D dengan -O3" ON)
set(SEEDLESS_PGO_MODE "OFF" CACHE STRING "Mode PGO: OFF, GENERATE, atau USE")
set_property(CACHE SEEDLESS_PGO_MODE PROPERTY STRINGS OFF GENERATE USE)
set(SEEDLESS_PGO_PROFILE "" CACHE FILEPATH "Path file .profdata (hasil llvm-profdata merge) untuk mode USE")

set(_seedless_not_debug "$<NOT:$<CONFIG:Debug>>")

# --- ThinLTO ---------------------------------------------------------------
# Full LTO (-flto, setara lto="fat") digabung dengan ThinLTO tidak bermakna: bila keduanya ON, full yang dipakai.
# Bandingkan dengan: -DSEEDLESS_ENABLE_FULL_LTO=ON (full) lawan default (thin), pada game dan perangkat yang sama,
# memakai overlay "Show frame timing" (EMU avg/p99) dan waktu build.
if(SEEDLESS_ENABLE_FULL_LTO)
    target_compile_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-flto=full>")
    target_link_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-flto=full>")
elseif(SEEDLESS_ENABLE_THIN_LTO)
    target_compile_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-flto=thin>")
    target_link_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-flto=thin>")
endif()

# --- PGO -------------------------------------------------------------------
# Alur:
#   1. Build dengan -DSEEDLESS_PGO_MODE=GENERATE, jalankan di perangkat dengan
#      beberapa game, lalu ambil file *.profraw (lihat LLVM_PROFILE_FILE).
#   2. llvm-profdata merge -output=recon.profdata *.profraw
#   3. Build ulang dengan -DSEEDLESS_PGO_MODE=USE -DSEEDLESS_PGO_PROFILE=/path/recon.profdata
if(SEEDLESS_PGO_MODE STREQUAL "GENERATE")
    target_compile_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-fprofile-generate>")
    target_link_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-fprofile-generate>")
elseif(SEEDLESS_PGO_MODE STREQUAL "USE")
    if(NOT SEEDLESS_PGO_PROFILE OR NOT EXISTS "${SEEDLESS_PGO_PROFILE}")
        message(FATAL_ERROR "SEEDLESS_PGO_MODE=USE membutuhkan SEEDLESS_PGO_PROFILE yang valid")
    endif()
    target_compile_options(recon_fn PRIVATE
        "$<${_seedless_not_debug}:-fprofile-use=${SEEDLESS_PGO_PROFILE}>"
        "$<${_seedless_not_debug}:-Wno-profile-instr-unprofiled>"
        "$<${_seedless_not_debug}:-Wno-profile-instr-out-of-date>")
    target_link_options(recon_fn PRIVATE "$<${_seedless_not_debug}:-fprofile-use=${SEEDLESS_PGO_PROFILE}>")
elseif(NOT SEEDLESS_PGO_MODE STREQUAL "OFF")
    message(FATAL_ERROR "SEEDLESS_PGO_MODE harus OFF, GENERATE, atau USE")
endif()

# --- -O3 untuk hot path raster/2D -------------------------------------------
if(SEEDLESS_RASTER_O3)
    set(_seedless_o3_dir ${CMAKE_CURRENT_LIST_DIR}/gpu)
    set(_seedless_o3_sources
        ${_seedless_o3_dir}/gpu3d/raster/bands.c
        ${_seedless_o3_dir}/gpu3d/raster/compose_band.c
        ${_seedless_o3_dir}/gpu3d/raster/depth_effects.c
        ${_seedless_o3_dir}/gpu3d/raster/interp.c
        ${_seedless_o3_dir}/gpu3d/raster/neon_helpers.c
        ${_seedless_o3_dir}/gpu3d/raster/span.c
        ${_seedless_o3_dir}/gpu3d/raster/texture.c
        ${_seedless_o3_dir}/gpu2d/bg_affine.c
        ${_seedless_o3_dir}/gpu2d/bg_text.c
        ${_seedless_o3_dir}/gpu2d/blend.c
        ${_seedless_o3_dir}/gpu2d/compose.c
        ${_seedless_o3_dir}/gpu2d/line.c
        ${_seedless_o3_dir}/gpu2d/obj.c
        ${_seedless_o3_dir}/gpu2d/planes.c
    )
    set_source_files_properties(${_seedless_o3_sources} PROPERTIES
        COMPILE_OPTIONS "$<${_seedless_not_debug}:-O3>")
endif()
