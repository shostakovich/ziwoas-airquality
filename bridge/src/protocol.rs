//! Serial line protocol (firmware -> bridge), see SPEC.md.

use serde::{Deserialize, Deserializer, de::Error as _};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Line {
    Hello(Hello),
    Measurement(Measurement),
    Error(ErrorReport),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Hello {
    #[serde(deserialize_with = "device_id")]
    pub device_id: String,
    pub product: String,
    pub sensor_fw: String,
    pub firmware: String,
}

/// All fields are required; `null` means "unknown". Integers must be JSON integers.
/// Only presence and type are checked, never value ranges.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Measurement {
    #[serde(deserialize_with = "device_id")]
    pub device_id: String,
    #[serde(deserialize_with = "nullable")]
    pub pm1_0: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub pm2_5: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub pm4_0: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub pm10: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub temperature: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub humidity: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub voc_index: Option<i64>,
    #[serde(deserialize_with = "nullable")]
    pub nox_index: Option<i64>,
    #[serde(deserialize_with = "nullable")]
    pub co2: Option<i64>,
    /// Raw `readDeviceStatus()` register; always present, never null.
    pub device_status: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ErrorReport {
    #[serde(default)]
    pub device_id: Option<String>,
    pub message: String,
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Empty,
    Invalid(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => f.write_str("empty line"),
            ParseError::Invalid(reason) => f.write_str(reason),
        }
    }
}

pub fn parse_line(bytes: &[u8]) -> Result<Line, ParseError> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(ParseError::Empty);
    }
    serde_json::from_slice(bytes).map_err(|e| ParseError::Invalid(e.to_string()))
}

// A custom deserializer turns off serde's "missing Option = None" shortcut,
// so the field must be present while `null` is still accepted.
fn nullable<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de)
}

// The id becomes an MQTT topic level, so reject what would break the topic.
fn device_id<'de, D: Deserializer<'de>>(de: D) -> Result<String, D::Error> {
    let id = String::deserialize(de)?;
    if id.is_empty() {
        return Err(D::Error::custom("device_id must not be empty"));
    }
    if id
        .chars()
        .any(|c| matches!(c, '/' | '+' | '#') || c.is_control())
    {
        return Err(D::Error::custom(
            "device_id must not contain '/', '+', '#' or control characters",
        ));
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEASUREMENT: &str = r#"{"type":"measurement","device_id":"0123456789ABCDEF","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}"#;

    fn measurement_json(
        edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
    ) -> String {
        let mut v: serde_json::Value = serde_json::from_str(MEASUREMENT).unwrap();
        edit(v.as_object_mut().unwrap());
        v.to_string()
    }

    fn invalid(line: &str) -> String {
        match parse_line(line.as_bytes()) {
            Err(ParseError::Invalid(reason)) => reason,
            other => panic!("expected invalid for {line:?}, got {other:?}"),
        }
    }

    #[test]
    fn valid_measurement() {
        let Line::Measurement(m) = parse_line(MEASUREMENT.as_bytes()).unwrap() else {
            panic!("not a measurement");
        };
        assert_eq!(
            m,
            Measurement {
                device_id: "0123456789ABCDEF".into(),
                pm1_0: Some(2.1),
                pm2_5: Some(3.4),
                pm4_0: Some(3.9),
                pm10: Some(4.2),
                temperature: Some(21.7),
                humidity: Some(48.2),
                voc_index: Some(102),
                nox_index: Some(1),
                co2: Some(612),
                device_status: 0,
            }
        );
    }

    #[test]
    fn measurement_with_nulls() {
        let line = measurement_json(|o| {
            for k in ["pm1_0", "temperature", "voc_index", "nox_index", "co2"] {
                o.insert(k.into(), serde_json::Value::Null);
            }
        });
        let Line::Measurement(m) = parse_line(line.as_bytes()).unwrap() else {
            panic!()
        };
        assert_eq!(m.pm1_0, None);
        assert_eq!(m.temperature, None);
        assert_eq!(m.voc_index, None);
        assert_eq!(m.nox_index, None);
        assert_eq!(m.co2, None);
        assert_eq!(m.pm2_5, Some(3.4));
    }

    #[test]
    fn integer_values_are_fine_for_float_fields() {
        let line = measurement_json(|o| {
            o.insert("temperature".into(), 21.into());
        });
        let Line::Measurement(m) = parse_line(line.as_bytes()).unwrap() else {
            panic!()
        };
        assert_eq!(m.temperature, Some(21.0));
    }

    #[test]
    fn every_measurement_field_is_required() {
        for field in [
            "device_id",
            "pm1_0",
            "pm2_5",
            "pm4_0",
            "pm10",
            "temperature",
            "humidity",
            "voc_index",
            "nox_index",
            "co2",
            "device_status",
        ] {
            let line = measurement_json(|o| {
                o.remove(field);
            });
            let reason = invalid(&line);
            assert!(reason.contains("missing field"), "{field}: {reason}");
        }
    }

    #[test]
    fn wrong_types_are_rejected() {
        for (field, value) in [
            ("pm2_5", serde_json::json!("3.4")),
            ("humidity", serde_json::json!(true)),
            ("co2", serde_json::json!([612])),
            ("device_id", serde_json::json!(123)),
            ("device_id", serde_json::Value::Null),
            ("device_status", serde_json::json!("0")),
            ("device_status", serde_json::Value::Null),
            ("device_status", serde_json::json!(-1)),
        ] {
            let line = measurement_json(|o| {
                o.insert(field.into(), value.clone());
            });
            invalid(&line);
        }
    }

    #[test]
    fn values_are_not_range_checked() {
        let line = measurement_json(|o| {
            o.insert("voc_index".into(), 0.into());
            o.insert("nox_index".into(), 9999.into());
            o.insert("co2".into(), (-5).into());
            o.insert("device_status".into(), u32::MAX.into());
            o.insert("temperature".into(), serde_json::json!(-273.5));
        });
        let Line::Measurement(m) = parse_line(line.as_bytes()).unwrap() else {
            panic!()
        };
        assert_eq!(
            (m.voc_index, m.nox_index, m.co2, m.device_status),
            (Some(0), Some(9999), Some(-5), u32::MAX)
        );
        assert_eq!(m.temperature, Some(-273.5));
    }

    #[test]
    fn float_where_integer_expected_is_rejected() {
        for field in ["voc_index", "nox_index", "co2", "device_status"] {
            let line = measurement_json(|o| {
                o.insert(field.into(), serde_json::json!(1.5));
            });
            invalid(&line);
        }
    }

    #[test]
    fn empty_or_unsafe_device_id_is_rejected() {
        for id in ["", "a/b", "a+b", "a#b", "a\nb"] {
            let line = measurement_json(|o| {
                o.insert("device_id".into(), id.into());
            });
            invalid(&line);
        }
    }

    #[test]
    fn hello_line() {
        let line = r#"{"type":"hello","device_id":"19B8E27966467D7A","product":"SEN66","sensor_fw":"4.0","firmware":"v1.0-3-gdeadbee"}"#;
        assert_eq!(
            parse_line(line.as_bytes()).unwrap(),
            Line::Hello(Hello {
                device_id: "19B8E27966467D7A".into(),
                product: "SEN66".into(),
                sensor_fw: "4.0".into(),
                firmware: "v1.0-3-gdeadbee".into(),
            })
        );
        invalid(r#"{"type":"hello","device_id":"ABC"}"#);
        invalid(
            r#"{"type":"hello","device_id":"","product":"SEN66","sensor_fw":"4.0","firmware":"x"}"#,
        );
    }

    #[test]
    fn error_line_with_and_without_device_id() {
        assert_eq!(
            parse_line(br#"{"type":"error","device_id":null,"message":"no sensor"}"#).unwrap(),
            Line::Error(ErrorReport {
                device_id: None,
                message: "no sensor".into()
            })
        );
        assert_eq!(
            parse_line(br#"{"type":"error","device_id":"ABC","message":"i2c \"read\" failed"}"#)
                .unwrap(),
            Line::Error(ErrorReport {
                device_id: Some("ABC".into()),
                message: "i2c \"read\" failed".into()
            })
        );
    }

    #[test]
    fn unknown_or_missing_type_is_rejected() {
        invalid(r#"{"type":"debug","device_id":"ABC"}"#);
        invalid(r#"{"device_id":"ABC","message":"x"}"#);
        invalid(r#"{"type":null}"#);
    }

    #[test]
    fn garbage_is_rejected() {
        for line in [
            "hello world",
            "{",
            r#"{"type":"measurement","device_id":"AB"#,
            "[]",
            "42",
            "null",
            &format!("{MEASUREMENT} trailing"),
        ] {
            invalid(line);
        }
        assert!(matches!(
            parse_line(&[0xff, 0xfe, b'{', 0x00]),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn empty_lines() {
        assert_eq!(parse_line(b""), Err(ParseError::Empty));
        assert_eq!(parse_line(b"  \t"), Err(ParseError::Empty));
    }
}
