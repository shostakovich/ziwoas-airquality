//! Serial port adapter: raw bytes to lines, tolerant of garbage.

use crate::runner::{LineSource, PortOpener};
use std::collections::VecDeque;
use std::io::{self, Read};
use std::time::Duration;
use tracing::warn;

/// Ignored by USB-CDC, but must never be 1200: that baud rate reboots the RP2040 into its bootloader.
const BAUD_RATE: u32 = 115_200;
const READ_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_LINE_BYTES: usize = 4096;

/// Splits a byte stream on `\n`, strips a trailing `\r`, drops overlong lines.
#[derive(Default)]
pub struct LineSplitter {
    buf: Vec<u8>,
    discarding: bool,
    ready: VecDeque<Vec<u8>>,
}

impl LineSplitter {
    pub fn push(&mut self, bytes: &[u8]) {
        for chunk in bytes.split_inclusive(|&b| b == b'\n') {
            let (data, complete) = match chunk.split_last() {
                Some((b'\n', data)) => (data, true),
                _ => (chunk, false),
            };
            if !self.discarding {
                if self.buf.len() + data.len() > MAX_LINE_BYTES {
                    warn!(limit = MAX_LINE_BYTES, "dropping overlong serial line");
                    self.buf.clear();
                    self.discarding = true;
                } else {
                    self.buf.extend_from_slice(data);
                }
            }
            if complete {
                if !self.discarding {
                    let mut line = std::mem::take(&mut self.buf);
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    self.ready.push_back(line);
                }
                self.discarding = false;
            }
        }
    }

    pub fn next_line(&mut self) -> Option<Vec<u8>> {
        self.ready.pop_front()
    }
}

pub struct SerialOpener {
    path: String,
}

impl SerialOpener {
    pub fn new(path: impl Into<String>) -> Self {
        SerialOpener { path: path.into() }
    }
}

impl PortOpener for SerialOpener {
    type Source = SerialLineSource<Box<dyn serialport::SerialPort>>;

    fn open(&mut self) -> io::Result<Self::Source> {
        let mut port = serialport::new(&self.path, BAUD_RATE)
            .timeout(READ_TIMEOUT)
            .open()?;
        // arduino-pico only reports a connected host (and writes) while DTR is asserted.
        port.write_data_terminal_ready(true)?;
        Ok(SerialLineSource::new(port))
    }

    fn describe(&self) -> String {
        self.path.clone()
    }
}

pub struct SerialLineSource<R> {
    reader: R,
    splitter: LineSplitter,
    buf: Box<[u8; 1024]>,
}

impl<R: Read> SerialLineSource<R> {
    pub fn new(reader: R) -> Self {
        SerialLineSource {
            reader,
            splitter: LineSplitter::default(),
            buf: Box::new([0; 1024]),
        }
    }
}

impl<R: Read> LineSource for SerialLineSource<R> {
    fn read_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        if let Some(line) = self.splitter.next_line() {
            return Ok(Some(line));
        }
        match self.reader.read(&mut self.buf[..]) {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "end of file on serial port",
            )),
            Ok(n) => {
                self.splitter.push(&self.buf[..n]);
                Ok(self.splitter.next_line())
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut s = LineSplitter::default();
        let mut out = Vec::new();
        for c in chunks {
            s.push(c);
            while let Some(l) = s.next_line() {
                out.push(l);
            }
        }
        out
    }

    #[test]
    fn splits_on_newline_and_strips_cr() {
        assert_eq!(
            split(&[b"a\r\nb\n\nc"]),
            vec![b"a".to_vec(), b"b".to_vec(), b"".to_vec()]
        );
    }

    #[test]
    fn joins_lines_across_chunks() {
        assert_eq!(
            split(&[b"{\"ty", b"pe\":1", b"}\r", b"\nx\n"]),
            vec![b"{\"type\":1}".to_vec(), b"x".to_vec()]
        );
    }

    #[test]
    fn keeps_invalid_utf8_bytes() {
        assert_eq!(split(&[&[0xff, 0x00, b'\n']]), vec![vec![0xff, 0x00]]);
    }

    #[test]
    fn drops_overlong_lines_and_recovers() {
        let long = vec![b'x'; MAX_LINE_BYTES + 1];
        let exact = vec![b'y'; MAX_LINE_BYTES];
        let mut input = long.clone();
        input.extend_from_slice(b"tail\nok\n");
        input.extend_from_slice(&exact);
        input.push(b'\n');
        assert_eq!(split(&[&input]), vec![b"ok".to_vec(), exact.clone()]);
        // Overlong across many chunks.
        let chunks: Vec<&[u8]> = std::iter::repeat_n(&b"zzzzzzzz"[..], 1000)
            .chain([&b"\nfine\n"[..]])
            .collect();
        assert_eq!(split(&chunks), vec![b"fine".to_vec()]);
    }

    struct Script(VecDeque<io::Result<Vec<u8>>>);

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front() {
                Some(Ok(bytes)) => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Some(Err(e)) => Err(e),
                None => Ok(0),
            }
        }
    }

    #[test]
    fn source_maps_timeouts_lines_and_disconnects() {
        let mut src = SerialLineSource::new(Script(
            vec![
                Err(io::Error::new(io::ErrorKind::TimedOut, "t")),
                Ok(b"a\nb\nc".to_vec()),
                Ok(b"\n".to_vec()),
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone")),
            ]
            .into(),
        ));
        assert_eq!(src.read_line().unwrap(), None);
        assert_eq!(src.read_line().unwrap(), Some(b"a".to_vec()));
        assert_eq!(src.read_line().unwrap(), Some(b"b".to_vec()));
        assert_eq!(src.read_line().unwrap(), Some(b"c".to_vec()));
        assert!(src.read_line().is_err());
    }

    #[test]
    fn eof_is_a_disconnect() {
        let mut src = SerialLineSource::new(Script(VecDeque::new()));
        let err = src.read_line().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}
