//! The playback buffer the desk sets for a musician's phone: "Auto", the phone's own adaptive
//! cushion, or a fixed length in milliseconds the phone keeps without adapting.
//!
//! Only a phone that announced [crate::CAPABILITY_PLAYBACK_BUFFER] is ever sent it (the `playback`
//! message, see [crate::protocol]); the engine keeps the engineer's choice per musician (by their
//! phone's client id, else their address) for as long as it runs, and sends it again whenever that
//! phone registers, so the choice survives a reconnect. The phone converts the length to whole
//! packets at its own block size and says what it plays in its `stats` (`cushion_ms`), which is the
//! only acknowledgement there is.

use std::net::SocketAddr;

use crate::targets::MusicianKey;

/// The shortest fixed buffer the desk may set, in milliseconds: about one packet of the shortest
/// block a phone plays.
pub const MIN_FIXED_BUFFER_MS: u32 = 5;

/// The longest fixed buffer the desk may set, in milliseconds. Half a second is far past any
/// cushion a musician can play along with; it is there for a link that must not drop out at all.
pub const MAX_FIXED_BUFFER_MS: u32 = 500;

/// The playback buffer of one musician's phone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackBuffer {
    /// The phone adapts its cushion on its own (the default, and what an older phone always does).
    #[default]
    Auto,
    /// The phone keeps a cushion of this many milliseconds (rounded up to whole packets, within
    /// what its audio track can hold) and does not adapt it.
    Fixed { ms: u16 },
}

impl PlaybackBuffer {
    /// A fixed buffer of `ms` milliseconds, or an error naming the range when it is outside
    /// [MIN_FIXED_BUFFER_MS]-[MAX_FIXED_BUFFER_MS].
    pub fn fixed(ms: u32) -> Result<Self, String> {
        if !(MIN_FIXED_BUFFER_MS..=MAX_FIXED_BUFFER_MS).contains(&ms) {
            return Err(format!(
                "a fixed buffer must be between {MIN_FIXED_BUFFER_MS} and {MAX_FIXED_BUFFER_MS} ms, \
                 got {ms} ms"
            ));
        }
        Ok(PlaybackBuffer::Fixed { ms: ms as u16 })
    }

    /// The mode's name on the wire and in the desk: `auto` or `fixed`.
    pub fn mode(self) -> &'static str {
        match self {
            PlaybackBuffer::Auto => "auto",
            PlaybackBuffer::Fixed { .. } => "fixed",
        }
    }

    /// The fixed length in milliseconds; `None` for Auto.
    pub fn fixed_ms(self) -> Option<u16> {
        match self {
            PlaybackBuffer::Auto => None,
            PlaybackBuffer::Fixed { ms } => Some(ms),
        }
    }
}

/// What became of a buffer the desk set (see [crate::EngineHandle::set_playback_buffer]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferOutcome {
    /// The phone's control channel is connected and was sent it.
    Sent,
    /// The engine keeps it, and the phone gets it when its control channel is back and it
    /// registers again.
    NotConnected,
    /// The musician's phone did not announce [crate::CAPABILITY_PLAYBACK_BUFFER] (an older app, or
    /// a configured target with no phone): nothing was kept or sent.
    Unsupported,
}

/// What [crate::EngineHandle::set_playback_buffer] did, and for whom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferSetting {
    /// The key the engine keeps the buffer under: the musician's phone's client id when it
    /// announced one, else their address. For [BufferOutcome::Unsupported], nothing is kept and
    /// this is the key the musician was named by.
    pub kept_for: MusicianKey,
    /// The musician's row, as the status reports it.
    pub address: SocketAddr,
    pub outcome: BufferOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_buffer_is_kept_within_its_range() {
        assert_eq!(
            PlaybackBuffer::fixed(40),
            Ok(PlaybackBuffer::Fixed { ms: 40 })
        );
        assert_eq!(
            PlaybackBuffer::fixed(MIN_FIXED_BUFFER_MS),
            Ok(PlaybackBuffer::Fixed { ms: 5 })
        );
        assert_eq!(
            PlaybackBuffer::fixed(MAX_FIXED_BUFFER_MS),
            Ok(PlaybackBuffer::Fixed { ms: 500 })
        );
    }

    #[test]
    fn a_fixed_buffer_outside_its_range_is_refused_naming_it() {
        for ms in [0, 4, 501, 70_000] {
            let error = PlaybackBuffer::fixed(ms).expect_err("out of range");
            assert!(error.contains("between 5 and 500 ms"), "{error}");
            assert!(error.contains(&format!("got {ms} ms")), "{error}");
        }
    }

    #[test]
    fn the_modes_are_named_as_the_wire_and_the_desk_name_them() {
        assert_eq!(PlaybackBuffer::Auto.mode(), "auto");
        assert_eq!(PlaybackBuffer::Auto.fixed_ms(), None);
        assert_eq!(PlaybackBuffer::Fixed { ms: 40 }.mode(), "fixed");
        assert_eq!(PlaybackBuffer::Fixed { ms: 40 }.fixed_ms(), Some(40));
        assert_eq!(PlaybackBuffer::default(), PlaybackBuffer::Auto);
    }
}
