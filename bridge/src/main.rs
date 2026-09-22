use sen66_bridge::bridge::{Bridge, SystemClock};
use sen66_bridge::config::Config;
use sen66_bridge::mqtt::{MqttPublisher, MqttSettings};
use sen66_bridge::runner::{interruptible_sleep, run};
use sen66_bridge::serial::SerialOpener;
use signal_hook::consts::{SIGINT, SIGTERM};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("configuration error: {e}");
            return ExitCode::from(2);
        }
    };
    let filter = match EnvFilter::try_new(&config.log_level) {
        Ok(filter) => filter,
        Err(e) => {
            eprintln!("configuration error: LOG_LEVEL={:?}: {e}", config.log_level);
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        .with_ansi(false)
        .with_target(false)
        .init();

    let stop = Arc::new(AtomicBool::new(false));
    for signal in [SIGTERM, SIGINT] {
        // A second signal while shutting down exits immediately.
        let registered = signal_hook::flag::register_conditional_shutdown(signal, 1, stop.clone())
            .and_then(|_| signal_hook::flag::register(signal, stop.clone()));
        if let Err(e) = registered {
            eprintln!("cannot install signal handler: {e}");
            return ExitCode::FAILURE;
        }
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        serial_port = %config.serial_port,
        broker = %format!("{}:{}", config.mqtt_host, config.mqtt_port),
        auth = config.mqtt_username.is_some(),
        topic_prefix = %config.topic_prefix,
        "starting sen66-bridge"
    );

    let publisher = MqttPublisher::new(MqttSettings {
        host: config.mqtt_host,
        port: config.mqtt_port,
        credentials: config
            .mqtt_username
            .map(|user| (user, config.mqtt_password.unwrap_or_default())),
    });
    let mut bridge = Bridge::new(publisher, SystemClock, config.topic_prefix);
    let mut opener = SerialOpener::new(config.serial_port);
    run(&mut opener, &mut bridge, &stop, interruptible_sleep);
    info!("bye");
    ExitCode::SUCCESS
}
