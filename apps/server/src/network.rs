use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::capture::TARGET_SAMPLE_RATE;
use crate::engine::OutputMeter;
use crate::mix::{MixPlan, MixState, OUTPUT_CHANNELS};
use crate::pipeline::{Block, Sink, SourceFormat};
use crate::protocol::{encode_audio_packet, HEADER_SIZE};
use crate::targets::TargetRegistry;

/// How many musicians the sink has room for before its first block; more grow the list once.
const EXPECTED_TARGETS: usize = 16;

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

/// One musician a block goes to, copied out of the target table: shared handles only, so the
/// copy is an address and three reference counts.
struct Destination {
    address: SocketAddr,
    mix: Arc<MixState>,
    counters: Arc<TargetCounters>,
    meter: OutputMeter,
}

/// The phones' sink: every block is mixed for each musician and sent to their phone over UDP.
///
/// The live target table is read once per block instead of being owned by the sink, so the engine
/// can add and remove targets at runtime without restarting the engine thread, the capture stream
/// or the control server. Each entry carries its own mix state and counters, so one block's send
/// loop never looks anything up by IP and can never fail on a missing entry.
///
/// Per block, the table's read guard is held only to copy the destinations into a reused list;
/// the mixing and the UDP sends run after it is released, so a slow send never holds up the engine
/// adding or removing a musician. Each mix is worked out once per block ([MixPlan]), mixed in float
/// into a reused stereo buffer and converted to 16 bits only as the packet is written into a reused
/// byte buffer: after the first block of a source, nothing here allocates.
pub(crate) struct NetworkSink {
    socket: UdpSocket,
    targets: Arc<TargetRegistry>,
    stats: Arc<PacketStats>,
    destinations: Vec<Destination>,
    stereo: Vec<f32>,
    packet: Vec<u8>,
    /// Runs right before each send, with the destination: lets a test act on the registry there.
    #[cfg(test)]
    before_send: Option<Box<dyn FnMut(SocketAddr) + Send>>,
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
            destinations: Vec::with_capacity(EXPECTED_TARGETS),
            stereo: Vec::new(),
            packet: Vec::new(),
            #[cfg(test)]
            before_send: None,
        })
    }
}

impl Sink for NetworkSink {
    fn process(&mut self, block: &Block<'_>) {
        self.destinations.clear();
        for entry in self.targets.read().iter() {
            self.destinations.push(Destination {
                address: entry.address,
                mix: Arc::clone(&entry.mix_state),
                counters: Arc::clone(&entry.counters),
                meter: entry.output_meter.clone(),
            });
        }
        // The table's guard is released: everything below works on the copies.

        let frames = block.samples.len() / block.channels.max(1);
        self.stereo
            .resize(frames * usize::from(OUTPUT_CHANNELS), 0.0);
        for destination in &self.destinations {
            let plan = MixPlan::new(&destination.mix.snapshot(), block.channels);
            plan.mix_into(block.samples, &mut self.stereo);
            // Two atomic updates on the buffer just mixed: no allocation, lock or clock.
            destination.meter.observe(&self.stereo);
            encode_audio_packet(
                &mut self.packet,
                OUTPUT_CHANNELS,
                TARGET_SAMPLE_RATE,
                block.sequence,
                &self.stereo,
            );
            #[cfg(test)]
            if let Some(hook) = &mut self.before_send {
                hook(destination.address);
            }
            match self.socket.send_to(&self.packet, destination.address) {
                Ok(_) => {
                    self.stats.sent.fetch_add(1, Ordering::Relaxed);
                    destination.counters.sent.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    self.stats.discarded.fetch_add(1, Ordering::Relaxed);
                    destination
                        .counters
                        .discarded
                        .fetch_add(1, Ordering::Relaxed);
                    eprintln!("UDP send error to {}: {error}", destination.address);
                }
            }
        }
        // Let go of the shared handles now, so a musician removed meanwhile is not kept alive.
        self.destinations.clear();
    }

    /// Sizes the mix and packet buffers for the new source's blocks, off the per-block path.
    fn source_changed(&mut self, format: SourceFormat) {
        let samples = format.block_frames * usize::from(OUTPUT_CHANNELS);
        self.stereo
            .reserve(samples.saturating_sub(self.stereo.len()));
        self.packet
            .reserve((HEADER_SIZE + 2 * samples).saturating_sub(self.packet.len()));
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
    packets: std::sync::mpsc::Receiver<crate::protocol::AudioPacket>,
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
    use crate::pipeline::{CaptureLinks, CaptureStats, CaptureWriter, Pipeline, PipelineHandle};
    use crate::protocol::{serialize_packet, AudioPacket};
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    fn bind(value: &str) -> UdpSocket {
        UdpSocket::bind(value).expect("test socket should bind")
    }

    /// A phone on loopback address `127.0.0.{host}`, or `None` when the machine has no such
    /// address. The engine keys musicians by IP, so a test with several musicians needs several
    /// loopback addresses. Linux and Windows answer on all of 127/8, so there a failed bind is a
    /// real failure and the test fails. macOS only configures 127.0.0.1 unless told otherwise
    /// (`sudo ifconfig lo0 alias 127.0.0.2`): there the test reports itself skipped and returns,
    /// unless `MIXLINK_REQUIRE_LOOPBACK_ALIASES` is set, which makes the gap a failure too.
    fn musician_phone(host: u8) -> Option<UdpSocket> {
        match UdpSocket::bind(format!("127.0.0.{host}:0")) {
            Ok(socket) => Some(socket),
            Err(error) => {
                let required = cfg!(any(target_os = "windows", target_os = "linux"))
                    || std::env::var_os("MIXLINK_REQUIRE_LOOPBACK_ALIASES").is_some();
                assert!(
                    !required,
                    "127.0.0.{host} cannot be bound ({error}); this test needs it for a second musician"
                );
                eprintln!(
                    "SKIPPED: 127.0.0.{host} cannot be bound ({error}); add the loopback alias or set MIXLINK_REQUIRE_LOOPBACK_ALIASES to make this a failure"
                );
                None
            }
        }
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
        let Some(kept) = musician_phone(2) else {
            return;
        };
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

        // About 30 steps of 720 frames is 0.45 s; poll with nothing sent, for at most 5 s.
        let deadline = Instant::now() + Duration::from_secs(5);
        while (targets.read()[0].output_meter.read() != [0, 0] || inputs.read(2) != [0, 0])
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(targets.read()[0].output_meter.read(), [0, 0]);
        assert_eq!(inputs.read(2), vec![0, 0]);

        engine.stop().expect("the engine thread stops");
    }

    /// The addition is visible to the same running send thread on the next packet: no restart, no
    /// reconfiguration of the capture stream or the control server.
    #[test]
    fn an_added_target_starts_receiving_without_restarting_the_network_thread() {
        let first = bind("127.0.0.1:0");
        let Some(second) = musician_phone(2) else {
            return;
        };
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

    fn datagram(socket: &UdpSocket) -> Vec<u8> {
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        let mut buffer = vec![0u8; 65_507];
        let length = socket.recv(&mut buffer).expect("a packet should arrive");
        buffer.truncate(length);
        buffer
    }

    /// The phones are sent what they were sent before the float path, byte for byte: for 16-bit
    /// capture, each packet equals the previous 16-bit mixer's output serialized the previous way,
    /// header and sequence included, across hundreds of drawn mixes and inputs.
    #[test]
    fn phones_receive_the_same_packets_as_from_the_16_bit_mixer() {
        use crate::mix::golden::{legacy_mix_channels, Noise};
        use crate::pipeline::CaptureSample;

        let phone = bind("127.0.0.1:0");
        let targets = registry(&[phone.local_addr().expect("phone address")]);
        let mix = Arc::clone(&targets.read()[0].mix_state);
        let mut sink = NetworkSink::new(Arc::clone(&targets), stats()).expect("sink should bind");
        let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
        let mut largest_difference = 0;

        for sequence in 0..400u64 {
            let channels = [1, 2, 3, 4, 8, 32, 33][noise.below(7) as usize];
            let frames = 1 + noise.below(64) as usize;
            let values = noise.mix();
            mix.update(values);
            mix.set_channel_flags(crate::mix::ChannelFlags {
                hidden: values.hidden_channels,
                linked: 0,
            });
            let input: Vec<i16> = (0..channels * frames).map(|_| noise.sample()).collect();
            let block: Vec<f32> = input.iter().map(|sample| sample.to_f32()).collect();

            sink.process(&Block {
                samples: &block,
                channels,
                sequence,
            });

            let expected = serialize_packet(&AudioPacket {
                channels: OUTPUT_CHANNELS,
                sample_rate: TARGET_SAMPLE_RATE,
                sequence,
                samples: legacy_mix_channels(&input, channels, mix.snapshot()),
            });
            let received = datagram(&phone);
            assert_eq!(received.len(), expected.len());
            assert_eq!(
                received[..20],
                expected[..20],
                "header of packet {sequence}"
            );
            for (got, want) in received[20..]
                .chunks_exact(2)
                .zip(expected[20..].chunks_exact(2))
            {
                let got = i16::from_le_bytes([got[0], got[1]]);
                let want = i16::from_le_bytes([want[0], want[1]]);
                largest_difference =
                    largest_difference.max((i32::from(got) - i32::from(want)).abs());
            }
            assert!(
                largest_difference <= 1,
                "packet {sequence} is more than 1 LSB off"
            );
            assert_eq!(received, expected, "packet {sequence} differs");
        }
    }

    /// The send happens after the target table's guard is released, so the engine can add or remove
    /// a musician while a block is on its way: here a removal runs inside the send itself, which
    /// would deadlock if the send still held the table.
    #[test]
    fn a_target_can_be_removed_while_a_block_is_being_sent() {
        let first = bind("127.0.0.1:0");
        let Some(second) = musician_phone(2) else {
            return;
        };
        let second_address = second.local_addr().expect("second address");
        let targets = registry(&[first.local_addr().expect("first address"), second_address]);
        let mut sink = NetworkSink::new(Arc::clone(&targets), stats()).expect("sink should bind");
        let registry = Arc::clone(&targets);
        sink.before_send = Some(Box::new(move |_| {
            let _ = registry.remove(&second_address.to_string());
        }));

        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sink.process(&Block {
                samples: &[0.0, 0.0],
                channels: 2,
                sequence: 0,
            });
            let _ = done.send(sink);
        });

        assert!(
            finished.recv_timeout(Duration::from_secs(2)).is_ok(),
            "the send held the target table"
        );
        assert!(receive(&first, Duration::from_secs(2)).is_some());
        assert_eq!(targets.read().len(), 1);
    }

    /// The whole per-block path after warm-up, as it runs live: a 24-bit, 96 kHz stereo callback
    /// converted, filtered, decimated and metered into the ring, then each block mixed for three
    /// phones, metered, encoded and sent, and mixed and metered for the local mix, with no local
    /// output and then with one playing. Not one heap allocation or free on the way.
    #[test]
    fn the_capture_to_phone_path_never_allocates() {
        use crate::pipeline::tests::allocations;

        // Three phones, the second with a centred mix of its own. Where only 127.0.0.1 can be
        // bound (macOS without aliases, see [musician_phone]) the path runs with that one phone,
        // which then takes the centred mix, rather than not at all.
        let phones: Vec<UdpSocket> = (1..=3).filter_map(musician_phone).collect();
        let addresses: Vec<SocketAddr> = phones
            .iter()
            .map(|phone| phone.local_addr().expect("phone address"))
            .collect();
        let targets = registry(&addresses);
        let centred = addresses.len().min(2) - 1;
        targets.read()[centred]
            .mix_state
            .apply(&MixUpdate {
                pans: Some(vec![50, 50]),
                volume_percent: Some(70),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        // The phones drain their sockets on their own threads, which are not watched.
        let draining = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let drains: Vec<_> = phones
            .into_iter()
            .map(|phone| {
                let draining = Arc::clone(&draining);
                std::thread::spawn(move || {
                    phone
                        .set_read_timeout(Some(Duration::from_millis(20)))
                        .expect("read timeout should set");
                    let mut buffer = [0u8; 4096];
                    while draining.load(Ordering::Relaxed) {
                        let _ = phone.recv(&mut buffer);
                    }
                })
            })
            .collect();

        let links = CaptureLinks {
            levels: Arc::new(LevelMeter::new()),
            stats: Arc::new(CaptureStats::default()),
            wake: None,
        };
        let (mut capture, reader) = CaptureWriter::new(
            96_000,
            SourceFormat {
                channels: 2,
                block_frames: 128,
            },
            links.clone(),
        )
        .expect("capture should build");
        let sink = NetworkSink::new(Arc::clone(&targets), stats()).expect("sink should bind");
        let local_mix = Arc::new(crate::mix::MixState::default());
        let (local, mut outputs) =
            crate::output::local_output_sink(local_mix, crate::engine::OutputMeter::default());
        let mut pipeline = Pipeline::new(vec![Box::new(sink), Box::new(local)], links.levels);
        pipeline.set_source(reader);
        let callback: Vec<i32> = (0..2 * 256).map(|index| (index * 4_096) << 8).collect();
        for _ in 0..4 {
            capture.write(&callback);
            pipeline.pump();
        }

        // Both sinks, with no local output.
        let without_output = allocations::count(|| {
            for _ in 0..1_000 {
                capture.write(&callback);
                pipeline.pump();
            }
        });

        // Both sinks with a local output playing (its device callback runs here too), after the
        // block that installs it.
        let (feed, mut player, _shared) = crate::output::output_link(2, 0, 128);
        outputs.send(Some(feed)).expect("the sink listens");
        capture.write(&callback);
        pipeline.pump();
        let mut device_buffer = vec![0.0f32; 2 * 128];
        let with_output = allocations::count(|| {
            for _ in 0..1_000 {
                capture.write(&callback);
                pipeline.pump();
                player.fill(&mut device_buffer);
            }
        });

        draining.store(false, Ordering::Relaxed);
        for drain in drains {
            drain.join().expect("drain thread");
        }
        assert_eq!(without_output, (0, 0), "no local output");
        assert_eq!(with_output, (0, 0), "a local output playing");
    }
}
