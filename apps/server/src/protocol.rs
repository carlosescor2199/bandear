use serde::{Deserialize, Deserializer, Serialize};

use crate::mix::{MixUpdate, MixValues};

const PACKET_MAGIC: [u8; 4] = *b"PMON";
const PACKET_VERSION: u8 = 1;
const HEADER_SIZE: usize = 4 + 1 + 1 + 4 + 8 + 2;

pub(crate) struct AudioPacket {
    pub(crate) channels: u8,
    pub(crate) sample_rate: u32,
    pub(crate) sequence: u64,
    pub(crate) samples: Vec<i16>,
}

#[derive(Deserialize)]
pub(crate) struct MixCommand {
    #[serde(rename = "type")]
    pub(crate) message_type: String,
    pub(crate) volume_percent: i32,
    pub(crate) max_level_percent: i32,
    pub(crate) muted: bool,
    #[serde(default)]
    pub(crate) channels: Option<Vec<i32>>,
    #[serde(default)]
    pub(crate) pans: Option<Vec<i32>>,
    #[serde(default)]
    pub(crate) mutes: Option<Vec<bool>>,
    #[serde(default)]
    pub(crate) solos: Option<Vec<bool>>,
    #[serde(default)]
    pub(crate) group_levels: Option<Vec<i32>>,
}

/// Parses a `mix` control message into the engine's partial update.
///
/// Lives here rather than in `mix` so the mix module stays free of wire-format knowledge: this is
/// the mapping between the two, and the dependency then only runs one way. The master values are
/// required on the wire, so they are always present; the lists stay absent when not sent.
pub(crate) fn parse_mix_update(json: &str) -> Result<MixUpdate, String> {
    let command: MixCommand = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if command.message_type != "mix" {
        return Err(format!(
            "unsupported control message type: {}",
            command.message_type
        ));
    }

    Ok(MixUpdate {
        channel_gains: command.channels,
        pans: command.pans,
        channel_muted: command.mutes,
        channel_solo: command.solos,
        group_levels: command.group_levels,
        volume_percent: Some(command.volume_percent),
        max_level_percent: Some(command.max_level_percent),
        muted: Some(command.muted),
    })
}

/// The number a phone gave its `mix`, so the answer to it can carry the number back. Newer phones
/// number their mixes in order, which lets them tell the answer to their newest mix from the
/// answers to older ones still on their way. Read leniently: anything but an unsigned integer, or
/// a message that is not JSON, is read as no number, and the answer then carries none.
pub(crate) fn mix_sequence(json: &str) -> Option<u64> {
    serde_json::from_str::<MixSequence>(json)
        .ok()
        .and_then(|numbered| numbered.seq)
}

#[derive(Deserialize)]
struct MixSequence {
    #[serde(default, deserialize_with = "lenient_sequence")]
    seq: Option<u64>,
}

fn lenient_sequence<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(serde_json::Value::as_u64))
}

/// Parses a `mix` control message and merges it into `current`, the two steps a connection takes
/// in one call, so the tests can pin the parse and the merge together.
#[cfg(test)]
pub(crate) fn parse_mix_command(json: &str, current: MixValues) -> Result<MixValues, String> {
    parse_mix_update(json)?.merge_into(current)
}

/// The longest musician name the server keeps from a registration.
///
/// A longer name is truncated rather than rejected: a musician who types too much must still get
/// audio, and the cap keeps one client from dictating an unbounded string to every UI.
pub(crate) const MAX_MUSICIAN_NAME_LENGTH: usize = 64;

/// The longest client id the server keeps from a registration. A UUID (36 characters) fits.
pub(crate) const MAX_CLIENT_ID_LENGTH: usize = 64;

/// A `register` control message: the musician's name, the UDP port the client listens on and,
/// from newer phones, the client id that phone generated once and keeps.
///
/// There is deliberately no address field. The server takes the address from the socket, so a
/// client cannot claim to be someone else's address even if it includes one in the message.
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterCommand {
    #[serde(rename = "type")]
    pub(crate) message_type: String,
    pub(crate) name: String,
    pub(crate) udp_port: u16,
    /// The phone's stable id, or `None` when it sent none or one the server does not keep.
    #[serde(default, deserialize_with = "lenient_client_id")]
    pub(crate) client_id: Option<String>,
}

/// Returns `raw` when it is a client id the server keeps: 1 to [MAX_CLIENT_ID_LENGTH] ASCII
/// letters, digits and hyphens. Anything else is `None`, exactly as if no id had been sent.
fn valid_client_id(raw: &str) -> Option<String> {
    let valid = (1..=MAX_CLIENT_ID_LENGTH).contains(&raw.len())
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    valid.then(|| raw.to_owned())
}

/// Reads `client_id` without ever failing the registration: a value of any other JSON type, or a
/// string outside [valid_client_id], is read as no id. A musician must not lose their audio
/// because a phone sent an id the server cannot use; they only lose the row-following it buys.
fn lenient_client_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .and_then(valid_client_id))
}

/// Parses a `register` control message. Errors name what is wrong so a client (or a person
/// debugging with a socket) can tell why the registration was not accepted.
pub(crate) fn parse_register_command(json: &str) -> Result<RegisterCommand, String> {
    let command: RegisterCommand = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if command.message_type != "register" {
        return Err(format!(
            "unsupported control message type: {}",
            command.message_type
        ));
    }
    if command.udp_port == 0 {
        return Err("registration requires a UDP port between 1 and 65535".to_owned());
    }
    Ok(command)
}

/// The longest reporting interval a `stats` message may cover, in milliseconds.
///
/// A phone reports about once a second; an interval of a minute or more would describe a link the
/// engineer can no longer act on, so it is rejected rather than averaged into a misleading rate.
pub(crate) const MAX_STATS_INTERVAL_MS: u32 = 60_000;

/// A `stats` control message: what the phone experienced over the last interval.
///
/// The counts are deltas for that interval, not running totals, so a report stands on its own and a
/// lost report never skews the next one.
#[derive(Debug, Deserialize)]
pub(crate) struct StatsCommand {
    #[serde(rename = "type")]
    pub(crate) message_type: String,
    pub(crate) interval_ms: u32,
    pub(crate) packets_received: u64,
    pub(crate) packets_lost: u64,
    pub(crate) underruns: u64,
    pub(crate) receive_timeouts: u64,
}

/// Parses a `stats` control message, rejecting an interval the rates cannot be normalised by.
pub(crate) fn parse_stats_command(json: &str) -> Result<StatsCommand, String> {
    let command: StatsCommand = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if command.message_type != "stats" {
        return Err(format!(
            "unsupported control message type: {}",
            command.message_type
        ));
    }
    if command.interval_ms == 0 || command.interval_ms > MAX_STATS_INTERVAL_MS {
        return Err(format!(
            "stats interval_ms must be between 1 and {MAX_STATS_INTERVAL_MS}, got {}",
            command.interval_ms
        ));
    }
    Ok(command)
}

/// Normalizes the name a client announced: trimmed, capped, and `None` when nothing is left.
///
/// Truncating instead of rejecting is deliberate: a musician who types a very long name must still
/// get audio, and the cap keeps one client from dictating an unbounded string to every UI.
pub(crate) fn registered_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_MUSICIAN_NAME_LENGTH).collect())
}

/// The envelope every control message shares: just enough to route it to the right parser.
#[derive(Deserialize)]
struct ControlEnvelope {
    #[serde(rename = "type")]
    message_type: String,
}

/// Reads only the `type` of a control message so the connection can route it before committing to
/// a full parse. Invalid JSON is reported the same way the mix parser would report it.
pub(crate) fn control_message_type(json: &str) -> Result<String, String> {
    let envelope: ControlEnvelope =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    Ok(envelope.message_type)
}

/// A whole mix as the server reports it to a phone, in the field names the `mix` command uses.
///
/// Two messages share this shape: `mix_ack` answers the phone's own `mix`, and `mix_state` tells
/// the phone its mix was changed from elsewhere (the desktop, or another socket of the same
/// musician). A phone that predates `mix_state` ignores it, as it ignores any unknown type. An ack
/// carries the number of the `mix` it answers when that mix had one; a push never does.
#[derive(Serialize)]
struct MixReport {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    seq: Option<u64>,
    volume_percent: u8,
    max_level_percent: u8,
    muted: bool,
    channels: Vec<u8>,
    pans: Vec<u8>,
    mutes: Vec<bool>,
    solos: Vec<bool>,
    group_levels: Vec<u8>,
}

/// One group as it appears in the `config` message: its stable id, a name and 0-based source
/// channel indices.
///
/// The id was added after the name and the channels: a phone that predates it ignores it, as it
/// ignores every field it does not know, and keeps matching group levels by position.
#[derive(Serialize)]
pub(crate) struct GroupConfig {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) channels: Vec<usize>,
}

#[derive(Serialize)]
pub(crate) struct ControlConfig {
    #[serde(rename = "type")]
    pub(crate) message_type: &'static str,
    pub(crate) source_channels: u8,
    pub(crate) sample_rate: u32,
    pub(crate) groups: Vec<GroupConfig>,
}

/// An `error` reply. It carries `seq` only when it refuses a numbered `mix`, so a phone can tell
/// which of its mixes was refused; an error answering anything else never carries one.
#[derive(Serialize)]
struct ControlError {
    #[serde(rename = "type")]
    message_type: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    seq: Option<u64>,
}

// Packet layout: magic[4], version[1], channels[1], sample_rate[4 LE],
// sequence[8 LE], sample_count[2 LE], then sample_count PCM16 samples in LE.
pub(crate) fn serialize_packet(packet: &AudioPacket) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_SIZE + packet.samples.len() * 2);
    bytes.extend_from_slice(&PACKET_MAGIC);
    bytes.push(PACKET_VERSION);
    bytes.push(packet.channels);
    bytes.extend_from_slice(&packet.sample_rate.to_le_bytes());
    bytes.extend_from_slice(&packet.sequence.to_le_bytes());
    bytes.extend_from_slice(&(packet.samples.len() as u16).to_le_bytes());
    for sample in &packet.samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// The `mix_ack` message: the whole mix now applied, answering a phone's `mix`, with that mix's
/// number when it had one (see [mix_sequence]).
pub(crate) fn mix_ack(values: MixValues, seq: Option<u64>) -> Result<String, serde_json::Error> {
    mix_report("mix_ack", values, seq)
}

/// The `mix_state` message: the musician's whole current mix, pushed when it changed without the
/// phone asking, so the phone's screen can follow without sending the change back.
pub(crate) fn mix_state_message(values: MixValues) -> Result<String, serde_json::Error> {
    mix_report("mix_state", values, None)
}

fn mix_report(
    message_type: &'static str,
    values: MixValues,
    seq: Option<u64>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MixReport {
        message_type,
        seq,
        volume_percent: values.volume_percent,
        max_level_percent: values.max_level_percent,
        muted: values.muted,
        channels: values.channel_gains.to_vec(),
        pans: values.pans.to_vec(),
        mutes: values.channel_muted.to_vec(),
        solos: values.channel_solo.to_vec(),
        group_levels: values.group_levels.to_vec(),
    })
}

/// An `error` answering anything but a `mix`; it never carries a number.
pub(crate) fn control_error(message: String) -> Result<String, serde_json::Error> {
    error_reply(message, None)
}

/// The `error` that refuses a phone's `mix`, with that mix's number when it had one.
pub(crate) fn mix_error(message: String, seq: Option<u64>) -> Result<String, serde_json::Error> {
    error_reply(message, seq)
}

fn error_reply(message: String, seq: Option<u64>) -> Result<String, serde_json::Error> {
    serde_json::to_string(&ControlError {
        message_type: "error",
        message,
        seq,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::{MixValues, MAX_GROUPS, MAX_MIX_CHANNELS};

    #[test]
    fn serializes_documented_header_and_pcm_payload() {
        let packet = AudioPacket {
            channels: 2,
            sample_rate: 48_000,
            sequence: 9,
            samples: vec![-1, 0x1234],
        };

        assert_eq!(
            serialize_packet(&packet),
            vec![
                b'P', b'M', b'O', b'N', 1, 2, 0x80, 0xbb, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0xff,
                0xff, 0x34, 0x12,
            ]
        );
    }

    #[test]
    fn mix_ack_reports_the_applied_channel_gains() {
        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"channels":[100,40]}"#,
            MixValues::default(),
        )
        .unwrap();

        let ack = mix_ack(values, None).unwrap();

        assert!(ack.contains(r#""type":"mix_ack""#));
        assert!(ack.contains(r#""channels":[100,40,100"#));
    }

    #[test]
    fn mix_ack_echoes_the_group_levels() {
        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"channels":[100,40],"group_levels":[80]}"#,
            MixValues::default(),
        )
        .unwrap();

        let ack = mix_ack(values, None).unwrap();

        assert!(ack.contains(r#""group_levels":[80,"#));
    }

    #[test]
    fn a_mix_sequence_is_read_only_as_an_unsigned_integer() {
        assert_eq!(
            mix_sequence(r#"{"type":"mix","seq":7,"volume_percent":80}"#),
            Some(7)
        );
        assert_eq!(mix_sequence(r#"{"type":"mix","volume_percent":80}"#), None);
        for odd in [
            r#"{"type":"mix","seq":-1}"#,
            r#"{"type":"mix","seq":"7"}"#,
            r#"{"type":"mix","seq":1.5}"#,
            r#"{"type":"mix","seq":null}"#,
            "not json",
        ] {
            assert_eq!(mix_sequence(odd), None, "{odd}");
        }
    }

    #[test]
    fn answers_to_a_mix_echo_its_sequence_and_nothing_else_carries_one() {
        let parse = |json: String| serde_json::from_str::<serde_json::Value>(&json).unwrap();

        let ack = parse(mix_ack(MixValues::default(), Some(7)).unwrap());
        let refused = parse(mix_error("too many channels".to_owned(), Some(8)).unwrap());

        assert_eq!(ack["type"], "mix_ack");
        assert_eq!(ack["seq"], 7);
        assert_eq!(refused["type"], "error");
        assert_eq!(refused["message"], "too many channels");
        assert_eq!(refused["seq"], 8);
        for unnumbered in [
            parse(mix_ack(MixValues::default(), None).unwrap()),
            parse(mix_error("too many channels".to_owned(), None).unwrap()),
            parse(control_error("no UDP target configured for client IP".to_owned()).unwrap()),
            parse(mix_state_message(MixValues::default()).unwrap()),
        ] {
            assert!(unnumbered.get("seq").is_none(), "{unnumbered}");
        }
    }

    #[test]
    fn mix_state_carries_the_full_mix_under_its_own_type() {
        let mut values = MixValues {
            volume_percent: 42,
            max_level_percent: 90,
            muted: true,
            ..MixValues::default()
        };
        values.channel_gains[1] = 30;
        values.pans[0] = 50;
        values.channel_muted[2] = true;
        values.channel_solo[3] = true;
        values.group_levels[0] = 70;

        let json: serde_json::Value =
            serde_json::from_str(&mix_state_message(values).unwrap()).unwrap();

        assert_eq!(json["type"], "mix_state");
        assert_eq!(json["volume_percent"], 42);
        assert_eq!(json["max_level_percent"], 90);
        assert_eq!(json["muted"], true);
        assert_eq!(json["channels"].as_array().unwrap().len(), MAX_MIX_CHANNELS);
        assert_eq!(json["channels"][1], 30);
        assert_eq!(json["pans"][0], 50);
        assert_eq!(json["pans"][1], 100);
        assert_eq!(json["mutes"][2], true);
        assert_eq!(json["solos"][3], true);
        assert_eq!(json["group_levels"].as_array().unwrap().len(), MAX_GROUPS);
        assert_eq!(json["group_levels"][0], 70);
    }

    #[test]
    fn a_mix_command_becomes_an_update_that_leaves_absent_lists_alone() {
        let update = parse_mix_update(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"pans":[10]}"#,
        )
        .unwrap();

        assert_eq!(update.volume_percent, Some(80));
        assert_eq!(update.max_level_percent, Some(90));
        assert_eq!(update.muted, Some(false));
        assert_eq!(update.pans, Some(vec![10]));
        assert_eq!(update.channel_gains, None);
        assert_eq!(update.channel_muted, None);
        assert_eq!(update.channel_solo, None);
        assert_eq!(update.group_levels, None);
    }

    #[test]
    fn parses_a_registration_with_the_name_and_udp_port() {
        let command =
            parse_register_command(r#"{"type":"register","name":"Ana","udp_port":50000}"#)
                .expect("a registration should parse");

        assert_eq!(command.name, "Ana");
        assert_eq!(command.udp_port, 50000);
    }

    #[test]
    fn a_registration_without_a_udp_port_is_rejected() {
        let error = parse_register_command(r#"{"type":"register","name":"Ana"}"#)
            .expect_err("a registration without a port must be rejected");

        assert!(
            error.contains("udp_port"),
            "message names the field: {error}"
        );
    }

    #[test]
    fn a_registration_with_port_zero_is_rejected() {
        let error = parse_register_command(r#"{"type":"register","name":"Ana","udp_port":0}"#)
            .expect_err("port zero is not listenable");

        assert!(error.contains("UDP port"), "message explains: {error}");
    }

    #[test]
    fn a_message_of_another_type_is_not_a_registration() {
        let error = parse_register_command(r#"{"type":"mix","name":"Ana","udp_port":50000}"#)
            .expect_err("a mix is not a registration");

        assert!(error.contains("mix"), "message names the type: {error}");
    }

    #[test]
    fn an_address_in_a_registration_is_ignored_because_there_is_no_such_field() {
        let command = parse_register_command(
            r#"{"type":"register","name":"Mallory","udp_port":50000,"address":"10.0.0.99:50000","ip":"10.0.0.99"}"#,
        )
        .expect("unknown fields must not break the parse");

        assert_eq!(command.name, "Mallory");
        assert_eq!(command.udp_port, 50000);
    }

    #[test]
    fn parses_a_registration_with_a_client_id() {
        let command = parse_register_command(
            r#"{"type":"register","name":"Ana","udp_port":50000,"client_id":"0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b"}"#,
        )
        .expect("a registration with a client id should parse");

        assert_eq!(command.name, "Ana");
        assert_eq!(command.udp_port, 50000);
        assert_eq!(
            command.client_id.as_deref(),
            Some("0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b")
        );
    }

    #[test]
    fn a_registration_without_a_client_id_parses_as_before() {
        let command =
            parse_register_command(r#"{"type":"register","name":"Ana","udp_port":50000}"#)
                .expect("an older phone sends no client id");

        assert_eq!(command.udp_port, 50000);
        assert_eq!(command.client_id, None);
    }

    #[test]
    fn a_client_id_of_up_to_64_letters_digits_and_hyphens_is_kept() {
        let longest = "A-9".repeat(21) + "z";
        assert_eq!(longest.len(), MAX_CLIENT_ID_LENGTH);

        for id in ["a", "phone-1", "ABC-def-123", longest.as_str()] {
            let command = parse_register_command(&format!(
                r#"{{"type":"register","name":"Ana","udp_port":50000,"client_id":"{id}"}}"#
            ))
            .expect("a valid id should parse");

            assert_eq!(command.client_id.as_deref(), Some(id));
        }
    }

    /// A malformed id must never cost the musician their audio: the registration goes through as
    /// if the phone had sent no id at all.
    #[test]
    fn an_invalid_client_id_is_ignored_and_the_registration_still_parses() {
        let too_long = format!(r#""{}""#, "a".repeat(MAX_CLIENT_ID_LENGTH + 1));
        let invalid = [
            r#""""#,
            r#""has space""#,
            r#""under_score""#,
            r#""slash/id""#,
            r#""acentuación""#,
            too_long.as_str(),
            "42",
            "true",
            "null",
            r#"["a"]"#,
            r#"{"id":"a"}"#,
        ];

        for client_id in invalid {
            let command = parse_register_command(&format!(
                r#"{{"type":"register","name":"Ana","udp_port":50000,"client_id":{client_id}}}"#
            ))
            .unwrap_or_else(|error| panic!("client_id {client_id} broke the parse: {error}"));

            assert_eq!(command.client_id, None, "client_id {client_id} is ignored");
            assert_eq!(command.udp_port, 50000);
        }
    }

    #[test]
    fn a_musician_name_is_trimmed_and_capped() {
        assert_eq!(registered_name("  Ana  ").as_deref(), Some("Ana"));
        assert_eq!(registered_name("   "), None);
        assert_eq!(registered_name(""), None);

        let long = "a".repeat(MAX_MUSICIAN_NAME_LENGTH + 10);
        let name = registered_name(&long).expect("a long name still registers");

        assert_eq!(name.chars().count(), MAX_MUSICIAN_NAME_LENGTH);
    }

    #[test]
    fn parses_a_stats_report_with_its_interval_and_counts() {
        let command = parse_stats_command(
            r#"{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":3,"underruns":1,"receive_timeouts":2,"battery":80}"#,
        )
        .expect("a stats report should parse, ignoring unknown fields");

        assert_eq!(command.interval_ms, 1_000);
        assert_eq!(command.packets_received, 200);
        assert_eq!(command.packets_lost, 3);
        assert_eq!(command.underruns, 1);
        assert_eq!(command.receive_timeouts, 2);
    }

    #[test]
    fn a_stats_report_with_a_zero_interval_is_rejected() {
        let error = parse_stats_command(
            r#"{"type":"stats","interval_ms":0,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0}"#,
        )
        .expect_err("a zero interval cannot normalise a rate");

        assert!(
            error.contains("interval_ms"),
            "message names the field: {error}"
        );
    }

    #[test]
    fn a_stats_interval_is_accepted_up_to_a_minute_and_rejected_above_it() {
        let at_limit = format!(
            r#"{{"type":"stats","interval_ms":{MAX_STATS_INTERVAL_MS},"packets_received":0,"packets_lost":0,"underruns":0,"receive_timeouts":0}}"#
        );
        let above_limit = format!(
            r#"{{"type":"stats","interval_ms":{},"packets_received":0,"packets_lost":0,"underruns":0,"receive_timeouts":0}}"#,
            MAX_STATS_INTERVAL_MS + 1
        );

        assert!(parse_stats_command(&at_limit).is_ok());
        let error = parse_stats_command(&above_limit).expect_err("an interval above a minute");
        assert!(
            error.contains("interval_ms"),
            "message names the field: {error}"
        );
    }

    #[test]
    fn a_stats_report_missing_a_count_is_rejected() {
        let error = parse_stats_command(
            r#"{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":0,"receive_timeouts":0}"#,
        )
        .expect_err("a report without underruns is incomplete");

        assert!(
            error.contains("underruns"),
            "message names the field: {error}"
        );
    }

    #[test]
    fn a_stats_report_with_a_negative_count_is_rejected() {
        assert!(parse_stats_command(
            r#"{"type":"stats","interval_ms":1000,"packets_received":-1,"packets_lost":0,"underruns":0,"receive_timeouts":0}"#,
        )
        .is_err());
    }

    #[test]
    fn a_message_of_another_type_is_not_a_stats_report() {
        let error = parse_stats_command(
            r#"{"type":"mix","interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0}"#,
        )
        .expect_err("a mix is not a stats report");

        assert!(error.contains("mix"), "message names the type: {error}");
    }

    #[test]
    fn config_carries_groups_with_their_ids_and_zero_based_channels() {
        let config = ControlConfig {
            message_type: "config",
            source_channels: 2,
            sample_rate: 48_000,
            groups: vec![GroupConfig {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![0, 1],
            }],
        };

        let json = serde_json::to_string(&config).unwrap();

        assert!(json.contains(r#""groups":[{"id":"kit","name":"Drums","channels":[0,1]}]"#));
    }
}
