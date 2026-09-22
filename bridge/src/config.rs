use std::fmt;

pub const DEFAULT_SERIAL_PORT: &str = "/dev/ttySEN66";
pub const DEFAULT_MQTT_PORT: u16 = 1883;
pub const DEFAULT_TOPIC_PREFIX: &str = "ziwoas/sen66";
pub const DEFAULT_LOG_LEVEL: &str = "info";

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub serial_port: String,
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub topic_prefix: String,
    pub log_level: String,
}

// Hand-written so the password never ends up in a log line.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("serial_port", &self.serial_port)
            .field("mqtt_host", &self.mqtt_host)
            .field("mqtt_port", &self.mqtt_port)
            .field("mqtt_username", &self.mqtt_username)
            .field("mqtt_password", &self.mqtt_password.as_ref().map(|_| "***"))
            .field("topic_prefix", &self.topic_prefix)
            .field("log_level", &self.log_level)
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid {
        var: &'static str,
        value: String,
        reason: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(var) => write!(f, "{var} is required but not set"),
            ConfigError::Invalid { var, value, reason } => {
                write!(f, "{var}={value:?} is invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Empty values count as unset, so `FOO=` in a compose file falls back to the default.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());

        let mqtt_host = get("MQTT_HOST").ok_or(ConfigError::Missing("MQTT_HOST"))?;

        let mqtt_port = match get("MQTT_PORT") {
            None => DEFAULT_MQTT_PORT,
            Some(raw) => match raw.trim().parse::<u16>() {
                Ok(port) if port != 0 => port,
                _ => {
                    return Err(ConfigError::Invalid {
                        var: "MQTT_PORT",
                        value: raw,
                        reason: "expected a port number between 1 and 65535",
                    });
                }
            },
        };

        let mqtt_username = get("MQTT_USERNAME");
        let mqtt_password = get("MQTT_PASSWORD");
        if mqtt_password.is_some() && mqtt_username.is_none() {
            return Err(ConfigError::Invalid {
                var: "MQTT_PASSWORD",
                value: "***".into(),
                reason: "MQTT_PASSWORD requires MQTT_USERNAME",
            });
        }

        let topic_prefix = match get("TOPIC_PREFIX") {
            None => DEFAULT_TOPIC_PREFIX.to_string(),
            Some(raw) => {
                let trimmed = raw.trim().trim_end_matches('/');
                if trimmed.is_empty() || trimmed.contains(['+', '#']) {
                    return Err(ConfigError::Invalid {
                        var: "TOPIC_PREFIX",
                        value: raw,
                        reason: "must be a non-empty topic without wildcards",
                    });
                }
                trimmed.to_string()
            }
        };

        Ok(Config {
            serial_port: get("SERIAL_PORT").unwrap_or_else(|| DEFAULT_SERIAL_PORT.to_string()),
            mqtt_host: mqtt_host.trim().to_string(),
            mqtt_port,
            mqtt_username,
            mqtt_password,
            topic_prefix,
            log_level: get("LOG_LEVEL").unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|name| map.get(name).cloned())
    }

    #[test]
    fn defaults_with_only_host() {
        let cfg = load(&[("MQTT_HOST", "broker")]).unwrap();
        assert_eq!(
            cfg,
            Config {
                serial_port: "/dev/ttySEN66".into(),
                mqtt_host: "broker".into(),
                mqtt_port: 1883,
                mqtt_username: None,
                mqtt_password: None,
                topic_prefix: "ziwoas/sen66".into(),
                log_level: "info".into(),
            }
        );
    }

    #[test]
    fn all_values_overridden() {
        let cfg = load(&[
            ("SERIAL_PORT", "/dev/ttyUSB3"),
            ("MQTT_HOST", "10.0.0.2"),
            ("MQTT_PORT", "1884"),
            ("MQTT_USERNAME", "u"),
            ("MQTT_PASSWORD", "p"),
            ("TOPIC_PREFIX", "home/air/"),
            ("LOG_LEVEL", "debug"),
        ])
        .unwrap();
        assert_eq!(cfg.serial_port, "/dev/ttyUSB3");
        assert_eq!(cfg.mqtt_port, 1884);
        assert_eq!(cfg.mqtt_username.as_deref(), Some("u"));
        assert_eq!(cfg.mqtt_password.as_deref(), Some("p"));
        assert_eq!(cfg.topic_prefix, "home/air");
        assert_eq!(cfg.log_level, "debug");
    }

    #[test]
    fn missing_host_is_an_error() {
        assert_eq!(load(&[]), Err(ConfigError::Missing("MQTT_HOST")));
        assert_eq!(
            load(&[("MQTT_HOST", "  ")]),
            Err(ConfigError::Missing("MQTT_HOST"))
        );
    }

    #[test]
    fn bad_port_is_an_error() {
        for bad in ["abc", "0", "65536", "-1", "1883.0"] {
            let err = load(&[("MQTT_HOST", "b"), ("MQTT_PORT", bad)]).unwrap_err();
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        var: "MQTT_PORT",
                        ..
                    }
                ),
                "{bad}: {err:?}"
            );
        }
    }

    #[test]
    fn empty_values_fall_back_to_defaults() {
        let cfg = load(&[
            ("MQTT_HOST", "b"),
            ("MQTT_PORT", ""),
            ("MQTT_USERNAME", ""),
            ("SERIAL_PORT", ""),
        ])
        .unwrap();
        assert_eq!(cfg.mqtt_port, 1883);
        assert_eq!(cfg.mqtt_username, None);
        assert_eq!(cfg.serial_port, "/dev/ttySEN66");
    }

    #[test]
    fn password_without_username_is_an_error() {
        let err = load(&[("MQTT_HOST", "b"), ("MQTT_PASSWORD", "p")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                var: "MQTT_PASSWORD",
                ..
            }
        ));
    }

    #[test]
    fn wildcard_prefix_is_an_error() {
        let err = load(&[("MQTT_HOST", "b"), ("TOPIC_PREFIX", "a/#")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                var: "TOPIC_PREFIX",
                ..
            }
        ));
    }

    #[test]
    fn debug_output_hides_password() {
        let cfg = load(&[
            ("MQTT_HOST", "b"),
            ("MQTT_USERNAME", "u"),
            ("MQTT_PASSWORD", "secret"),
        ])
        .unwrap();
        assert!(!format!("{cfg:?}").contains("secret"));
    }
}
