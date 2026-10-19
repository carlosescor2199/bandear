//! Packet redundancy: the desk can have a musician's audio sent twice, so a packet Wi-Fi loses can
//! still arrive as its copy.
//!
//! The copy is the exact same datagram, header and sequence included: the PMON format does not
//! change, and a phone that does not need the copy drops it as a duplicate, which it always did (its
//! sequence is not newer than the last one played).
//!
//! By default (delay 0) the copy goes right after its original, in the same block's send: no
//! latency is added, and a phone that lost the original has the copy before the next datagram, so
//! it plays it in order, as every phone always did. That covers isolated losses, the usual kind.
//!
//! The desk may instead delay the copy 1 to [MAX_REDUNDANCY_DELAY_BLOCKS] blocks, right after that
//! later block's own datagram: a Wi-Fi burst that takes a packet must then last that long to take
//! its copy too. A delayed copy arrives after newer audio, so only a phone that holds packets back
//! for that many blocks before playing them can use it ([Redundancy::hold_blocks]). The hold costs
//! that phone the delay in latency (at the usual 256-frame blocks, 5.3 ms a block).
//!
//! Redundancy costs that musician's link double the bandwidth (every datagram is sent twice) and
//! nothing for the others. The engine keeps the setting per musician (by their phone's client id,
//! else their address) for as long as it runs, whether or not they are here, as it keeps a playback
//! buffer.
//!
//! Real time: delayed copies of a musician's datagrams are held in a [HeldCopies] allocated off the
//! engine thread, when the desk turns redundancy on or the musician's row is created or moved; the
//! engine thread only copies bytes into it and out of it and sends them, with no allocation and no
//! lock. An immediate copy is the datagram just sent, sent again: nothing is held.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::mix::OUTPUT_CHANNELS;
use crate::pipeline::MAX_BLOCK_FRAMES;
use crate::protocol::HEADER_SIZE;
use crate::targets::MusicianKey;

/// The shortest delay of a copy, in blocks: 0 sends it right after its original.
pub const MIN_REDUNDANCY_DELAY_BLOCKS: u8 = 0;

/// The longest delay of a copy, in blocks: about 21 ms at 256-frame blocks.
pub const MAX_REDUNDANCY_DELAY_BLOCKS: u8 = 4;

/// The delay the desk uses when it names none: an immediate copy, which adds no latency and needs
/// nothing of the phone (see the module documentation).
pub const DEFAULT_REDUNDANCY_DELAY_BLOCKS: u8 = 0;

/// The longest datagram the engine sends: a header and the stereo 16-bit samples of the longest
/// block.
pub(crate) const MAX_DATAGRAM_BYTES: usize =
    HEADER_SIZE + 2 * OUTPUT_CHANNELS as usize * MAX_BLOCK_FRAMES;

/// Whether a musician's audio is sent twice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Redundancy {
    /// Each datagram is sent once (the default).
    #[default]
    Off,
    /// Each datagram is sent again `delay_blocks` blocks later (0: right after itself).
    On { delay_blocks: u8 },
}

impl Redundancy {
    /// Redundancy with copies `delay_blocks` blocks late (0: immediate), or an error naming the
    /// range when it is outside [MIN_REDUNDANCY_DELAY_BLOCKS]-[MAX_REDUNDANCY_DELAY_BLOCKS].
    pub fn on(delay_blocks: u32) -> Result<Self, String> {
        let range = u32::from(MIN_REDUNDANCY_DELAY_BLOCKS)..=u32::from(MAX_REDUNDANCY_DELAY_BLOCKS);
        if !range.contains(&delay_blocks) {
            return Err(format!(
                "a copy must be from {MIN_REDUNDANCY_DELAY_BLOCKS} to {MAX_REDUNDANCY_DELAY_BLOCKS} \
                 blocks late, got {delay_blocks}"
            ));
        }
        Ok(Redundancy::On {
            delay_blocks: delay_blocks as u8,
        })
    }

    /// The delay of the copies in blocks; `None` when off.
    pub fn delay_blocks(self) -> Option<u8> {
        match self {
            Redundancy::Off => None,
            Redundancy::On { delay_blocks } => Some(delay_blocks),
        }
    }

    /// The blocks a phone must hold packets back to play a late copy in place of a lost original:
    /// the delay of the copies; 0 while off or with immediate copies, which arrive in order.
    pub fn hold_blocks(self) -> u8 {
        self.delay_blocks().unwrap_or(0)
    }
}

/// What [crate::EngineHandle::set_redundancy] did, and for whom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedundancySetting {
    /// The key the engine keeps the setting under: the musician's phone's client id when it
    /// announced one, else their address.
    pub kept_for: MusicianKey,
    /// The musician's row, as the status reports it.
    pub address: SocketAddr,
    pub redundancy: Redundancy,
}

/// The datagrams of one musician waiting to be sent again: one slot per block of delay, each with
/// room for the longest datagram, allocated once when it is made. For immediate copies it holds
/// nothing: the network sink sends the datagram it just sent again.
///
/// Only the engine thread uses it once it is in the target table, so its atomics are never
/// contended: they are what lets the table share it with that thread without a lock, the same way
/// the send counters are shared. Bytes are held in 64-bit words.
pub(crate) struct HeldCopies {
    delay_blocks: u8,
    slot_words: usize,
    words: Box<[AtomicU64]>,
    /// Each slot's datagram length in bytes; 0 while it holds none.
    lengths: Box<[AtomicUsize]>,
    /// The slot the next block's copy is taken from and its datagram held in.
    next: AtomicUsize,
}

impl HeldCopies {
    /// Room for `delay_blocks` datagrams of up to [MAX_DATAGRAM_BYTES], all empty; none for
    /// immediate copies. Allocates: never on the engine thread.
    pub(crate) fn new(delay_blocks: u8) -> Self {
        let slots = usize::from(delay_blocks);
        let slot_words = MAX_DATAGRAM_BYTES.div_ceil(8);
        Self {
            delay_blocks,
            slot_words,
            words: (0..slots * slot_words).map(|_| AtomicU64::new(0)).collect(),
            lengths: (0..slots).map(|_| AtomicUsize::new(0)).collect(),
            next: AtomicUsize::new(0),
        }
    }

    pub(crate) fn delay_blocks(&self) -> u8 {
        self.delay_blocks
    }

    /// One block's turn: when a datagram was held `delay_blocks` blocks ago, it is copied into
    /// `copy` (which must already have room for it, so nothing allocates) and the answer is true;
    /// then `current` is held in its place. A datagram longer than a slot is not held. A store for
    /// immediate copies holds nothing and answers false.
    pub(crate) fn exchange(&self, current: &[u8], copy: &mut Vec<u8>) -> bool {
        if self.delay_blocks == 0 {
            return false;
        }
        let slot = self.next.load(Ordering::Relaxed);
        let words = &self.words[slot * self.slot_words..(slot + 1) * self.slot_words];
        let held = self.lengths[slot].load(Ordering::Relaxed);
        let taken = held > 0 && held <= copy.capacity();
        if taken {
            copy.clear();
            copy.resize(held, 0);
            for (chunk, word) in copy.chunks_mut(8).zip(words) {
                let bytes = word.load(Ordering::Relaxed).to_le_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
        }
        if current.len() <= self.slot_words * 8 {
            for (chunk, word) in current.chunks(8).zip(words) {
                let mut bytes = [0u8; 8];
                bytes[..chunk.len()].copy_from_slice(chunk);
                word.store(u64::from_le_bytes(bytes), Ordering::Relaxed);
            }
            self.lengths[slot].store(current.len(), Ordering::Relaxed);
        } else {
            self.lengths[slot].store(0, Ordering::Relaxed);
        }
        self.next.store(
            (slot + 1) % usize::from(self.delay_blocks),
            Ordering::Relaxed,
        );
        taken
    }

    /// Forgets every datagram held, so none of them is sent again.
    pub(crate) fn clear(&self) {
        for length in self.lengths.iter() {
            length.store(0, Ordering::Relaxed);
        }
        self.next.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redundancy_is_on_with_a_delay_within_its_range() {
        assert_eq!(Redundancy::on(0), Ok(Redundancy::On { delay_blocks: 0 }));
        assert_eq!(Redundancy::on(1), Ok(Redundancy::On { delay_blocks: 1 }));
        assert_eq!(Redundancy::on(4), Ok(Redundancy::On { delay_blocks: 4 }));
        for delay in [5, 300] {
            let error = Redundancy::on(delay).expect_err("out of range");
            assert!(error.contains("from 0 to 4 blocks"), "{error}");
            assert!(error.contains(&format!("got {delay}")), "{error}");
        }
        assert_eq!(Redundancy::default(), Redundancy::Off);
        assert_eq!(Redundancy::Off.delay_blocks(), None);
        assert_eq!(Redundancy::On { delay_blocks: 3 }.delay_blocks(), Some(3));
        assert_eq!(
            Redundancy::on(u32::from(DEFAULT_REDUNDANCY_DELAY_BLOCKS)),
            Ok(Redundancy::On { delay_blocks: 0 }),
            "the copy goes right after its original unless the desk asks for a delay"
        );
    }

    /// The hold a phone needs to play a late copy: the delay while on, none while off or with
    /// immediate copies (which arrive before the next datagram).
    #[test]
    fn the_hold_is_the_delay_of_the_copies() {
        assert_eq!(Redundancy::Off.hold_blocks(), 0);
        assert_eq!(Redundancy::On { delay_blocks: 0 }.hold_blocks(), 0);
        assert_eq!(Redundancy::On { delay_blocks: 3 }.hold_blocks(), 3);
    }

    /// Immediate copies need no store: nothing is held, so nothing is ever sent from one.
    #[test]
    fn a_store_for_immediate_copies_holds_nothing() {
        let held = HeldCopies::new(0);
        assert_eq!(held.delay_blocks(), 0);
        let mut copy = Vec::with_capacity(MAX_DATAGRAM_BYTES);
        for block in 0..3 {
            assert!(!held.exchange(&datagram(block, 100), &mut copy));
        }
    }

    fn datagram(sequence: u8, length: usize) -> Vec<u8> {
        (0..length)
            .map(|index| sequence.wrapping_mul(31).wrapping_add(index as u8))
            .collect()
    }

    /// Each datagram comes back exactly, `delay_blocks` blocks later, whatever its length (not a
    /// whole number of words included); nothing comes back before the first delay has passed.
    #[test]
    fn each_datagram_comes_back_exactly_after_its_delay() {
        for delay in 1..=MAX_REDUNDANCY_DELAY_BLOCKS {
            // Delay 0 holds nothing (see a_store_for_immediate_copies_holds_nothing).
            let held = HeldCopies::new(delay);
            let mut copy = Vec::with_capacity(MAX_DATAGRAM_BYTES);
            let lengths = [1_044, 21, 1_043, 8, 7, 1_044, 64, 1];
            for (block, length) in lengths.iter().enumerate() {
                let current = datagram(block as u8, *length);
                let sent_again = held.exchange(&current, &mut copy);
                match block.checked_sub(usize::from(delay)) {
                    Some(earlier) => {
                        assert!(sent_again, "delay {delay}, block {block}");
                        assert_eq!(
                            copy,
                            datagram(earlier as u8, lengths[earlier]),
                            "delay {delay}, block {block} sends block {earlier} again"
                        );
                    }
                    None => assert!(!sent_again, "delay {delay}, block {block}: nothing yet"),
                }
            }
        }
    }

    #[test]
    fn a_cleared_store_sends_nothing_again_until_a_delay_has_passed() {
        let held = HeldCopies::new(2);
        let mut copy = Vec::with_capacity(MAX_DATAGRAM_BYTES);
        held.exchange(&datagram(0, 100), &mut copy);
        held.exchange(&datagram(1, 100), &mut copy);
        held.clear();
        assert!(!held.exchange(&datagram(2, 100), &mut copy));
        assert!(!held.exchange(&datagram(3, 100), &mut copy));
        assert!(held.exchange(&datagram(4, 100), &mut copy));
        assert_eq!(copy, datagram(2, 100));
    }

    /// The copy is written into the room the caller made for it: a buffer too small to take it
    /// without growing is left alone, so the engine thread never allocates for a copy.
    #[test]
    fn a_copy_never_grows_the_buffer_it_is_written_into() {
        let held = HeldCopies::new(1);
        let mut small = Vec::with_capacity(16);
        held.exchange(&datagram(0, 100), &mut small);
        assert!(!held.exchange(&datagram(1, 100), &mut small));
        assert!(small.capacity() < 100);

        let mut copy = Vec::with_capacity(MAX_DATAGRAM_BYTES);
        assert!(held.exchange(&datagram(2, 100), &mut copy));
        assert_eq!(copy, datagram(1, 100));
    }

    #[test]
    fn the_longest_datagram_is_held_whole() {
        let held = HeldCopies::new(1);
        let mut copy = Vec::with_capacity(MAX_DATAGRAM_BYTES);
        let longest = datagram(9, MAX_DATAGRAM_BYTES);
        held.exchange(&longest, &mut copy);
        assert!(held.exchange(&datagram(1, 10), &mut copy));
        assert_eq!(copy, longest);
        // One byte past it is never held, so nothing is sent again for it.
        held.exchange(&datagram(2, MAX_DATAGRAM_BYTES + 1), &mut copy);
        assert!(!held.exchange(&datagram(3, 10), &mut copy));
    }
}
