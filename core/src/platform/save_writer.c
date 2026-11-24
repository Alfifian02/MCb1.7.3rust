#include "platform/save_writer.h"

#include "seedlessds/platform.h"

#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <unistd.h>

#define SAVE_NICE 10

typedef struct record_header {
    uint64_t offset;
    uint32_t length;
    uint32_t reserved;
} record_header_t;

struct save_job {
    struct save_job *next;
    void *file;
    long truncate_size;
    char *path;
    uint8_t *blob;
    size_t used;
    size_t capacity;
};

static pthread_mutex_t writer_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t writer_work = PTHREAD_COND_INITIALIZER;
static pthread_cond_t writer_idle = PTHREAD_COND_INITIALIZER;
static save_job_t *queue_head;
static save_job_t *queue_tail;
static uint32_t queue_length;
static int writer_busy;
static int writer_state; /* 0 not started, 1 running, -1 could not start */
static pthread_t writer_thread;

save_job_t *save_job_create(void *file, size_t reserve)
{
    save_job_t *job = calloc(1, sizeof *job);

    if (job == NULL)
        return NULL;
    job->file = file;
    if (reserve != 0) {
        job->blob = malloc(reserve);
        if (job->blob == NULL) {
            free(job);
            return NULL;
        }
        job->capacity = reserve;
    }
    return job;
}

void save_job_discard(save_job_t *job)
{
    if (job == NULL)
        return;
    free(job->blob);
    free(job->path);
    free(job);
}

static int job_reserve(save_job_t *job, size_t more)
{
    size_t need = job->used + more;

    if (need <= job->capacity)
        return 0;
    {
        size_t cap = job->capacity != 0 ? job->capacity : 4096u;
        uint8_t *grown;

        while (cap < need)
            cap *= 2u;
        grown = realloc(job->blob, cap);
        if (grown == NULL)
            return -1;
        job->blob = grown;
        job->capacity = cap;
    }
    return 0;
}

int save_job_add(save_job_t *job, uint64_t offset, const void *data, size_t length)
{
    record_header_t header;

    if (job == NULL || length > 0xffffffffu)
        return -1;
    if (job_reserve(job, sizeof header + length) != 0)
        return -1;
    memset(&header, 0, sizeof header);
    header.offset = offset;
    header.length = (uint32_t)length;
    memcpy(job->blob + job->used, &header, sizeof header);
    memcpy(job->blob + job->used + sizeof header, data, length);
    job->used += sizeof header + length;
    return 0;
}

void save_job_set_truncate(save_job_t *job, long size)
{
    if (job != NULL)
        job->truncate_size = size;
}

int save_job_set_path(save_job_t *job, const char *path)
{
    char *copy;

    if (job == NULL || path == NULL)
        return -1;
    copy = strdup(path);
    if (copy == NULL)
        return -1;
    free(job->path);
    job->path = copy;
    return 0;
}

int save_job_run(save_job_t *job)
{
    int status = 0;
    size_t at = 0;

    if (job == NULL)
        return 0;

    if (job->path != NULL) {
        FILE *f = platform_file_open(job->path, "wb");

        if (f == NULL) {
            save_job_discard(job);
            return 0;
        }
        while (at + sizeof(record_header_t) <= job->used) {
            record_header_t header;

            memcpy(&header, job->blob + at, sizeof header);
            at += sizeof header;
            fwrite(job->blob + at, header.length, 1, f);
            at += header.length;
        }
        status = fclose(f);
        save_job_discard(job);
        return status;
    }

    if (job->file == NULL) {
        save_job_discard(job);
        return 0;
    }
    {
        FILE *f = job->file;

        if (job->truncate_size != 0)
            (void)ftruncate(fileno(f), job->truncate_size);
        while (at + sizeof(record_header_t) <= job->used) {
            record_header_t header;

            memcpy(&header, job->blob + at, sizeof header);
            at += sizeof header;
            fseek(f, (long)header.offset, SEEK_SET);
            fwrite(job->blob + at, header.length, 1, f);
            at += header.length;
        }
        status = fflush(f);
    }
    save_job_discard(job);
    return status;
}

static void *writer_main(void *unused)
{
    (void)unused;
    (void)setpriority(PRIO_PROCESS, 0, SAVE_NICE);
    (void)prctl(PR_SET_NAME, "sds-save-io", 0, 0, 0);

    pthread_mutex_lock(&writer_lock);
    for (;;) {
        save_job_t *job;

        while (queue_head == NULL)
            pthread_cond_wait(&writer_work, &writer_lock);
        job = queue_head;
        queue_head = job->next;
        if (queue_head == NULL)
            queue_tail = NULL;
        queue_length--;
        writer_busy = 1;
        pthread_mutex_unlock(&writer_lock);

        job->next = NULL;
        (void)save_job_run(job);

        pthread_mutex_lock(&writer_lock);
        writer_busy = 0;
        if (queue_head == NULL)
            pthread_cond_broadcast(&writer_idle);
    }
    return NULL;
}

void save_job_submit(save_job_t *job)
{
    if (job == NULL)
        return;

    pthread_mutex_lock(&writer_lock);
    if (writer_state == 0)
        writer_state = platform_thread_create(&writer_thread, NULL, writer_main, NULL) == 0 ? 1 : -1;
    if (writer_state == 1) {
        job->next = NULL;
        if (queue_tail != NULL)
            queue_tail->next = job;
        else
            queue_head = job;
        queue_tail = job;
        queue_length++;
        pthread_cond_signal(&writer_work);
        pthread_mutex_unlock(&writer_lock);
        return;
    }
    pthread_mutex_unlock(&writer_lock);

    /* No I/O thread: nothing has ever been queued, so running inline keeps the order. */
    (void)save_job_run(job);
}

void save_writer_flush(void)
{
    pthread_mutex_lock(&writer_lock);
    while (queue_head != NULL || writer_busy)
        pthread_cond_wait(&writer_idle, &writer_lock);
    pthread_mutex_unlock(&writer_lock);
}

uint32_t save_writer_pending(void)
{
    uint32_t n;

    pthread_mutex_lock(&writer_lock);
    n = queue_length + (uint32_t)(writer_busy != 0);
    pthread_mutex_unlock(&writer_lock);
    return n;
}
