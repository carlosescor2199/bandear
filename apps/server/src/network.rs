use std::net::UdpSocket;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::capture::TARGET_SAMPLE_RATE;
use crate::mix::{mix_channels, OUTPUT_CHANNELS};
use crate::pipeline::{Block, Sink};
use crate::protocol::{serialize_packet, to_pcm16, AudioPacket};
use crate::targets::TargetRegistry;

pub(crate) struct PacketStats {
    pub(crate) sent: AtomicU64,
    pub(crate) discarded: AtomicU64,
}

/// Send counters for one target, so the UI can attribute loss to a musician instead of a global
/// total.
///
/// [PacketStats] stays as the global total the CLI summary prints; these are additive and do not
/// change what is sent on the wire.
#[derive(Default)]
pub(crate) struct TargetCounters {
    pub(crate) sent: AtomicU64,
    pub(crate) discarded: AtomicU64,
}

/// The phones' sink: every block is mixed for each musician and sent to their phone over UDP.
///
/// The live target table is read once per block instead of being owned by the sink, so the engine
/// can add and remove targets at runtime without restarting the engine thread, the capture stream
/// or the control server. Each entry carries its own mix state and counters, so one block's send
/// loop never looks anything up by IP and can never fail on a missing entry.
pub(crate) struct NetworkSink {
    socket: UdpSocket,
    targets: Arc<TargetRegistry>,
    stats: Arc<PacketStats>,
}

impl NetworkSink {
    pub(crate) fn new(
        targets: Arc<TargetRegistry>,
        stats: Arc<PacketStats>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind("0.0.0.0:0")?,
            targets,
            stats,
        })
    }
}

impl Sink for NetworkSink {
    fn process(&mut self, block: &Block<'_>) {
        let source: Vec<i16> = block
            .samples
            .iter()
            .map(|sample| to_pcm16(*sample))
            .collect();
        let entries = self.targets.read();
        for entry in entries.iter() {
            let samples = mix_channels(&source, block.channels, entry.mix_state.snapshot());
            // Two atomic updates on the buffer just mixed: no allocation, lock or clock.
            entry.output_meter.observe(&samples);
            let target_packet = AudioPacket {
                channels: OUTPUT_CHANNELS,
                sample_rate: TARGET_SAMPLE_RATE,
                sequence: block.sequence,
                samples,
            };
            let bytes = serialize_packet(&target_packet);
            match self.socket.send_to(&bytes, entry.address) {
                Ok(_) => {
                    self.stats.sent.fetch_add(1, Ordering::Relaxed);
                    entry.counters.sent.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    self.stats.discarded.fetch_add(1, Ordering::Relaxed);
                    entry.counters.discarded.fetch_add(1, Ordering::Relaxed);
                    eprintln!("UDP send error to {}: {error}", entry.address);
                }
            }
        }
    }

    /// Every musician's output meter falls with the silence, as the input meters do.
    fn silence(&mut self, frames: usize) {
        for entry in self.targets.read().iter() {
            entry.output_meter.fall(frames);
        }
    }
}

/// A send thread fed 16-bit packets instead of capture blocks: the network sink exactly as the
/// engine thread runs it, for the tests that drive it from other modules.
#[cfg(test)]
pub(crate) fn spawn_network_thread(
    packets: std::sync::mpsc::Receiver<AudioPacket>,
    targets: Arc<TargetRegistry>,
    stats: Arc<PacketStats>,
    _levels: Arc<crate::engine::LevelMeter>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    use crate::pipeline::CaptureSample;

    let mut sink = NetworkSink::new(targets, stats)?;
    Ok(std::thread::spawn(move || {
        while let Ok(packet) = packets.recv() {
            let samples: Vec<f32> = packet
                .samples
                .iter()
                .map(|sample| sample.to_f32())
                .collect();
            sink.process(&Block {
                samples: &samples,
                channels: usize::from(packet.channels),
                sequence: packet.sequence,
            });
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::LevelMeter;
    use crate::mix::{GroupLayout, MixUpdate};
    use crate::pipeline::{
        CaptureLinks, CaptureStats, CaptureWriter, Pipeline, PipelineHandle, SourceFormat,
    };
    use std::net::SocketAddr;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    fn bind(value: &str) -> UdpSocket {
        UdpSocket::bind(value).expect("test socket should bind")
    }

    fn packet() -> AudioPacket {
        AudioPacket {
            channels: 2,
            sample_rate: 48_000,
            sequence: 0,
            samples: vec![0, 0],
        }
    }

    fn receive(socket: &UdpSocket, timeout: Duration) -> Option<usize> {
        socket
            .set_read_timeout(Some(timeout))
            .expect("read timeout should set");
        let mut buffer = [0u8; 4096];
        socket.recv(&mut buffer).ok()
    }

    fn registry(addresses: &[SocketAddr]) -> Arc<TargetRegistry> {
        Arc::new(TargetRegistry::new(addresses, &GroupLayout::default()).expect("registry"))
    }

    fn stats() -> Arc<PacketStats> {
        Arc::new(PacketStats {
            sent: AtomicU64::new(0),
            discarded: AtomicU64::new(0),
        })
    }

    fn inputs() -> Arc<LevelMeter> {
        Arc::new(LevelMeter::new())
    }

    /// Removal happens under the registry's write lock, before the next packet is queued, so the
    /// send thread must see the shorter table on the next packet: the removed target gets nothing,
    /// the kept one keeps streaming, and the thread is never restarted.
    #[test]
    fn a_removed_target_stops_receiving_while_the_others_keep_streaming() {
        let removed = bind("127.0.0.1:0");
        let kept = bind("127.0.0.2:0");
        let targets = registry(&[
            removed.local_addr().expect("removed address"),
            kept.local_addr().expect("kept address"),
        ]);
        let (sender, receiver) = sync_channel(4);
        let thread = spawn_network_thread(receiver, Arc::clone(&targets), stats(), inputs())
            .expect("network thread should spawn");

        sender.send(packet()).expect("packet should queue");
        assert!(receive(&removed, Duration::from_secs(2)).is_some());
        assert!(receive(&kept, Duration::from_secs(2)).is_some());

        targets
            .remove(&removed.local_addr().expect("removed address").to_string())
            .expect("configured target should be removed");

        sender.send(packet()).expect("packet should queue");
        assert!(receive(&kept, Duration::from_secs(2)).is_some());
        assert!(receive(&removed, Duration::from_millis(300)).is_none());

        drop(sender);
        thread
            .join()
            .expect("thread should exit when the sender drops");
    }

    /// Each musician's meter reads the stereo mix that musician is sent, after their own gains and
    /// master, not the input: here the input peaks near 91 and 61 and the half-volume mix at 45 and
    /// 30.
    #[test]
    fn the_send_loop_meters_each_musicians_mixed_output() {
        let phone = bind("127.0.0.1:0");
        let targets = registry(&[phone.local_addr().expect("phone address")]);
        targets.read()[0]
            .mix_state
            .apply(&MixUpdate {
                volume_percent: Some(50),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let (sender, receiver) = sync_channel(4);
        let thread = spawn_network_thread(receiver, Arc::clone(&targets), stats(), inputs())
            .expect("network thread should spawn");

        sender
            .send(AudioPacket {
                samples: vec![30_000, 20_000],
                ..packet()
            })
            .expect("packet should queue");
        // Closing the queue right away ends the thread after this packet, before any wait could
        // count as silence, so the level read below is exactly the packet's.
        drop(sender);
        thread
            .join()
            .expect("thread should exit when the sender drops");
        assert!(receive(&phone, Duration::from_secs(2)).is_some());

        assert_eq!(targets.read()[0].output_meter.read(), [45, 30]);
    }

    /// When the capture stops delivering, no audio time passes for a meter to fall by. The engine
    /// thread counts its wait as silence, so the input and output meters fall to zero instead of
    /// freezing on the last level.
    #[test]
    fn meters_fall_to_zero_when_the_audio_stops() {
        let phone = bind("127.0.0.1:0");
        let targets = registry(&[phone.local_addr().expect("phone address")]);
        let inputs = Arc::new(LevelMeter::new());
        let sink = NetworkSink::new(Arc::clone(&targets), stats()).expect("sink should bind");
        let engine =
            PipelineHandle::spawn(Pipeline::new(vec![Box::new(sink)], Arc::clone(&inputs)))
                .expect("engine thread should spawn");
        let (mut capture, reader) = CaptureWriter::new(
            48_000,
            SourceFormat {
                channels: 2,
                block_frames: 1,
            },
            CaptureLinks {
                levels: Arc::clone(&inputs),
                stats: Arc::new(CaptureStats::default()),
                wake: Some(engine.waker()),
            },
        )
        .expect("capture should build");
        engine
            .set_source(reader)
            .expect("the engine takes the source");

        // The capture's last block before the device goes quiet: 30 and 20 on the meters.
        capture.write(&[9_830i16, 6_554]);
        assert!(receive(&phone, Duration::from_secs(2)).is_some());

        // About 30 steps of 720 frames is 0.45 s of audio; wait well past it with nothing sent.
        std::thread::sleep(Duration::from_secs(1));

        assert_eq!(targets.read()[0].output_meter.read(), [0, 0]);
        assert_eq!(inputs.read(2), vec![0, 0]);

        engine.stop().expect("the engine thread stops");
    }

    /// The addition is visible to the same running send thread on the next packet: no restart, no
    /// reconfiguration of the capture stream or the control server.
    #[test]
    fn an_added_target_starts_receiving_without_restarting_the_network_thread() {
        let first = bind("127.0.0.1:0");
        let second = bind("127.0.0.2:0");
        let targets = registry(&[first.local_addr().expect("first address")]);
        let (sender, receiver) = sync_channel(4);
        let thread = spawn_network_thread(receiver, Arc::clone(&targets), stats(), inputs())
            .expect("network thread should spawn");

        sender.send(packet()).expect("packet should queue");
        assert!(receive(&first, Duration::from_secs(2)).is_some());
        assert!(receive(&second, Duration::from_millis(300)).is_none());

        targets
            .add(
                &second.local_addr().expect("second address").to_string(),
                &GroupLayout::default(),
            )
            .expect("fresh address should be accepted");

        sender.send(packet()).expect("packet should queue");
        assert!(receive(&first, Duration::from_secs(2)).is_some());
        assert!(receive(&second, Duration::from_secs(2)).is_some());

        drop(sender);
        thread
            .join()
            .expect("thread should exit when the sender drops");
    }
}
