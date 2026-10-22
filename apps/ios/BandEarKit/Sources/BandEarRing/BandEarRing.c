#include "BandEarRing.h"

#include <stdatomic.h>
#include <stdlib.h>

/// Left and right.
#define SAMPLES_PER_FRAME 2

struct BandEarRing {
    int64_t capacity_frames;
    /// `capacity_frames` interleaved frames, allocated with the ring.
    int16_t *samples;
    /// Frames written since the ring was created. Only the producer stores it.
    _Atomic int64_t write_position;
    /// Frames consumed since the ring was created. Only the consumer stores it.
    _Atomic int64_t read_position;
    _Atomic int64_t target_frames;
    _Atomic int64_t underruns;
    _Atomic int64_t output_latency_frames;
    _Atomic bool closed;
};

static int64_t smaller(int64_t a, int64_t b) { return a < b ? a : b; }

BandEarRing *bandear_ring_create(int64_t capacity_frames) {
    if (capacity_frames <= 0 || (uint64_t)capacity_frames > SIZE_MAX / SAMPLES_PER_FRAME) {
        return NULL;
    }
    BandEarRing *ring = malloc(sizeof(BandEarRing));
    if (ring == NULL) return NULL;
    ring->samples = calloc((size_t)capacity_frames * SAMPLES_PER_FRAME, sizeof(int16_t));
    if (ring->samples == NULL) {
        free(ring);
        return NULL;
    }
    ring->capacity_frames = capacity_frames;
    atomic_init(&ring->write_position, 0);
    atomic_init(&ring->read_position, 0);
    atomic_init(&ring->target_frames, 1);
    atomic_init(&ring->underruns, 0);
    atomic_init(&ring->output_latency_frames, 0);
    atomic_init(&ring->closed, false);
    return ring;
}

void bandear_ring_destroy(BandEarRing *ring) {
    free(ring->samples);
    free(ring);
}

int64_t bandear_ring_capacity(BandEarRing *ring) { return ring->capacity_frames; }

int64_t bandear_ring_write(BandEarRing *ring, const int16_t *samples, int64_t frames) {
    if (frames <= 0 || atomic_load_explicit(&ring->closed, memory_order_acquire)) return 0;
    int64_t written = atomic_load_explicit(&ring->write_position, memory_order_relaxed);
    // Acquire: the consumer has read a slot out before the read position gives it back.
    int64_t consumed = atomic_load_explicit(&ring->read_position, memory_order_acquire);
    int64_t count = smaller(frames, ring->capacity_frames - (written - consumed));
    if (count <= 0) return 0;

    int64_t start = written % ring->capacity_frames;
    int64_t before_end = smaller(count, ring->capacity_frames - start);
    // Sample by sample rather than with `memcpy`, so Thread Sanitizer checks each store against
    // the render side's reads: that is how the tests catch a missing release below. It reported no
    // race on a `memcpy`'d range here, even with every ordering relaxed.
    for (int64_t i = 0; i < before_end * SAMPLES_PER_FRAME; i++) {
        ring->samples[start * SAMPLES_PER_FRAME + i] = samples[i];
    }
    for (int64_t i = 0; i < (count - before_end) * SAMPLES_PER_FRAME; i++) {
        ring->samples[i] = samples[before_end * SAMPLES_PER_FRAME + i];
    }

    // Release: the frames are in their slots before the consumer can see them counted.
    atomic_store_explicit(&ring->write_position, written + count, memory_order_release);
    return count;
}

int64_t bandear_ring_queued(BandEarRing *ring) {
    int64_t consumed = atomic_load_explicit(&ring->read_position, memory_order_relaxed);
    return atomic_load_explicit(&ring->write_position, memory_order_acquire) - consumed;
}

int64_t bandear_ring_peek(BandEarRing *ring, int64_t frames, const int16_t **samples) {
    int64_t consumed = atomic_load_explicit(&ring->read_position, memory_order_relaxed);
    // Acquire: pairs with the producer's release, so the frames counted are in their slots.
    int64_t queued = atomic_load_explicit(&ring->write_position, memory_order_acquire) - consumed;
    int64_t start = consumed % ring->capacity_frames;
    *samples = ring->samples + start * SAMPLES_PER_FRAME;
    int64_t run = smaller(smaller(queued, ring->capacity_frames - start), frames);
    return run > 0 ? run : 0;
}

void bandear_ring_consume(BandEarRing *ring, int64_t frames) {
    int64_t consumed = atomic_load_explicit(&ring->read_position, memory_order_relaxed);
    int64_t queued = atomic_load_explicit(&ring->write_position, memory_order_acquire) - consumed;
    int64_t count = smaller(frames, queued);
    if (count <= 0) return;
    // Release: the frames are read out before the producer may write over their slots.
    atomic_store_explicit(&ring->read_position, consumed + count, memory_order_release);
}

int64_t bandear_ring_frames_consumed(BandEarRing *ring) {
    return atomic_load_explicit(&ring->read_position, memory_order_acquire);
}

void bandear_ring_set_target(BandEarRing *ring, int64_t frames) {
    atomic_store_explicit(&ring->target_frames, frames, memory_order_release);
}

int64_t bandear_ring_target(BandEarRing *ring) {
    return atomic_load_explicit(&ring->target_frames, memory_order_acquire);
}

void bandear_ring_count_underrun(BandEarRing *ring) {
    atomic_fetch_add_explicit(&ring->underruns, 1, memory_order_release);
}

int64_t bandear_ring_underruns(BandEarRing *ring) {
    return atomic_load_explicit(&ring->underruns, memory_order_acquire);
}

int64_t bandear_ring_output_latency(BandEarRing *ring) {
    return atomic_load_explicit(&ring->output_latency_frames, memory_order_acquire);
}

void bandear_ring_set_output_latency(BandEarRing *ring, int64_t frames) {
    atomic_store_explicit(&ring->output_latency_frames, frames, memory_order_release);
}

void bandear_ring_close(BandEarRing *ring) {
    atomic_store_explicit(&ring->closed, true, memory_order_release);
}

bool bandear_ring_is_closed(BandEarRing *ring) {
    return atomic_load_explicit(&ring->closed, memory_order_acquire);
}
