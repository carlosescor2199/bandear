use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::net::{SocketAddr, ToSocketAddrs};

use crate::capture::AudioHost;
use crate::mix::{Group, GroupLayout, MAX_GROUPS};

const DEFAULT_TARGET: &str = "127.0.0.1:50000";
const DEFAULT_CONTROL_PORT: u16 = 50001;
/// Bounds for `--buffer`: below a plausible driver period, up to one second at 48 kHz. A value
/// outside them is a typo rather than a tuning choice.
const MIN_BUFFER_FRAMES: u32 = 16;
const MAX_BUFFER_FRAMES: u32 = 48_000;

/// The engine's startup configuration, independent of how it was gathered.
///
/// The command line fills this in [parse_arguments], but a UI can build one directly and hand it to
/// [crate::start] without parsing anything.
///
/// `buffer_frames` is the input buffer size in frames; `None` leaves the device default, which is
/// what every caller used before the option existed. It is the biggest single lever on capture
/// latency, because the engine packages exactly one packet per input callback, so the callback size
/// is also the packet interval.
#[derive(Debug)]
pub struct EngineConfig {
    pub device_filter: Option<String>,
    pub targets: Vec<String>,
    pub control_port: u16,
    pub groups: Vec<GroupDefinition>,
    pub buffer_frames: Option<u32>,
    /// The capture backend. `Auto` prefers ASIO where it is available; the window lets the engineer
    /// pick a concrete one.
    pub host: AudioHost,
}

/// The crate-internal name the parsing tests use for [EngineConfig].
#[cfg(test)]
pub(crate) type Arguments = EngineConfig;

/// A group as the engineer defined it: a stable id, a name and 1-based channel numbers.
///
/// The id is what a group is known by while the engine runs: when the groups are edited, every
/// musician's group level follows its group by id, so a group can be renamed, given other channels
/// or moved in the list without its level moving to another group. Groups from the command line
/// are numbered by their order ("1", "2", ...); a UI supplies its own ids.
///
/// The conversion to the protocol's 0-based indices needs the captured channel count, so it happens
/// in [validate_groups] once the input device has been selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupDefinition {
    pub id: String,
    pub name: String,
    pub channels: Vec<usize>,
}

pub fn parse_arguments() -> Result<EngineConfig, Box<dyn Error>> {
    parse_from(env::args().skip(1))
}

fn parse_from(arguments: impl Iterator<Item = String>) -> Result<EngineConfig, Box<dyn Error>> {
    let mut arguments = arguments;
    let mut device_filter = None;
    let mut targets = Vec::new();
    let mut control_port = DEFAULT_CONTROL_PORT;
    let mut groups = Vec::new();
    let mut buffer_frames = None;
    let mut host = AudioHost::Auto;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--device" => {
                device_filter = Some(
                    arguments
                        .next()
                        .ok_or("--device requires a search string")?,
                );
            }
            "--target" => {
                targets.push(
                    arguments
                        .next()
                        .ok_or("--target requires a host:port value")?,
                );
            }
            "--group" => {
                let value = arguments
                    .next()
                    .ok_or("--group requires a Name=1,2,3 value")?;
                // Numbered by order: the first --group is "1", the next "2", and so on.
                groups.push(parse_group(&value, groups.len() + 1)?);
            }
            "--control-port" => {
                let port = arguments
                    .next()
                    .ok_or("--control-port requires a port")?
                    .parse()
                    .map_err(|_| "--control-port must be a valid TCP port")?;
                if port == 0 {
                    return Err("--control-port must be between 1 and 65535".into());
                }
                control_port = port;
            }
            "--buffer" => {
                let value = arguments.next().ok_or("--buffer requires a frame count")?;
                buffer_frames = Some(parse_buffer_frames(&value)?);
            }
            "--host" => {
                let value = arguments
                    .next()
                    .ok_or("--host requires auto, asio or wasapi")?;
                host = AudioHost::parse(&value)?;
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }

    if targets.is_empty() {
        targets.push(DEFAULT_TARGET.to_owned());
    }

    Ok(EngineConfig {
        device_filter,
        targets,
        control_port,
        groups,
        buffer_frames,
        host,
    })
}

/// Parses one `Name=1,2,3` group value, the `number`th on the command line, which is also its id.
/// Every error names the offending value, so a typo is easy to spot on a command line.
fn parse_group(value: &str, number: usize) -> Result<GroupDefinition, Box<dyn Error>> {
    let (name, channels) = value
        .split_once('=')
        .ok_or_else(|| format!("--group `{value}` must be written as Name=1,2,3"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("--group `{value}` has an empty name").into());
    }
    if channels.trim().is_empty() {
        return Err(format!("--group `{value}` has an empty channel list").into());
    }

    let mut parsed = Vec::new();
    for channel in channels.split(',') {
        let channel = channel.trim();
        let number = channel
            .parse()
            .map_err(|_| format!("--group `{value}` has a non-numeric channel `{channel}`"))?;
        parsed.push(number);
    }

    Ok(GroupDefinition {
        id: number.to_string(),
        name: name.to_owned(),
        channels: parsed,
    })
}

/// Parses one `--buffer` frame count.
///
/// The range is validated here rather than at the audio device, so a typo fails at startup with the
/// offending value named instead of silently falling back to the device default later.
fn parse_buffer_frames(value: &str) -> Result<u32, Box<dyn Error>> {
    let frames: u32 = value
        .parse()
        .map_err(|_| format!("--buffer `{value}` must be a whole number of frames"))?;
    if !(MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(&frames) {
        return Err(format!(
            "--buffer `{value}` is outside the supported range {MIN_BUFFER_FRAMES}..={MAX_BUFFER_FRAMES} frames"
        )
        .into());
    }
    Ok(frames)
}

/// Checks what makes a set of groups unusable on any device: more than [MAX_GROUPS] groups, an
/// empty or repeated id, an empty name, or a channel listed in two groups (or twice in one).
///
/// Channel numbers are not compared with a device here. At start-up [validate_groups] rejects one
/// the device lacks; a running engine keeps such a group but marks it invalid, as after a device
/// switch, so the engineer's definition survives until the channel is back.
pub(crate) fn check_groups(definitions: &[GroupDefinition]) -> Result<(), String> {
    if definitions.len() > MAX_GROUPS {
        return Err(format!(
            "at most {MAX_GROUPS} groups are supported, received {}",
            definitions.len()
        ));
    }
    let mut ids = HashSet::with_capacity(definitions.len());
    let mut owners: HashMap<usize, &str> = HashMap::new();
    for definition in definitions {
        if definition.id.trim().is_empty() {
            return Err(format!("group \"{}\" has an empty id", definition.name));
        }
        if !ids.insert(definition.id.as_str()) {
            return Err(format!(
                "group id \"{}\" is used by more than one group",
                definition.id
            ));
        }
        if definition.name.trim().is_empty() {
            return Err(format!("group \"{}\" has an empty name", definition.id));
        }
        for &channel in &definition.channels {
            if let Some(previous) = owners.insert(channel, &definition.name) {
                return Err(format!(
                    "channel {channel} is listed in both \"{previous}\" and \"{}\"",
                    definition.name
                ));
            }
        }
    }
    Ok(())
}

/// Validates the engineer's groups against the captured channel count and converts the 1-based
/// command-line channels into the 0-based indices the protocol and mixer use.
///
/// This runs after the input device is selected because the channel count is only known then. A
/// group that cannot be validated is a startup error, not a silent no-op.
pub(crate) fn validate_groups(
    definitions: &[GroupDefinition],
    channel_count: usize,
) -> Result<GroupLayout, Box<dyn Error>> {
    check_groups(definitions)?;

    let mut groups = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let mut channels = Vec::with_capacity(definition.channels.len());
        for &channel in &definition.channels {
            if channel == 0 || channel > channel_count {
                return Err(format!(
                    "group \"{}\" names channel {channel}, outside the captured range of 1..={channel_count}",
                    definition.name
                )
                .into());
            }
            channels.push(channel - 1);
        }
        groups.push(Group {
            id: definition.id.clone(),
            name: definition.name.clone(),
            channels,
        });
    }

    Ok(GroupLayout::new(groups))
}

pub(crate) fn resolve_targets(targets: &[String]) -> Result<Vec<SocketAddr>, Box<dyn Error>> {
    let resolved = targets
        .iter()
        .map(|target| {
            target
                .to_socket_addrs()
                .map_err(|error| format!("invalid --target `{target}`: {error}"))?
                .next()
                .ok_or_else(|| format!("invalid --target `{target}`: it resolved to no address"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| -> Box<dyn Error> { error.into() })?;

    let mut seen_ips = HashMap::new();
    for target in &resolved {
        if let Some(previous) = seen_ips.insert(target.ip(), *target) {
            return Err(format!(
                "duplicate UDP target IP {} is not supported (targets {} and {})",
                target.ip(),
                previous,
                target
            )
            .into());
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_and_target_arguments() {
        let arguments = parse_arguments_from(["--device", "Volt", "--target", "localhost:6000"])
            .expect("arguments should parse");

        assert_eq!(arguments.device_filter.as_deref(), Some("Volt"));
        assert_eq!(arguments.targets, vec!["localhost:6000"]);
        assert_eq!(arguments.control_port, DEFAULT_CONTROL_PORT);
    }

    #[test]
    fn parses_repeated_target_arguments() {
        let arguments = parse_arguments_from([
            "--target",
            "192.168.1.3:50000",
            "--target",
            "192.168.1.4:50000",
        ])
        .expect("arguments should parse");

        assert_eq!(
            arguments.targets,
            vec!["192.168.1.3:50000", "192.168.1.4:50000"]
        );
    }

    #[test]
    fn defaults_to_one_target_when_not_configured() {
        let arguments = parse_arguments_from::<0>([]).expect("arguments should parse");

        assert_eq!(arguments.targets, vec![DEFAULT_TARGET]);
    }

    #[test]
    fn parses_an_explicit_buffer_size() {
        let arguments = parse_arguments_from(["--buffer", "144"]).expect("arguments should parse");

        assert_eq!(arguments.buffer_frames, Some(144));
    }

    #[test]
    fn defaults_to_the_device_buffer_when_not_given() {
        let arguments = parse_arguments_from::<0>([]).expect("arguments should parse");

        assert_eq!(arguments.buffer_frames, None);
    }

    #[test]
    fn rejects_a_buffer_without_a_value_naming_the_flag() {
        let error = parse_arguments_from(["--buffer"]).unwrap_err();

        assert!(error.to_string().contains("--buffer"));
    }

    #[test]
    fn rejects_a_non_numeric_buffer_naming_the_value() {
        let error = parse_arguments_from(["--buffer", "small"]).unwrap_err();

        assert!(error.to_string().contains("small"));
    }

    #[test]
    fn rejects_a_buffer_outside_the_supported_range_naming_the_value() {
        let error = parse_arguments_from(["--buffer", "4"]).unwrap_err();

        assert!(error.to_string().contains('4'));
    }

    #[test]
    fn defaults_to_the_automatic_host() {
        let arguments = parse_arguments_from::<0>([]).expect("arguments should parse");

        assert_eq!(arguments.host, AudioHost::Auto);
    }

    #[test]
    fn parses_a_concrete_host_case_insensitively() {
        let arguments = parse_arguments_from(["--host", "ASIO"]).expect("arguments should parse");

        assert_eq!(arguments.host, AudioHost::Asio);
    }

    #[test]
    fn rejects_an_unknown_host_naming_the_value() {
        let error = parse_arguments_from(["--host", "coreaudio"]).unwrap_err();

        assert!(error.to_string().contains("coreaudio"));
    }

    #[test]
    fn rejects_targets_with_the_same_ip() {
        let targets = vec![
            "192.168.1.3:50000".to_owned(),
            "192.168.1.3:50001".to_owned(),
        ];

        let error = resolve_targets(&targets).unwrap_err();

        assert!(error
            .to_string()
            .contains("duplicate UDP target IP 192.168.1.3"));
    }

    #[test]
    fn parses_repeated_group_arguments_in_order() {
        let arguments = parse_arguments_from(["--group", "Drums=1,2,3", "--group", "Vocals=4,5"])
            .expect("arguments should parse");

        assert_eq!(arguments.groups.len(), 2);
        assert_eq!(arguments.groups[0].name, "Drums");
        assert_eq!(arguments.groups[0].channels, vec![1, 2, 3]);
        assert_eq!(arguments.groups[1].name, "Vocals");
        assert_eq!(arguments.groups[1].channels, vec![4, 5]);
    }

    #[test]
    fn command_line_groups_take_their_order_as_their_id() {
        let arguments = parse_arguments_from(["--group", "Drums=1,2,3", "--group", "Vocals=4,5"])
            .expect("arguments should parse");

        assert_eq!(arguments.groups[0].id, "1");
        assert_eq!(arguments.groups[1].id, "2");
    }

    fn group(id: &str, name: &str, channels: &[usize]) -> GroupDefinition {
        GroupDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            channels: channels.to_vec(),
        }
    }

    #[test]
    fn groups_need_distinct_non_empty_ids_and_names() {
        let repeated = check_groups(&[group("a", "Drums", &[1]), group("a", "Vocals", &[2])])
            .expect_err("two groups cannot share an id");
        let no_id = check_groups(&[group(" ", "Drums", &[1])]).expect_err("an id is needed");
        let no_name = check_groups(&[group("a", "", &[1])]).expect_err("a name is needed");

        assert!(repeated.contains("\"a\""), "names the id: {repeated}");
        assert!(no_id.contains("Drums"), "names the group: {no_id}");
        assert!(no_name.contains("\"a\""), "names the group's id: {no_name}");
        assert_eq!(
            check_groups(&[group("a", "Drums", &[1]), group("b", "Vocals", &[2])]),
            Ok(())
        );
    }

    #[test]
    fn a_channel_belongs_to_one_group_even_beyond_the_device() {
        let error = check_groups(&[group("a", "Drums", &[9]), group("b", "Vocals", &[9])])
            .expect_err("channel 9 cannot be in two groups");

        assert!(error.contains("Drums") && error.contains("Vocals") && error.contains('9'));
        assert_eq!(
            check_groups(&[group("a", "Drums", &[9])]),
            Ok(()),
            "a channel the device lacks is not this check's concern"
        );
    }

    #[test]
    fn validated_groups_keep_their_ids() {
        let layout =
            validate_groups(&[group("kit", "Drums", &[1, 2])], 2).expect("groups should validate");

        assert_eq!(layout.groups()[0].id, "kit");
    }

    #[test]
    fn rejects_group_without_an_equals_sign() {
        let error = parse_arguments_from(["--group", "Drums1,2"]).unwrap_err();

        assert!(error.to_string().contains("Drums1,2"));
    }

    #[test]
    fn rejects_group_with_an_empty_name() {
        let error = parse_arguments_from(["--group", "=1,2"]).unwrap_err();

        assert!(error.to_string().contains("empty name"));
    }

    #[test]
    fn rejects_group_with_an_empty_channel_list() {
        let error = parse_arguments_from(["--group", "Drums="]).unwrap_err();

        assert!(error.to_string().contains("empty channel"));
    }

    #[test]
    fn rejects_group_with_a_non_numeric_channel_naming_the_value() {
        let error = parse_arguments_from(["--group", "Drums=1,kick"]).unwrap_err();

        assert!(error.to_string().contains("kick"));
    }

    #[test]
    fn validates_group_channels_against_the_captured_count() {
        let definitions = vec![group("1", "Drums", &[1, 7])];

        let error = validate_groups(&definitions, 2).unwrap_err();

        assert!(error.to_string().contains("Drums"));
        assert!(error.to_string().contains('7'));
    }

    #[test]
    fn rejects_a_channel_listed_in_two_groups_naming_both() {
        let definitions = vec![group("1", "Drums", &[1, 2]), group("2", "Vocals", &[2])];

        let error = validate_groups(&definitions, 2).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("Drums"));
        assert!(message.contains("Vocals"));
        assert!(message.contains('2'));
    }

    #[test]
    fn converts_validated_groups_to_zero_based_protocol_channels() {
        let definitions = vec![group("1", "Drums", &[1, 2])];

        let layout = validate_groups(&definitions, 2).expect("groups should validate");

        assert_eq!(layout.groups().len(), 1);
        assert_eq!(layout.groups()[0].name, "Drums");
        assert_eq!(layout.groups()[0].channels, vec![0, 1]);
    }

    #[test]
    fn rejects_more_groups_than_the_supported_maximum() {
        let definitions = (0..MAX_GROUPS + 1)
            .map(|index| GroupDefinition {
                id: index.to_string(),
                name: format!("Group {index}"),
                channels: vec![index + 1],
            })
            .collect::<Vec<_>>();

        let error = validate_groups(&definitions, 2).unwrap_err();

        assert!(error.to_string().contains(&MAX_GROUPS.to_string()));
    }

    fn parse_arguments_from<const N: usize>(
        values: [&str; N],
    ) -> Result<Arguments, Box<dyn Error>> {
        parse_from(values.into_iter().map(str::to_owned))
    }
}
