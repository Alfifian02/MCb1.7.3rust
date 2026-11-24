#ifndef SEEDLESSDS_FRONTEND_AAUDIO_OUT_H
#define SEEDLESSDS_FRONTEND_AAUDIO_OUT_H

#include <stdint.h>

/* Optional AAudio playback next to OpenSL ES. See aaudio_out.c. All of these are no-ops unless the
 * app asked for AAudio (recon_audio_backend_set(1)) and the stream opened. */
int aaudio_out_open(void);                       /* after OpenSL is set up; 1 = AAudio now owns playback */
int aaudio_out_is_active(void);
int aaudio_out_write(const void *pcm, uint32_t samples);   /* int16 samples, emulation thread */
void aaudio_out_pause(void);                     /* flush and stop consuming */
void aaudio_out_resume(void);                    /* after audio_out_restart_playback() */
void aaudio_out_close(void);
void aaudio_out_set_volume_mb(int16_t millibel);

#endif
