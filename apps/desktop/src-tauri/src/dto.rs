//! The JSON contract between the webview and the engine.
//!
//! The engine's own types are not serialized directly: this module owns the wire shape the window
//! consumes, so the engine keeps no presentation concerns and can change without breaking the UI.

use serde::{Deserialize, Serialize};

use personal_monitoring::{
    CaptureFormat, EngineStatus, InputDevice, LinkHealth, MixUpdate, MixValues, MusicianStatus,
};

/// The start-up configuration the webview sends to [crate::commands::start_engine].
///
/// Every field is optional so the window can send a partial object and let the defaults fill in.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    #[serde(default)]
    pub device_filter: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub control_port: Option<u16>,
    #[serde(default)]
    pub groups: Vec<GroupRequest>,
    /// The capture backend: "auto", "asio" or "wasapi". `None` means "auto", which prefers ASIO.
    #[serde(default)]
    pub host: Option<String>,
    /// The capture buffer size in frames. `None` means the engine default.
    #[serde(default)]
    pub buffer_frames: Option<u32>,
}

/// One group as the window describes it. Channel numbers are 1-based, exactly as the CLI takes
/// them, because the conversion to the protocol's 0-based indices happens in the engine.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRequest {
    pub name: String,
    pub channels: Vec<usize>,
}

/// The capture format the engine selected, in plain values a webview can render.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFormatDto {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: String,
    pub buffer_size: String,
}

impl From<&CaptureFormat> for CaptureFormatDto {
    fn from(format: &CaptureFormat) -> Self {
        Self {
            channels: format.channels,
            sample_rate: format.sample_rate,
            sample_format: format.sample_format.clone(),
            buffer_size: format.buffer_size.clone(),
        }
    }
}

impl CaptureFormatDto {
    /// A one-line description in the same words the CLI prints at startup.
    pub fn describe(&self) -> String {
        format!(
            "{} ch @ {} Hz, {}, buffer {}",
            self.channels, self.sample_rate, self.sample_format, self.buffer_size
        )
    }
}

/// What [crate::commands::start_engine] returns: the two values T3 asks the window to show.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSummary {
    pub device_name: String,
    pub capture_format: CaptureFormatDto,
}

/// One musician row: the engine's target/control join, flattened for JSON.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicianDto {
    pub address: String,
    /// The name the musician typed on their phone and announced when registering, straight from the
    /// engine. It is client input, never a desktop label, and is never sent back to a client.
    pub client_name: Option<String>,
    /// The name this desk shows: the client's own announced name when it has one, otherwise the
    /// engineer's local label. Filled by the command layer; the engine knows nothing of the label.
    pub name: Option<String>,
    /// True when `name` is the desk's local label rather than the client's announced name, so the
    /// window can mark it as local. It is only ever true when the client announced no name; a
    /// client that announced one always wins, which is why the two cases need telling apart.
    pub name_is_local: bool,
    pub control_connected: bool,
    /// The musician's whole mix, flat on the row so `volumePercent`, `maxLevelPercent` and `muted`
    /// keep the names the window already reads.
    #[serde(flatten)]
    pub mix: MixDto,
    pub packets_sent: u64,
    pub packets_discarded: u64,
    /// The phone's link health as the server classified it: "unknown", "good", "warning" or "bad".
    pub link_health: &'static str,
    /// The rates the health was judged on. All four are absent when there is no fresh report: a
    /// phone that never reported, one that left, or a lost link ("bad" with no numbers to show).
    ///
    /// Packets the phone received per second; 0 means it hears no audio at all.
    pub packets_per_second: Option<f64>,
    pub underruns_per_second: Option<f64>,
    /// Lost packets as a percentage of the packets the phone expected.
    pub loss_percent: Option<f64>,
    pub receive_timeouts_per_second: Option<f64>,
}

/// One musician's whole mix.
///
/// The per-channel tables always hold the engine's full 32 entries (16 for groups), index-aligned
/// with the 0-based source channels and the group list; the window shows as many as the current
/// device has. Entries past that are the stored values for channels the device lacks.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixDto {
    pub volume_percent: u8,
    pub max_level_percent: u8,
    pub muted: bool,
    pub channel_gains: Vec<u8>,
    pub pans: Vec<u8>,
    pub channel_muted: Vec<bool>,
    pub channel_solo: Vec<bool>,
    pub group_levels: Vec<u8>,
}

impl From<&MixValues> for MixDto {
    fn from(mix: &MixValues) -> Self {
        Self {
            volume_percent: mix.volume_percent,
            max_level_percent: mix.max_level_percent,
            muted: mix.muted,
            channel_gains: mix.channel_gains.to_vec(),
            pans: mix.pans.to_vec(),
            channel_muted: mix.channel_muted.to_vec(),
            channel_solo: mix.channel_solo.to_vec(),
            group_levels: mix.group_levels.to_vec(),
        }
    }
}

/// A partial mix change the webview sends to [crate::commands::set_mix].
///
/// Every field is optional and an absent one keeps its value. A list replaces the values from the
/// first channel (or group) on, so the window can send a whole table or just its first entries;
/// levels are 0-100 and are clamped by the engine. A field the engine does not know is rejected
/// instead of ignored, so a misspelt name fails loudly rather than silently changing nothing.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MixUpdateRequest {
    #[serde(default)]
    pub volume_percent: Option<i32>,
    #[serde(default)]
    pub max_level_percent: Option<i32>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub channel_gains: Option<Vec<i32>>,
    #[serde(default)]
    pub pans: Option<Vec<i32>>,
    #[serde(default)]
    pub channel_muted: Option<Vec<bool>>,
    #[serde(default)]
    pub channel_solo: Option<Vec<bool>>,
    #[serde(default)]
    pub group_levels: Option<Vec<i32>>,
}

impl From<MixUpdateRequest> for MixUpdate {
    fn from(request: MixUpdateRequest) -> Self {
        Self {
            channel_gains: request.channel_gains,
            pans: request.pans,
            channel_muted: request.channel_muted,
            channel_solo: request.channel_solo,
            group_levels: request.group_levels,
            volume_percent: request.volume_percent,
            max_level_percent: request.max_level_percent,
            muted: request.muted,
        }
    }
}

/// One configured group, with the same 0-based source indices the engine uses.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub name: String,
    pub channels: Vec<usize>,
    /// True when the current input device is missing a channel this group names. The group is still
    /// shown and kept; the window uses this to mark it and to warn before a further switch.
    pub invalid: bool,
}

/// One selectable input device: its name and the channel count a switch would capture.
///
/// `channels` of `0` means the device has no supported PCM input, so it is listed but not usable.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceDto {
    pub name: String,
    pub channels: u16,
}

impl From<&InputDevice> for InputDeviceDto {
    fn from(device: &InputDevice) -> Self {
        Self {
            name: device.name.clone(),
            channels: device.channels,
        }
    }
}

/// One source channel as the window shows it: a 1-based number, its group when the engineer
/// configured one, and the live pre-mix peak.
///
/// There are no per-channel names in the system yet, so a channel is labelled by number and group
/// rather than by a name like "Kick". Naming is engineer configuration that does not exist, and
/// inventing names here would misrepresent the system.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDto {
    pub number: u8,
    pub group: Option<String>,
    pub level: u8,
}

impl From<&MusicianStatus> for MusicianDto {
    fn from(musician: &MusicianStatus) -> Self {
        Self {
            address: musician.address.to_string(),
            client_name: musician.name.clone(),
            name: None,
            name_is_local: false,
            control_connected: musician.control_connected,
            mix: MixDto::from(&musician.mix),
            packets_sent: musician.counters.packets_sent,
            packets_discarded: musician.counters.packets_discarded,
            link_health: link_health_name(musician.link.health),
            packets_per_second: musician.link.packets_per_second,
            underruns_per_second: musician.link.underruns_per_second,
            loss_percent: musician.link.loss_percent,
            receive_timeouts_per_second: musician.link.receive_timeouts_per_second,
        }
    }
}

/// The name of a link health level, in the lowercase the window matches on.
fn link_health_name(health: LinkHealth) -> &'static str {
    match health {
        LinkHealth::Unknown => "unknown",
        LinkHealth::Good => "good",
        LinkHealth::Warning => "warning",
        LinkHealth::Bad => "bad",
    }
}

/// A snapshot of the engine, ready for the webview.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatusDto {
    pub device_name: String,
    pub capture_format: CaptureFormatDto,
    pub control_port: u16,
    pub sample_rate: u32,
    pub source_channels: u8,
    pub groups: Vec<GroupDto>,
    pub channels: Vec<ChannelDto>,
    pub stopped: bool,
    pub samples_received: u64,
    pub packets_sent: u64,
    pub packets_discarded: u64,
    pub musicians: Vec<MusicianDto>,
}

impl From<&EngineStatus> for EngineStatusDto {
    fn from(status: &EngineStatus) -> Self {
        let groups: Vec<GroupDto> = status
            .groups
            .iter()
            .map(|group| GroupDto {
                name: group.name.clone(),
                channels: group.channels.clone(),
                invalid: group.invalid,
            })
            .collect();
        // The engine publishes the group layout once; the per-channel membership the window needs
        // is derived here rather than being stored twice and risking the two views disagreeing.
        let channels = (0..usize::from(status.source_channels))
            .map(|index| ChannelDto {
                number: (index + 1) as u8,
                // An invalid group is shown in the group list but does not own any channel, because
                // the mixer ignores it; labelling a channel with it would overstate what is applied.
                group: status
                    .groups
                    .iter()
                    .find(|group| !group.invalid && group.channels.contains(&index))
                    .map(|group| group.name.clone()),
                level: status.channel_levels.get(index).copied().unwrap_or(0),
            })
            .collect();
        Self {
            device_name: status.device_name.clone(),
            capture_format: (&status.capture_format).into(),
            control_port: status.control_port,
            sample_rate: status.sample_rate,
            source_channels: status.source_channels,
            groups,
            channels,
            stopped: status.stopped,
            samples_received: status.samples_received,
            packets_sent: status.packets_sent,
            packets_discarded: status.packets_discarded,
            musicians: status.musicians.iter().map(MusicianDto::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use personal_monitoring::{
        CaptureFormat, EngineStatus, GroupStatus, LinkHealth, LinkStatus, MusicianStatus,
    };

    fn status_with(groups: Vec<GroupStatus>, channel_levels: Vec<u8>) -> EngineStatus {
        EngineStatus {
            device_name: "Test device".to_owned(),
            capture_format: CaptureFormat {
                channels: 2,
                sample_rate: 48_000,
                sample_format: "I16".to_owned(),
                buffer_size: "Default".to_owned(),
            },
            control_port: 50001,
            sample_rate: 48_000,
            source_channels: 2,
            groups,
            channel_levels,
            stopped: false,
            samples_received: 0,
            packets_sent: 0,
            packets_discarded: 0,
            musicians: Vec::new(),
        }
    }

    #[test]
    fn channels_label_by_number_with_their_group_and_level() {
        let status = status_with(
            vec![GroupStatus {
                name: "Drums".to_owned(),
                channels: vec![0],
                invalid: false,
            }],
            vec![50, 100],
        );

        let dto = EngineStatusDto::from(&status);

        assert_eq!(dto.channels.len(), 2);
        assert_eq!(dto.channels[0].number, 1);
        assert_eq!(dto.channels[0].group.as_deref(), Some("Drums"));
        assert_eq!(dto.channels[0].level, 50);
        assert_eq!(dto.channels[1].number, 2);
        assert_eq!(dto.channels[1].group, None);
        assert_eq!(dto.channels[1].level, 100);
        assert_eq!(dto.groups.len(), 1);
        assert_eq!(dto.groups[0].channels, vec![0]);
    }

    #[test]
    fn a_channel_without_a_group_reports_no_group() {
        let status = status_with(Vec::new(), vec![0, 0]);

        let dto = EngineStatusDto::from(&status);

        assert!(dto.channels.iter().all(|channel| channel.group.is_none()));
        assert!(dto.groups.is_empty());
    }

    #[test]
    fn an_invalid_group_is_flagged_for_the_window_and_keeps_its_channels() {
        let status = status_with(
            vec![GroupStatus {
                name: "Vocals".to_owned(),
                channels: vec![2, 3],
                invalid: true,
            }],
            vec![0, 0],
        );

        let dto = EngineStatusDto::from(&status);

        assert!(dto.groups[0].invalid);
        assert_eq!(dto.groups[0].channels, vec![2, 3]);
    }

    #[test]
    fn the_engine_name_becomes_the_client_name_and_the_label_is_left_to_the_command_layer() {
        let musician = MusicianStatus {
            address: "192.168.1.30:50000".parse().expect("test address"),
            name: Some("Ana".to_owned()),
            control_connected: false,
            mix: Default::default(),
            counters: Default::default(),
            link: Default::default(),
        };

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.client_name.as_deref(), Some("Ana"));
        assert_eq!(dto.name, None);
        assert!(!dto.name_is_local);
    }

    #[test]
    fn a_musician_that_never_announced_a_name_reports_no_client_name() {
        let musician = MusicianStatus {
            address: "192.168.1.30:50000".parse().expect("test address"),
            name: None,
            control_connected: false,
            mix: Default::default(),
            counters: Default::default(),
            link: Default::default(),
        };

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.client_name, None);
        assert_eq!(dto.name, None);
        assert!(!dto.name_is_local);
    }

    fn musician_with_link(link: LinkStatus) -> MusicianStatus {
        MusicianStatus {
            address: "192.168.1.30:50000".parse().expect("test address"),
            name: None,
            control_connected: true,
            mix: Default::default(),
            counters: Default::default(),
            link,
        }
    }

    #[test]
    fn each_link_health_level_reaches_the_window_as_its_lowercase_name() {
        for (health, name) in [
            (LinkHealth::Unknown, "unknown"),
            (LinkHealth::Good, "good"),
            (LinkHealth::Warning, "warning"),
            (LinkHealth::Bad, "bad"),
        ] {
            let musician = musician_with_link(LinkStatus {
                health,
                ..LinkStatus::default()
            });

            assert_eq!(MusicianDto::from(&musician).link_health, name);
        }
    }

    #[test]
    fn the_rates_the_health_was_judged_on_are_passed_through() {
        let musician = musician_with_link(LinkStatus {
            health: LinkHealth::Warning,
            packets_per_second: Some(187.5),
            underruns_per_second: Some(0.5),
            loss_percent: Some(1.25),
            receive_timeouts_per_second: Some(0.0),
        });

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.link_health, "warning");
        assert_eq!(dto.packets_per_second, Some(187.5));
        assert_eq!(dto.underruns_per_second, Some(0.5));
        assert_eq!(dto.loss_percent, Some(1.25));
        assert_eq!(dto.receive_timeouts_per_second, Some(0.0));
    }

    #[test]
    fn a_musician_without_a_fresh_report_has_no_rates() {
        let never_reported = MusicianDto::from(&musician_with_link(LinkStatus::default()));
        let lost = MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            ..LinkStatus::default()
        }));

        assert_eq!(never_reported.link_health, "unknown");
        assert_eq!(
            lost.link_health, "bad",
            "a lost link is bad with no numbers"
        );
        for dto in [never_reported, lost] {
            assert_eq!(dto.packets_per_second, None);
            assert_eq!(dto.underruns_per_second, None);
            assert_eq!(dto.loss_percent, None);
            assert_eq!(dto.receive_timeouts_per_second, None);
        }
    }

    #[test]
    fn the_link_fields_are_camel_case_json_with_absent_rates_as_null() {
        let lost = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            ..LinkStatus::default()
        })))
        .expect("a musician serializes");
        let reporting = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Good,
            packets_per_second: Some(200.0),
            underruns_per_second: Some(0.0),
            loss_percent: Some(0.25),
            receive_timeouts_per_second: Some(0.0),
        })))
        .expect("a musician serializes");
        let silent = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            packets_per_second: Some(0.0),
            underruns_per_second: Some(0.0),
            loss_percent: Some(0.0),
            receive_timeouts_per_second: Some(0.0),
        })))
        .expect("a musician serializes");

        assert_eq!(lost["linkHealth"], "bad");
        assert!(lost["packetsPerSecond"].is_null());
        assert!(lost["underrunsPerSecond"].is_null());
        assert!(lost["lossPercent"].is_null());
        assert!(lost["receiveTimeoutsPerSecond"].is_null());
        assert_eq!(reporting["linkHealth"], "good");
        assert_eq!(reporting["packetsPerSecond"], 200.0);
        assert_eq!(reporting["underrunsPerSecond"], 0.0);
        assert_eq!(reporting["lossPercent"], 0.25);
        assert_eq!(reporting["receiveTimeoutsPerSecond"], 0.0);
        assert_eq!(silent["linkHealth"], "bad");
        assert_eq!(
            silent["packetsPerSecond"], 0.0,
            "a phone hearing nothing is a number the window can tell from a lost link"
        );
    }

    #[test]
    fn the_full_mix_reaches_the_window_in_camel_case() {
        let mut mix = MixValues {
            volume_percent: 42,
            max_level_percent: 90,
            muted: true,
            ..MixValues::default()
        };
        mix.channel_gains[1] = 30;
        mix.pans[0] = 50;
        mix.channel_muted[2] = true;
        mix.channel_solo[3] = true;
        mix.group_levels[0] = 70;
        let musician = MusicianStatus {
            mix,
            ..musician_with_link(LinkStatus::default())
        };

        let dto = MusicianDto::from(&musician);
        let json = serde_json::to_value(&dto).expect("a musician serializes");

        assert_eq!(dto.mix.channel_gains, mix.channel_gains.to_vec());
        assert_eq!(dto.mix.pans, mix.pans.to_vec());
        assert_eq!(dto.mix.channel_muted, mix.channel_muted.to_vec());
        assert_eq!(dto.mix.channel_solo, mix.channel_solo.to_vec());
        assert_eq!(dto.mix.group_levels, mix.group_levels.to_vec());
        // The mix sits flat on the row, so the fields the window already reads keep their names.
        assert_eq!(json["volumePercent"], 42);
        assert_eq!(json["maxLevelPercent"], 90);
        assert_eq!(json["muted"], true);
        assert_eq!(json["channelGains"][1], 30);
        assert_eq!(json["pans"][0], 50);
        assert_eq!(json["channelMuted"][2], true);
        assert_eq!(json["channelSolo"][3], true);
        assert_eq!(json["groupLevels"][0], 70);
        assert!(json.get("mix").is_none(), "no nested object: {json}");
    }

    #[test]
    fn a_mix_request_reads_camel_case_and_leaves_absent_fields_unset() {
        let request: MixUpdateRequest =
            serde_json::from_str(r#"{"pans":[10,90],"volumePercent":80,"channelSolo":[true]}"#)
                .expect("a partial mix parses");

        let update = MixUpdate::from(request);

        assert_eq!(
            update,
            MixUpdate {
                pans: Some(vec![10, 90]),
                volume_percent: Some(80),
                channel_solo: Some(vec![true]),
                ..MixUpdate::default()
            }
        );
    }

    #[test]
    fn a_mix_request_with_an_unknown_field_is_rejected_instead_of_ignored() {
        let error = serde_json::from_str::<MixUpdateRequest>(r#"{"volume":80}"#)
            .expect_err("a misspelt field must not be a silent no-op");

        assert!(error.to_string().contains("volume"), "names it: {error}");
    }

    #[test]
    fn input_device_reports_its_name_and_channel_count() {
        let device = InputDevice {
            name: "Interface".to_owned(),
            channels: 6,
        };

        let dto = InputDeviceDto::from(&device);

        assert_eq!(dto.name, "Interface");
        assert_eq!(dto.channels, 6);
    }

    #[test]
    fn a_channel_of_an_invalid_group_is_not_labelled_with_it() {
        let status = status_with(
            vec![GroupStatus {
                name: "Drums".to_owned(),
                channels: vec![0, 1],
                invalid: true,
            }],
            vec![0, 0],
        );

        let dto = EngineStatusDto::from(&status);

        assert_eq!(dto.channels[0].group, None);
        assert!(dto.groups[0].invalid);
    }
}
