//! I/O-free bridge core: turns serial lines and port events into MQTT actions.

use crate::payload::state_payload;
use crate::protocol::{Hello, Line, Measurement, ParseError, parse_line};
use std::fmt;
use std::time::SystemTime;
use tracing::{debug, info, warn};

pub const ONLINE: &[u8] = b"online";
pub const OFFLINE: &[u8] = b"offline";
const EXCERPT_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qos {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub topic: String,
    pub payload: Vec<u8>,
    pub retain: bool,
    pub qos: Qos,
}

#[derive(Debug)]
pub struct PublishError(pub String);

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Publisher {
    /// Starts a broker session; reconnects after broker outages are the publisher's job.
    fn connect(&mut self, client_id: &str, will: Message);
    fn publish(&mut self, message: Message) -> Result<(), PublishError>;
    /// Ends the session cleanly (no last will), flushing what was queued before.
    fn disconnect(&mut self);
}

pub trait Clock {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub struct Bridge<P, C> {
    publisher: P,
    clock: C,
    topic_prefix: String,
    /// Device the MQTT session belongs to (set once connected).
    device_id: Option<String>,
    online: bool,
    last_hello: Option<Hello>,
}

impl<P: Publisher, C: Clock> Bridge<P, C> {
    pub fn new(publisher: P, clock: C, topic_prefix: impl Into<String>) -> Self {
        Bridge {
            publisher,
            clock,
            topic_prefix: topic_prefix.into(),
            device_id: None,
            online: false,
            last_hello: None,
        }
    }

    pub fn publisher(&self) -> &P {
        &self.publisher
    }

    pub fn handle_line(&mut self, raw: &[u8]) {
        match parse_line(raw) {
            Ok(Line::Hello(hello)) => self.on_hello(hello),
            Ok(Line::Measurement(m)) => self.on_measurement(m),
            Ok(Line::Error(e)) => warn!(
                device_id = e.device_id.as_deref().unwrap_or("unknown"),
                "firmware error: {}", e.message
            ),
            Err(ParseError::Empty) => debug!("ignoring empty line"),
            Err(ParseError::Invalid(reason)) => {
                warn!(line = %excerpt(raw), "ignoring invalid line: {reason}")
            }
        }
    }

    /// Serial port lost: the device is unreachable, the MQTT session stays up.
    pub fn on_port_lost(&mut self) {
        if self.online {
            self.set_availability(false);
        }
    }

    pub fn shutdown(&mut self) {
        if self.device_id.is_some() {
            if self.online {
                self.set_availability(false);
            }
            self.publisher.disconnect();
            self.device_id = None;
        }
    }

    fn on_hello(&mut self, hello: Hello) {
        if self.last_hello.as_ref() == Some(&hello) {
            debug!(device_id = %hello.device_id, "hello");
        } else {
            info!(
                device_id = %hello.device_id,
                product = %hello.product,
                sensor_fw = %hello.sensor_fw,
                firmware = %hello.firmware,
                "device hello"
            );
        }
        self.ensure_online(&hello.device_id);
        self.last_hello = Some(hello);
    }

    fn on_measurement(&mut self, m: Measurement) {
        self.ensure_online(&m.device_id);
        let payload = state_payload(&m, self.clock.now());
        debug!(%payload, "measurement");
        let topic = self.topic(&m.device_id, "state");
        if let Err(e) = self.publisher.publish(Message {
            topic,
            payload: payload.into_bytes(),
            retain: false,
            qos: Qos::AtLeastOnce,
        }) {
            warn!("dropping measurement: {e}");
        }
    }

    fn ensure_online(&mut self, device_id: &str) {
        if self.device_id.as_deref() != Some(device_id) {
            if let Some(old) = self.device_id.take() {
                info!(old = %old, new = %device_id, "device id changed, reconnecting");
                if self.online {
                    self.publish_availability(&old, false);
                }
                self.publisher.disconnect();
            }
            self.online = false;
            info!(%device_id, "connecting to MQTT broker");
            let will = self.availability_message(device_id, false);
            self.publisher
                .connect(&format!("sen66_bridge_{device_id}"), will);
            self.device_id = Some(device_id.to_string());
        }
        if !self.online {
            self.set_availability(true);
        }
    }

    fn set_availability(&mut self, online: bool) {
        if let Some(id) = self.device_id.clone() {
            self.publish_availability(&id, online);
        }
        self.online = online;
    }

    fn publish_availability(&mut self, device_id: &str, online: bool) {
        info!(%device_id, "availability {}", if online { "online" } else { "offline" });
        let msg = self.availability_message(device_id, online);
        if let Err(e) = self.publisher.publish(msg) {
            warn!("failed to publish availability: {e}");
        }
    }

    fn availability_message(&self, device_id: &str, online: bool) -> Message {
        Message {
            topic: self.topic(device_id, "availability"),
            payload: if online { ONLINE } else { OFFLINE }.to_vec(),
            retain: true,
            qos: Qos::AtLeastOnce,
        }
    }

    fn topic(&self, device_id: &str, leaf: &str) -> String {
        format!("{}/{device_id}/{leaf}", self.topic_prefix)
    }
}

/// Short, printable excerpt of an untrusted line for log output.
pub fn excerpt(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out: String = text
        .chars()
        .take(EXCERPT_CHARS)
        .flat_map(char::escape_debug)
        .collect();
    if text.chars().count() > EXCERPT_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
pub mod fakes {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Connect { client_id: String, will: Message },
        Publish(Message),
        Disconnect,
    }

    #[derive(Default)]
    pub struct FakePublisher {
        pub calls: Vec<Call>,
    }

    impl Publisher for FakePublisher {
        fn connect(&mut self, client_id: &str, will: Message) {
            self.calls.push(Call::Connect {
                client_id: client_id.into(),
                will,
            });
        }
        fn publish(&mut self, message: Message) -> Result<(), PublishError> {
            self.calls.push(Call::Publish(message));
            Ok(())
        }
        fn disconnect(&mut self) {
            self.calls.push(Call::Disconnect);
        }
    }

    pub struct FixedClock(pub SystemTime);

    impl Default for FixedClock {
        fn default() -> Self {
            // 2026-09-22T14:03:00.750Z
            FixedClock(UNIX_EPOCH + Duration::from_millis(1_790_085_780_750))
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    pub fn hello(id: &str) -> String {
        format!(
            r#"{{"type":"hello","device_id":"{id}","product":"SEN66","sensor_fw":"4.0","firmware":"v1"}}"#
        )
    }

    pub fn measurement(id: &str) -> String {
        format!(
            r#"{{"type":"measurement","device_id":"{id}","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}}"#
        )
    }

    pub fn state_json(id: &str) -> String {
        format!(
            r#"{{"device_id":"{id}","taken_at":"2026-09-22T14:03:00Z","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}}"#
        )
    }

    fn avail(id: &str, payload: &[u8]) -> Message {
        Message {
            topic: format!("ziwoas/sen66/{id}/availability"),
            payload: payload.to_vec(),
            retain: true,
            qos: Qos::AtLeastOnce,
        }
    }

    pub fn connect(id: &str) -> Call {
        Call::Connect {
            client_id: format!("sen66_bridge_{id}"),
            will: avail(id, OFFLINE),
        }
    }

    pub fn online(id: &str) -> Call {
        Call::Publish(avail(id, ONLINE))
    }

    pub fn offline(id: &str) -> Call {
        Call::Publish(avail(id, OFFLINE))
    }

    pub fn state(id: &str) -> Call {
        Call::Publish(Message {
            topic: format!("ziwoas/sen66/{id}/state"),
            payload: state_json(id).into_bytes(),
            retain: false,
            qos: Qos::AtLeastOnce,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;

    fn bridge() -> Bridge<FakePublisher, FixedClock> {
        Bridge::new(
            FakePublisher::default(),
            FixedClock::default(),
            "ziwoas/sen66",
        )
    }

    fn feed(b: &mut Bridge<FakePublisher, FixedClock>, lines: &[&str]) {
        for line in lines {
            b.handle_line(line.as_bytes());
        }
    }

    #[test]
    fn nothing_happens_before_the_device_id_is_known() {
        let mut b = bridge();
        feed(
            &mut b,
            &[
                "garbage",
                "",
                r#"{"type":"error","device_id":null,"message":"no sensor"}"#,
            ],
        );
        b.on_port_lost();
        b.shutdown();
        assert!(b.publisher().calls.is_empty());
    }

    #[test]
    fn hello_connects_and_goes_online() {
        let mut b = bridge();
        feed(&mut b, &[&hello("A"), &hello("A")]);
        assert_eq!(b.publisher().calls, vec![connect("A"), online("A")]);
    }

    #[test]
    fn measurement_before_hello_connects_goes_online_and_publishes_state() {
        let mut b = bridge();
        feed(&mut b, &[&measurement("A"), &measurement("A")]);
        assert_eq!(
            b.publisher().calls,
            vec![connect("A"), online("A"), state("A"), state("A")]
        );
    }

    #[test]
    fn port_lost_goes_offline_and_next_valid_line_goes_online() {
        let mut b = bridge();
        feed(&mut b, &[&hello("A")]);
        b.on_port_lost();
        b.on_port_lost();
        feed(&mut b, &["garbage after reconnect", &measurement("A")]);
        assert_eq!(
            b.publisher().calls,
            vec![
                connect("A"),
                online("A"),
                offline("A"),
                online("A"),
                state("A")
            ]
        );
    }

    #[test]
    fn device_id_change_sets_old_offline_and_reconnects() {
        let mut b = bridge();
        feed(&mut b, &[&hello("A"), &measurement("B")]);
        assert_eq!(
            b.publisher().calls,
            vec![
                connect("A"),
                online("A"),
                offline("A"),
                Call::Disconnect,
                connect("B"),
                online("B"),
                state("B"),
            ]
        );
    }

    #[test]
    fn device_id_change_while_port_was_lost() {
        let mut b = bridge();
        feed(&mut b, &[&hello("A")]);
        b.on_port_lost();
        feed(&mut b, &[&hello("B")]);
        assert_eq!(
            b.publisher().calls,
            vec![
                connect("A"),
                online("A"),
                offline("A"),
                Call::Disconnect,
                connect("B"),
                online("B"),
            ]
        );
    }

    #[test]
    fn shutdown_publishes_offline_and_disconnects() {
        let mut b = bridge();
        feed(&mut b, &[&hello("A")]);
        b.shutdown();
        assert_eq!(
            b.publisher().calls,
            vec![connect("A"), online("A"), offline("A"), Call::Disconnect]
        );
    }

    #[test]
    fn shutdown_after_port_lost_does_not_repeat_offline() {
        let mut b = bridge();
        feed(&mut b, &[&hello("A")]);
        b.on_port_lost();
        b.shutdown();
        assert_eq!(
            b.publisher().calls,
            vec![connect("A"), online("A"), offline("A"), Call::Disconnect]
        );
    }

    #[test]
    fn error_lines_and_invalid_measurements_are_only_logged() {
        let mut b = bridge();
        feed(
            &mut b,
            &[
                r#"{"type":"error","device_id":"A","message":"i2c"}"#,
                r#"{"type":"measurement","device_id":"A","pm1_0":1.0}"#,
            ],
        );
        assert!(b.publisher().calls.is_empty());
    }

    #[test]
    fn garbage_never_aborts_and_valid_lines_still_flow() {
        let mut b = bridge();
        let mut junk: Vec<Vec<u8>> = vec![
            vec![0xff; 5000],
            b"\x00\x01\x02".to_vec(),
            b"{\"type\":\"measurement\"".to_vec(),
            b"}}}}".to_vec(),
            "\u{1F600} not json".as_bytes().to_vec(),
        ];
        junk.push(measurement("A").into_bytes());
        for line in &junk {
            b.handle_line(line);
        }
        assert_eq!(
            b.publisher().calls,
            vec![connect("A"), online("A"), state("A")]
        );
    }

    #[test]
    fn excerpt_is_truncated_and_printable() {
        let long = vec![b'x'; 500];
        let e = excerpt(&long);
        assert_eq!(e.chars().count(), EXCERPT_CHARS + 1);
        assert!(e.ends_with('…'));
        assert_eq!(excerpt(b"a\x1bb\n"), "a\\u{1b}b\\n");
        assert_eq!(excerpt(&[0xff]), "\u{fffd}");
    }

    #[test]
    fn custom_topic_prefix() {
        let mut b = Bridge::new(FakePublisher::default(), FixedClock::default(), "home/air");
        feed(&mut b, &[&hello("A")]);
        let Call::Publish(msg) = &b.publisher().calls[1] else {
            panic!()
        };
        assert_eq!(msg.topic, "home/air/A/availability");
    }
}
