#ifndef BANDEAR_RING_H
#define BANDEAR_RING_H

#include <stdbool.h>
#include <stdint.h>

#pragma clang assume_nonnull begin

/// A single-producer, single-consumer ring of interleaved stereo PCM16 frames: the receive loop
/// writes into it, the render callback reads from it, and neither ever waits for the other.
///
/// Written in C because iOS 16 has no `Synchronization.Atomic`, and C11 atomics need no package to
/// fetch. The positions are monotonic 64-bit frame counters, so the frames queued are their
/// difference and a slot is a position modulo the capacity. Each side stores only its own
/// position, with release ordering, and loads the other's with acquire ordering: a frame is
/// copied in before the write position says it is there, and read out before the read position
/// gives its slot back.
///
/// The storage is allocated once, by `bandear_ring_create`; nothing after that allocates, locks or
/// blocks.
///
/// Each function names the side that may call it. "Either side" means any thread.
typedef struct BandEarRing BandEarRing;

/// A ring holding at most `capacity_frames` frames, or NULL when that is not a positive count or
/// the storage cannot be allocated. Starts with a target of one frame, nothing written and nothing
/// counted.
BandEarRing *_Nullable bandear_ring_create(int64_t capacity_frames);

/// Releases the ring and its storage. Neither side may touch it again.
void bandear_ring_destroy(BandEarRing *ring);

/// The most frames the ring holds at once. Either side.
int64_t bandear_ring_capacity(BandEarRing *ring);

/// Producer: copies up to `frames` frames from `samples` (two samples each, left first) into the
/// free space and returns how many it took. Takes nothing once the ring is closed.
int64_t bandear_ring_write(BandEarRing *ring, const int16_t *samples, int64_t frames);

/// Consumer: the frames written and not consumed yet.
int64_t bandear_ring_queued(BandEarRing *ring);

/// Consumer: points `*samples` at the oldest queued frame and returns how many queued frames follow
/// it without wrapping, at most `frames`. The frames stay queued until `bandear_ring_consume`.
int64_t bandear_ring_peek(
    BandEarRing *ring, int64_t frames, const int16_t *_Nullable *_Nonnull samples);

/// Consumer: gives back the slots of the oldest `frames` queued frames, at most all of them.
void bandear_ring_consume(BandEarRing *ring, int64_t frames);

/// Frames consumed since the ring was created: the read position. Either side.
int64_t bandear_ring_frames_consumed(BandEarRing *ring);

/// Producer: the frames to have queued before playing. Stored as given.
void bandear_ring_set_target(BandEarRing *ring, int64_t frames);

/// Consumer: the frames to have queued before playing.
int64_t bandear_ring_target(BandEarRing *ring);

/// Consumer: counts one time the output ran dry.
void bandear_ring_count_underrun(BandEarRing *ring);

/// The times the output ran dry since the ring was created. Either side.
int64_t bandear_ring_underruns(BandEarRing *ring);

/// The frames between the render callback and the listener, as last set. Either side.
int64_t bandear_ring_output_latency(BandEarRing *ring);

/// Sets the frames between the render callback and the listener. Either side.
void bandear_ring_set_output_latency(BandEarRing *ring, int64_t frames);

/// Producer: closes the ring. Nothing is written after this; the storage stays until
/// `bandear_ring_destroy`, so a render callback already running can finish.
void bandear_ring_close(BandEarRing *ring);

/// Whether the ring was closed. Either side.
bool bandear_ring_is_closed(BandEarRing *ring);

#pragma clang assume_nonnull end

#endif
