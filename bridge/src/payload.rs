//! MQTT state payload, key order as in the contract.

use crate::protocol::Measurement;
use serde::Serialize;
use std::time::SystemTime;

#[derive(Serialize)]
struct State<'a> {
    device_id: &'a str,
    taken_at: String,
    pm1_0: Option<f64>,
    pm2_5: Option<f64>,
    pm4_0: Option<f64>,
    pm10: Option<f64>,
    temperature: Option<f64>,
    humidity: Option<f64>,
    voc_index: Option<i64>,
    nox_index: Option<i64>,
    co2: Option<i64>,
    device_status: u32,
}

/// UTC, RFC 3339, whole seconds (sub-seconds are truncated), `Z` suffix.
pub fn format_taken_at(t: SystemTime) -> String {
    humantime::format_rfc3339_seconds(t).to_string()
}

pub fn state_payload(m: &Measurement, taken_at: SystemTime) -> String {
    let state = State {
        device_id: &m.device_id,
        taken_at: format_taken_at(taken_at),
        pm1_0: m.pm1_0,
        pm2_5: m.pm2_5,
        pm4_0: m.pm4_0,
        pm10: m.pm10,
        temperature: m.temperature,
        humidity: m.humidity,
        voc_index: m.voc_index,
        nox_index: m.nox_index,
        co2: m.co2,
        device_status: m.device_status,
    };
    serde_json::to_string(&state).expect("state payload is always serializable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Line, parse_line};
    use std::time::{Duration, UNIX_EPOCH};

    // 2026-09-22T14:03:00Z
    const T: u64 = 1_790_085_780;

    fn measurement(line: &str) -> Measurement {
        match parse_line(line.as_bytes()).unwrap() {
            Line::Measurement(m) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn matches_contract_example_exactly() {
        let m = measurement(
            r#"{"type":"measurement","device_id":"0123456789ABCDEF","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}"#,
        );
        let payload = state_payload(&m, UNIX_EPOCH + Duration::from_secs(T));
        assert_eq!(
            payload,
            r#"{"device_id":"0123456789ABCDEF","taken_at":"2026-09-22T14:03:00Z","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}"#
        );
        assert!(!payload.contains("\"type\""));
    }

    #[test]
    fn key_order_is_independent_of_input_order() {
        let m = measurement(
            r#"{"device_status":7,"co2":null,"nox_index":null,"voc_index":null,"humidity":null,"temperature":null,"pm10":null,"pm4_0":null,"pm2_5":null,"pm1_0":null,"device_id":"X","type":"measurement","extra":1}"#,
        );
        assert_eq!(
            state_payload(&m, UNIX_EPOCH + Duration::from_secs(T)),
            r#"{"device_id":"X","taken_at":"2026-09-22T14:03:00Z","pm1_0":null,"pm2_5":null,"pm4_0":null,"pm10":null,"temperature":null,"humidity":null,"voc_index":null,"nox_index":null,"co2":null,"device_status":7}"#
        );
    }

    #[test]
    fn taken_at_is_truncated_to_seconds() {
        let t = UNIX_EPOCH + Duration::from_secs(T) + Duration::from_millis(999);
        assert_eq!(format_taken_at(t), "2026-09-22T14:03:00Z");
        let t = UNIX_EPOCH + Duration::from_secs(T + 59) + Duration::from_nanos(1);
        assert_eq!(format_taken_at(t), "2026-09-22T14:03:59Z");
    }
}
