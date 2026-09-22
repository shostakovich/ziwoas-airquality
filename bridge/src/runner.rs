//! Main loop: open the port, feed lines to the bridge, reopen with backoff.

use crate::bridge::{Bridge, Clock, Publisher};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{debug, info, warn};

pub const MIN_BACKOFF: Duration = Duration::from_secs(1);
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);

pub trait LineSource {
    /// `Ok(Some(line))` for a complete line, `Ok(None)` on read timeout,
    /// `Err` when the port is gone.
    fn read_line(&mut self) -> io::Result<Option<Vec<u8>>>;
}

pub trait PortOpener {
    type Source: LineSource;
    fn open(&mut self) -> io::Result<Self::Source>;
    fn describe(&self) -> String;
}

/// Runs until `stop` is set, then publishes offline and disconnects.
/// `sleep` must return early once `stop` is set.
pub fn run<O, P, C>(
    opener: &mut O,
    bridge: &mut Bridge<P, C>,
    stop: &AtomicBool,
    mut sleep: impl FnMut(Duration, &AtomicBool),
) where
    O: PortOpener,
    P: Publisher,
    C: Clock,
{
    let mut backoff = MIN_BACKOFF;
    let mut open_failures = 0u32;

    'outer: while !stop.load(Ordering::SeqCst) {
        match opener.open() {
            Ok(mut source) => {
                info!(port = %opener.describe(), "serial port opened");
                open_failures = 0;
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break 'outer;
                    }
                    match source.read_line() {
                        Ok(Some(line)) => {
                            backoff = MIN_BACKOFF;
                            bridge.handle_line(&line);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!(port = %opener.describe(), "serial port lost: {e}");
                            bridge.on_port_lost();
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                bridge.on_port_lost();
                open_failures += 1;
                // Log the first failure loudly, repeats quietly.
                if open_failures == 1 {
                    warn!(port = %opener.describe(), "cannot open serial port: {e}; retrying with backoff");
                } else {
                    debug!(port = %opener.describe(), attempt = open_failures, "cannot open serial port: {e}");
                }
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        debug!(delay = ?backoff, "waiting before reopening serial port");
        sleep(backoff, stop);
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }

    info!("shutting down");
    bridge.shutdown();
}

/// Sleeps in small steps so a signal can cut the wait short.
pub fn interruptible_sleep(duration: Duration, stop: &AtomicBool) {
    const STEP: Duration = Duration::from_millis(100);
    let mut left = duration;
    while !left.is_zero() && !stop.load(Ordering::SeqCst) {
        let step = left.min(STEP);
        std::thread::sleep(step);
        left -= step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::fakes::*;
    use std::collections::VecDeque;
    use std::rc::Rc;

    enum Read {
        Line(String),
        Timeout,
        Lost,
    }

    struct FakeSource {
        reads: VecDeque<Read>,
        stop: Rc<AtomicBool>,
    }

    impl LineSource for FakeSource {
        fn read_line(&mut self) -> io::Result<Option<Vec<u8>>> {
            match self.reads.pop_front() {
                Some(Read::Line(l)) => Ok(Some(l.into_bytes())),
                Some(Read::Timeout) => Ok(None),
                Some(Read::Lost) => Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone")),
                None => {
                    // Script exhausted: simulate SIGTERM.
                    self.stop.store(true, Ordering::SeqCst);
                    Ok(None)
                }
            }
        }
    }

    /// Each entry is one open attempt: `None` = open fails, `Some` = the reads of that session.
    struct FakeOpener {
        sessions: VecDeque<Option<Vec<Read>>>,
        stop: Rc<AtomicBool>,
    }

    impl PortOpener for FakeOpener {
        type Source = FakeSource;
        fn open(&mut self) -> io::Result<FakeSource> {
            match self.sessions.pop_front() {
                Some(Some(reads)) => Ok(FakeSource {
                    reads: reads.into(),
                    stop: self.stop.clone(),
                }),
                Some(None) => Err(io::Error::new(io::ErrorKind::NotFound, "no such device")),
                None => {
                    self.stop.store(true, Ordering::SeqCst);
                    Err(io::Error::new(io::ErrorKind::NotFound, "script exhausted"))
                }
            }
        }
        fn describe(&self) -> String {
            "fake".into()
        }
    }

    fn run_script(sessions: Vec<Option<Vec<Read>>>) -> (Vec<Call>, Vec<Duration>) {
        let stop = Rc::new(AtomicBool::new(false));
        let mut opener = FakeOpener {
            sessions: sessions.into(),
            stop: stop.clone(),
        };
        let mut bridge = crate::bridge::Bridge::new(
            FakePublisher::default(),
            FixedClock::default(),
            "ziwoas/sen66",
        );
        let mut sleeps = Vec::new();
        run(&mut opener, &mut bridge, &stop, |d, _| sleeps.push(d));
        (bridge.publisher().calls.clone(), sleeps)
    }

    #[test]
    fn full_lifecycle_unplug_replug_shutdown() {
        let (calls, sleeps) = run_script(vec![
            Some(vec![
                Read::Line(hello("A")),
                Read::Timeout,
                Read::Line("garbage".into()),
                Read::Line(measurement("A")),
                Read::Lost,
            ]),
            None,
            None,
            Some(vec![Read::Timeout, Read::Line(measurement("A"))]),
        ]);
        assert_eq!(
            calls,
            vec![
                connect("A"),
                online("A"),
                state("A"),
                offline("A"),
                online("A"),
                state("A"),
                offline("A"),
                Call::Disconnect,
            ]
        );
        assert_eq!(
            sleeps,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4)
            ]
        );
    }

    #[test]
    fn backoff_caps_at_sixty_seconds() {
        let (calls, sleeps) = run_script((0..9).map(|_| None).collect());
        assert!(calls.is_empty());
        let secs: Vec<u64> = sleeps.iter().map(Duration::as_secs).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }

    #[test]
    fn backoff_resets_after_a_line_was_received() {
        let (_, sleeps) = run_script(vec![
            None,
            None,
            Some(vec![Read::Line(hello("A")), Read::Lost]),
            None,
        ]);
        let secs: Vec<u64> = sleeps.iter().map(Duration::as_secs).collect();
        assert_eq!(secs, vec![1, 2, 1, 2]);
    }

    #[test]
    fn interruptible_sleep_returns_immediately_when_stopped() {
        let stop = AtomicBool::new(true);
        let start = std::time::Instant::now();
        interruptible_sleep(Duration::from_secs(60), &stop);
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
