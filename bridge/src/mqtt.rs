//! rumqttc adapter for the `Publisher` trait.

use crate::bridge::{Message, PublishError, Publisher, Qos};
use crate::runner::interruptible_sleep;
use rumqttc::{Client, Connection, Event, LastWill, MqttOptions, Outgoing, Packet, QoS};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tracing::{debug, info, trace, warn};

const KEEP_ALIVE: Duration = Duration::from_secs(30);
const REQUEST_QUEUE: usize = 16;
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RETRY: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct MqttSettings {
    pub host: String,
    pub port: u16,
    pub credentials: Option<(String, String)>,
}

pub struct MqttPublisher {
    settings: MqttSettings,
    session: Option<Session>,
}

struct Session {
    client: Client,
    closing: Arc<AtomicBool>,
    /// Last retained payload per topic, re-sent after the broker connection comes back
    /// (the broker will have fired our last will in the meantime).
    retained: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    done: mpsc::Receiver<()>,
    thread: thread::JoinHandle<()>,
}

impl MqttPublisher {
    pub fn new(settings: MqttSettings) -> Self {
        MqttPublisher {
            settings,
            session: None,
        }
    }
}

fn qos(q: Qos) -> QoS {
    match q {
        Qos::AtMostOnce => QoS::AtMostOnce,
        Qos::AtLeastOnce => QoS::AtLeastOnce,
        Qos::ExactlyOnce => QoS::ExactlyOnce,
    }
}

impl Publisher for MqttPublisher {
    fn connect(&mut self, client_id: &str, will: Message) {
        self.disconnect();

        let mut options = MqttOptions::new(client_id, &self.settings.host, self.settings.port);
        options
            .set_keep_alive(KEEP_ALIVE)
            .set_clean_session(true)
            .set_last_will(LastWill::new(
                will.topic,
                will.payload,
                qos(will.qos),
                will.retain,
            ));
        if let Some((user, pass)) = &self.settings.credentials {
            options.set_credentials(user, pass);
        }

        let (client, connection) = Client::new(options, REQUEST_QUEUE);
        let closing = Arc::new(AtomicBool::new(false));
        let retained = Arc::new(Mutex::new(HashMap::new()));
        let (done_tx, done) = mpsc::channel();
        let thread = {
            let client = client.clone();
            let closing = closing.clone();
            let retained = retained.clone();
            let broker = format!("{}:{}", self.settings.host, self.settings.port);
            thread::Builder::new()
                .name("mqtt".into())
                .spawn(move || {
                    drive(connection, &client, &closing, &retained, &broker);
                    let _ = done_tx.send(());
                })
                .expect("spawning the MQTT thread")
        };
        self.session = Some(Session {
            client,
            closing,
            retained,
            done,
            thread,
        });
    }

    fn publish(&mut self, message: Message) -> Result<(), PublishError> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| PublishError("not connected".into()))?;
        if message.retain {
            session
                .retained
                .lock()
                .unwrap()
                .insert(message.topic.clone(), message.payload.clone());
        }
        // Never block the serial loop: while the broker is away the queue fills and we drop.
        session
            .client
            .try_publish(
                message.topic,
                qos(message.qos),
                message.retain,
                message.payload,
            )
            .map_err(|e| PublishError(format!("MQTT queue full or closed: {e}")))
    }

    fn disconnect(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        session.closing.store(true, Ordering::SeqCst);
        if let Err(e) = session.client.try_disconnect() {
            warn!("could not queue MQTT disconnect: {e}");
        }
        match session.done.recv_timeout(FLUSH_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = session.thread.join();
                debug!("MQTT session closed");
            }
            Err(RecvTimeoutError::Timeout) => {
                warn!("MQTT disconnect not confirmed within {FLUSH_TIMEOUT:?}, giving up");
            }
        }
    }
}

/// Polls the connection (rumqttc reconnects on every poll after an error)
/// and logs state changes once rather than per retry.
fn drive(
    mut connection: Connection,
    client: &Client,
    closing: &AtomicBool,
    retained: &Mutex<HashMap<String, Vec<u8>>>,
    broker: &str,
) {
    let mut connected = false;
    let mut connacks = 0u32;
    let mut last_error: Option<String> = None;
    let mut retry = Duration::from_secs(1);

    for event in connection.iter() {
        match event {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                connacks += 1;
                info!(%broker, "connected to MQTT broker");
                connected = true;
                last_error = None;
                retry = Duration::from_secs(1);
                if connacks > 1 {
                    for (topic, payload) in retained.lock().unwrap().iter() {
                        if let Err(e) =
                            client.try_publish(topic, QoS::AtLeastOnce, true, payload.clone())
                        {
                            warn!("could not republish {topic}: {e}");
                        }
                    }
                }
            }
            Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                debug!("MQTT disconnect sent");
                break;
            }
            Ok(event) => trace!(?event, "mqtt event"),
            Err(e) => {
                if closing.load(Ordering::SeqCst) {
                    break;
                }
                let msg = e.to_string();
                if connected {
                    warn!(%broker, "lost connection to MQTT broker: {msg}");
                } else if last_error.as_deref() != Some(&msg) {
                    warn!(%broker, "cannot connect to MQTT broker: {msg}; retrying");
                }
                connected = false;
                last_error = Some(msg);
                interruptible_sleep(retry, closing);
                retry = (retry * 2).min(MAX_RETRY);
            }
        }
    }
}
