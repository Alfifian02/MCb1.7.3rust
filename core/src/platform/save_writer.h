#ifndef SEEDLESSDS_PLATFORM_SAVE_WRITER_H
#define SEEDLESSDS_PLATFORM_SAVE_WRITER_H

/*
 * Background writer for save files (SRAM, EEPROM, flash, firmware, slot 2).
 *
 * The emulation thread used to seek, write and fflush these files itself, on
 * the frame that closes the 60-frame autosave countdown. On slow storage or a
 * SAF-backed path that is a visible hitch. Now the emulation thread only copies
 * the bytes that changed into a job (a few hundred KB at most, a memcpy) and
 * hands it to one I/O thread; the file work happens there.
 *
 * Rules that keep the files correct:
 *  - Jobs run strictly in submission order on one thread.
 *  - A job owns a copy of its data, so the emulator can keep changing or free
 *    its own buffers straight away.
 *  - Anything that closes, reopens or synchronously rewrites a save file calls
 *    save_writer_flush() first.
 *  - If the I/O thread cannot be started the job runs inline, which is exactly
 *    the old behaviour.
 */

#include <stddef.h>
#include <stdint.h>

typedef struct save_job save_job_t;

/* `file` is a stdio stream owned by the caller. `reserve` is a size hint for the copied data. */
save_job_t *save_job_create(void *file, size_t reserve);
/* Copies `length` bytes that belong at `offset` in the file. Returns 0 on success, -1 when out of memory. */
int save_job_add(save_job_t *job, uint64_t offset, const void *data, size_t length);
void save_job_set_truncate(save_job_t *job, long size);
/* Whole-file mode: the job reopens `path` with "wb" and writes the added data sequentially. */
int save_job_set_path(save_job_t *job, const char *path);

/* Hands the job to the I/O thread, which takes ownership. */
void save_job_submit(save_job_t *job);
/* Runs the job on the calling thread, frees it and returns the stdio status of the last flush or close. */
int save_job_run(save_job_t *job);
void save_job_discard(save_job_t *job);

/* Blocks until every submitted job has finished. */
void save_writer_flush(void);
uint32_t save_writer_pending(void);

#endif
