// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under
//  the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
//  KIND, either express or implied. See the License for the specific language governing
//  permissions and limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Minimal FIX 4.2 initiator for the local simulated exchange.
//!
//! Wire format follows the exchange's strict profile (see
//! `docs/orderhub/local-exchange.md`): NewOrderSingle requires
//! `HandlInst=1` and `TimeInForce=0` (DAY); only market and limit order
//! types are supported; cancels use 35=F with a fresh ClOrdID.
//!
//! The split session services heartbeats independently of the execution
//! core; all outgoing messages share one serialized socket and sequence.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rust_decimal::Decimal;

use crate::fix_config::FixSessionConfig;
use crate::fix_log::FixLog;

/// FIX field separator.
const SOH: char = '\u{1}';
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(250);
const TRANSPORT_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors from the FIX initiator.
#[derive(Debug)]
pub enum FixError {
    /// Underlying socket I/O failure.
    Io(std::io::Error),
    /// The exchange rejected the session or a message.
    Rejected(String),
    /// A received frame was malformed.
    Malformed(String),
}

impl std::error::Error for FixError {}

impl std::fmt::Display for FixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "FIX transport error: {err}"),
            Self::Rejected(reason) => write!(f, "FIX rejected: {reason}"),
            Self::Malformed(reason) => write!(f, "FIX malformed message: {reason}"),
        }
    }
}

impl From<std::io::Error> for FixError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// A decoded execution event for one client order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecEvent {
    /// The venue acknowledged the order (ExecType=0).
    New {
        /// Client order ID (tag 11).
        cl_ord_id: String,
        /// Venue order ID (tag 37).
        order_id: String,
    },
    /// A fill or partial fill (ExecType=1/2).
    Fill {
        cl_ord_id: String,
        order_id: String,
        /// Stable venue execution identity (tag 17).
        exec_id: String,
        /// ExecType 2 means fully filled.
        complete: bool,
        last_qty: Decimal,
        last_px: Decimal,
        cum_qty: Decimal,
    },
    /// The order was cancelled (ExecType=4).
    Canceled { cl_ord_id: String, order_id: String },
    /// The venue rejected the request (ExecType=8 or session-level 35=3).
    Rejected { cl_ord_id: String, reason: String },
    /// A session-level message with no order semantics.
    Session(String),
}

/// FIX 4.2 initiator over a TCP stream.
#[derive(Debug)]
pub struct FixInitiator {
    reader: FixReader,
    writer: FixWriter,
}

impl FixInitiator {
    /// Connects and performs the FIX logon handshake.
    ///
    /// # Errors
    ///
    /// Returns an error if the TCP connect, logon exchange, or heartbeat
    /// negotiation fails.
    pub fn connect(
        addr: &str,
        sender_comp_id: &str,
        target_comp_id: &str,
        heart_bt_int: u32,
    ) -> Result<Self, FixError> {
        Self::connect_logged(
            addr,
            sender_comp_id,
            target_comp_id,
            heart_bt_int,
            &FixLog::default(),
        )
    }

    /// Connects with validated file settings and a shared asynchronous logger.
    ///
    /// # Errors
    /// Returns configuration, transport or logon errors.
    pub fn connect_configured(config: &FixSessionConfig, log: &FixLog) -> Result<Self, FixError> {
        config
            .validate()
            .map_err(|e| FixError::Malformed(e.to_string()))?;
        Self::connect_logged(
            &config.address(),
            &config.sender_comp_id,
            &config.target_comp_id,
            config.heartbeat_seconds,
            log,
        )
    }

    fn connect_logged(
        addr: &str,
        sender: &str,
        target: &str,
        heartbeat: u32,
        log: &FixLog,
    ) -> Result<Self, FixError> {
        log.event(format!(
            "CONNECT_ATTEMPT address={addr} sender={sender} target={target}"
        ));
        let result = Self::connect_inner(addr, sender, target, heartbeat, log.clone());
        if let Err(e) = &result {
            log.event(format!("CONNECT_FAILED {e}"));
        }
        result
    }

    fn connect_inner(
        addr: &str,
        sender_comp_id: &str,
        target_comp_id: &str,
        heart_bt_int: u32,
        log: FixLog,
    ) -> Result<Self, FixError> {
        if heart_bt_int == 0 {
            return Err(FixError::Malformed("HeartBtInt must be positive".into()));
        }
        let stream = TcpStream::connect(addr)?;
        log.event(format!("TCP_CONNECTED address={addr}"));
        stream.set_read_timeout(Some(SESSION_POLL_INTERVAL))?;
        stream.set_write_timeout(Some(TRANSPORT_TIMEOUT))?;
        let writer = FixWriter {
            state: Arc::new(Mutex::new(FixWriteState {
                stream: stream.try_clone()?,
                sender_comp_id: sender_comp_id.to_string(),
                target_comp_id: target_comp_id.to_string(),
                outbound_seq: 1,
                last_sent: Instant::now(),
                closed: false,
                log: log.clone(),
            })),
            log,
        };
        let mut initiator = Self {
            reader: FixReader {
                stream,
                buffer: Vec::new(),
                writer: writer.clone(),
                heartbeat_interval: Duration::from_secs(u64::from(heart_bt_int)),
            },
            writer,
        };
        initiator.writer.send_app(
            "A",
            &[
                (98, "0".to_string()),           // EncryptMethod
                (108, heart_bt_int.to_string()), // HeartBtInt
            ],
        )?;

        // Await the acceptor's logon (35=A) acknowledging the session.
        let deadline = Instant::now() + TRANSPORT_TIMEOUT;
        loop {
            if Instant::now() >= deadline {
                return Err(FixError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "FIX logon timed out",
                )));
            }
            let Some(message) =
                poll_frame(&mut initiator.reader.stream, &mut initiator.reader.buffer)?
            else {
                continue;
            };
            initiator.writer.log.incoming(&message);
            let fields = parse_fields(&message)?;
            match field(&fields, 35).ok_or_else(|| FixError::Malformed("no MsgType".into()))? {
                "A" => {
                    if field(&fields, 108).and_then(|value| value.parse::<u32>().ok())
                        != Some(heart_bt_int)
                    {
                        return Err(FixError::Rejected("HeartBtInt negotiation mismatch".into()));
                    }
                    initiator
                        .writer
                        .log
                        .event(format!("LOGON_ACCEPTED heartbeat={heart_bt_int}s"));
                    return Ok(initiator);
                }
                "5" => {
                    return Err(FixError::Rejected(format!(
                        "logged out during handshake: {}",
                        field(&fields, 58).unwrap_or("no text")
                    )));
                }
                "3" => {
                    return Err(FixError::Rejected(format!(
                        "session reject: {}",
                        field(&fields, 58).unwrap_or("unspecified")
                    )));
                }
                _ => {}
            }
        }
    }

    /// Submits a new order (35=D).
    ///
    /// `price=None` sends a market order; `Some(px)` sends a limit order.
    /// TimeInForce is fixed to DAY per the exchange profile.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent.
    pub fn submit_order(
        &mut self,
        cl_ord_id: &str,
        symbol: &str,
        side: Side,
        quantity: Decimal,
        price: Option<Decimal>,
    ) -> Result<(), FixError> {
        self.writer
            .submit_order(cl_ord_id, symbol, side, quantity, price)
    }

    /// Requests cancellation of a live order (35=F).
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent.
    pub fn cancel_order(
        &mut self,
        cl_ord_id: &str,
        orig_cl_ord_id: &str,
        symbol: &str,
        side: Side,
    ) -> Result<(), FixError> {
        self.writer
            .cancel_order(cl_ord_id, orig_cl_ord_id, symbol, side)
    }

    /// Reads the next frame, servicing heartbeats and decoding order semantics.
    ///
    /// Callers using this unsplit API must keep reading to service the session.
    ///
    /// # Errors
    ///
    /// Returns an error on transport failure or a malformed frame.
    pub fn next_event(&mut self) -> Result<ExecEvent, FixError> {
        let result = self.reader.next_event();
        if result.is_err() {
            let _ = self.writer.close();
        }
        result
    }
}

/// Order side for FIX tag 54.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Buy (1).
    Buy,
    /// Sell (2).
    Sell,
}

impl Side {
    const fn tag(self) -> u8 {
        match self {
            Self::Buy => 1,
            Self::Sell => 2,
        }
    }
}

fn decode_execution_report(fields: &[(u32, String)]) -> Result<ExecEvent, FixError> {
    let cl_ord_id = field(fields, 11).unwrap_or_default().to_string();
    let order_id = field(fields, 37).unwrap_or_default().to_string();
    let exec_type = field(fields, 150).unwrap_or("0");
    // Cancel reports carry the fresh cancel request's ClOrdID in tag 11 and
    // reference the cancelled order through OrigClOrdID (tag 41); map them
    // back to the original order so the pump finds it in the cache.
    let cancel_cl_ord_id = field(fields, 41).unwrap_or(&cl_ord_id).to_string();
    Ok(match exec_type {
        "0" => ExecEvent::New {
            cl_ord_id,
            order_id,
        },
        "1" | "2" => ExecEvent::Fill {
            exec_id: field(fields, 17)
                .filter(|id| !id.is_empty() && id.len() <= 64 && !id.chars().any(char::is_control))
                .ok_or_else(|| FixError::Malformed("missing or invalid ExecID".into()))?
                .to_string(),
            cl_ord_id,
            order_id,
            complete: exec_type == "2",
            last_qty: parse_dec(field(fields, 32).unwrap_or("0"))?,
            last_px: parse_dec(field(fields, 31).unwrap_or("0"))?,
            cum_qty: parse_dec(field(fields, 14).unwrap_or("0"))?,
        },
        "4" => ExecEvent::Canceled {
            cl_ord_id: cancel_cl_ord_id,
            order_id,
        },
        "8" => ExecEvent::Rejected {
            cl_ord_id,
            reason: field(fields, 58).unwrap_or("unspecified").to_string(),
        },
        other => ExecEvent::Session(format!("ExecType {other}")),
    })
}

fn parse_dec(value: &str) -> Result<Decimal, FixError> {
    Decimal::from_str_radix_naive(value)
}

/// Minimal decimal parser (delegates to rust_decimal).
trait DecimalFromStrNaive {
    fn from_str_radix_naive(value: &str) -> Result<Decimal, FixError>;
}

impl DecimalFromStrNaive for Decimal {
    fn from_str_radix_naive(value: &str) -> Result<Decimal, FixError> {
        use rust_decimal::prelude::FromStr;
        Self::from_str(value).map_err(|_| FixError::Malformed(format!("invalid decimal {value}")))
    }
}

fn timestamp_now() -> String {
    // SendingTime (52): YYYYMMDD-HH:MM:SS in UTC.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let days = secs / 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let rem = secs % 86_400;
    format!(
        "{year:04}{month:02}{day:02}-{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Civil date from days since epoch (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Builds an application-level body (tags 35/49/56/34/52 + payload).
fn build_body(
    msg_type: &str,
    sender: &str,
    target: &str,
    seq: u64,
    fields: &[(u32, String)],
) -> String {
    let mut body = String::new();
    for (tag, value) in fields {
        body.push_str(&format!("{tag}={value}{SOH}"));
    }
    format!(
        "35={msg_type}{SOH}49={sender}{SOH}56={target}{SOH}34={seq}{SOH}52={}{SOH}{}",
        timestamp_now(),
        body
    )
}

fn field(fields: &[(u32, String)], tag: u32) -> Option<&str> {
    fields
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(_, v)| v.as_str())
}

fn parse_fields(message: &str) -> Result<Vec<(u32, String)>, FixError> {
    message
        .split(SOH)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (tag, value) = part
                .split_once('=')
                .ok_or_else(|| FixError::Malformed(format!("field without '=': {part}")))?;
            let tag: u32 = tag
                .parse()
                .map_err(|_| FixError::Malformed(format!("invalid tag {tag}")))?;
            Ok((tag, value.to_string()))
        })
        .collect()
}

fn checksum(body: &str) -> u32 {
    body.bytes().map(|b| b as u32).sum::<u32>() % 256
}

fn frame(body: &str) -> String {
    let body_length = body.len();
    let head = format!("8=FIX.4.2{SOH}9={body_length}{SOH}");
    let message = format!("{head}{body}");
    format!("{message}10={:03}{SOH}", checksum(&message))
}

fn poll_frame(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Result<Option<String>, FixError> {
    if let Some(message) = take_frame(buffer)? {
        return Ok(Some(message));
    }
    let mut chunk = [0u8; 4096];
    let read = match stream.read(&mut chunk) {
        Ok(read) => read,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::Interrupted
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    if read == 0 {
        return Err(FixError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "FIX connection closed",
        )));
    }
    buffer.extend_from_slice(&chunk[..read]);
    if buffer.len() > 16 * 1024 {
        return Err(FixError::Malformed("FIX frame exceeds 16 KiB".into()));
    }
    take_frame(buffer)
}

fn take_frame(buffer: &mut Vec<u8>) -> Result<Option<String>, FixError> {
    find_frame_end(buffer)
        .map(|end| {
            String::from_utf8(buffer.drain(..end).collect())
                .map_err(|_| FixError::Malformed("non-UTF8 frame".into()))
        })
        .transpose()
}

fn find_frame_end(buffer: &[u8]) -> Option<usize> {
    // The checksum field is the last field: "10=XXX<SOH>".
    let tail_len = "10=000\u{1}".len();
    if buffer.len() < tail_len {
        return None;
    }
    buffer
        .windows(tail_len)
        .enumerate()
        .find(|(offset, field)| {
            *offset > 0
                && buffer[*offset - 1] == SOH as u8
                && field.starts_with(b"10=")
                && field[3..6].iter().all(u8::is_ascii_digit)
                && field[6] == SOH as u8
        })
        .map(|(pos, _)| pos + tail_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RunningSession {
        writer: FixWriter,
        reports: std::sync::mpsc::Receiver<ExecEvent>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for RunningSession {
        fn drop(&mut self) {
            let _ = self.writer.close();
            self.thread.take().unwrap().join().unwrap();
        }
    }

    fn connected_pair(heartbeat: u32) -> (FixInitiator, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TRANSPORT_TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TRANSPORT_TIMEOUT)).unwrap();
            let logon = receive(&mut stream, &mut Vec::new());
            assert_eq!(field(&logon, 35), Some("A"));
            assert_eq!(field(&logon, 34), Some("1"));
            assert_eq!(field(&logon, 108), Some(heartbeat.to_string().as_str()));
            stream
                .write_all(
                    frame(&build_body(
                        "A",
                        "GOX",
                        "ORDERHUB",
                        1,
                        &[(98, "0".into()), (108, heartbeat.to_string())],
                    ))
                    .as_bytes(),
                )
                .unwrap();
            stream
        });
        let initiator =
            FixInitiator::connect(&addr.to_string(), "ORDERHUB", "GOX", heartbeat).unwrap();
        (initiator, peer.join().unwrap())
    }

    fn running_session(heartbeat: u32) -> (RunningSession, TcpStream) {
        let (initiator, peer) = connected_pair(heartbeat);
        let (writer, reports, thread) = initiator.split();
        (
            RunningSession {
                writer,
                reports,
                thread: Some(thread),
            },
            peer,
        )
    }

    fn receive(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Vec<(u32, String)> {
        let deadline = Instant::now() + TRANSPORT_TIMEOUT;
        loop {
            if let Some(message) = poll_frame(stream, buffer).unwrap() {
                return parse_fields(&message).unwrap();
            }
            assert!(Instant::now() < deadline, "FIX response timed out");
        }
    }

    fn peer_message(msg_type: &str, seq: u64, fields: &[(u32, String)]) -> String {
        frame(&build_body(msg_type, "GOX", "ORDERHUB", seq, fields))
    }

    #[test]
    fn configured_session_logs_logon_probe_response_and_malformed_request() {
        let dir = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TRANSPORT_TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TRANSPORT_TIMEOUT)).unwrap();
            let logon = receive(&mut stream, &mut Vec::new());
            assert_eq!(field(&logon, 35), Some("A"));
            stream
                .write_all(peer_message("A", 1, &[(108, "30".into()), (98, "0".into())]).as_bytes())
                .unwrap();
            stream
        });
        let path = dir.path().join("fix.cfg");
        std::fs::write(
            &path,
            include_str!("../../../config/orderhub-fix.cfg")
                .replace(
                    "SocketConnectPort=5001",
                    &format!("SocketConnectPort={port}"),
                )
                .replace("FileLogPath=../logs/fix", "FileLogPath=fix-logs"),
        )
        .unwrap();
        let config = FixSessionConfig::load(path).unwrap();
        let log = FixLog::start(&config).unwrap();
        let mut initiator = FixInitiator::connect_configured(&config, &log).unwrap();
        let mut peer = peer.join().unwrap();
        peer.write_all(peer_message("1", 2, &[(112, "probe-2".into())]).as_bytes())
            .unwrap();
        assert_eq!(
            initiator.next_event().unwrap(),
            ExecEvent::Session("1".into())
        );
        assert_eq!(
            field(&receive(&mut peer, &mut Vec::new()), 112),
            Some("probe-2")
        );
        peer.write_all(peer_message("1", 3, &[]).as_bytes())
            .unwrap();
        assert!(initiator.next_event().is_err());
        log.flush(Duration::from_secs(5)).unwrap();
        let messages = std::fs::read_to_string(
            config
                .logging
                .directory
                .join("FIX.4.2-ORDERHUB-GOX.messages.current.log"),
        )
        .unwrap();
        let events = std::fs::read_to_string(
            config
                .logging
                .directory
                .join("FIX.4.2-ORDERHUB-GOX.event.current.log"),
        )
        .unwrap();
        assert!(messages.contains(" out 8=FIX.4.2") && messages.contains(" in 8=FIX.4.2"));
        assert!(messages.contains("35=0\u{1}") && messages.contains("112=probe-2\u{1}"));
        for expected in [
            "LOGON_SENT",
            "LOGON_ACCEPTED",
            "TEST_REQUEST_RECEIVED test_req_id=probe-2",
            "TEST_REQUEST_ANSWERED test_req_id=probe-2",
            "SESSION_ERROR FIX malformed message: TestRequest missing TestReqID",
            "SESSION_CLOSING",
        ] {
            assert!(
                events.contains(expected),
                "missing event {expected}: {events}"
            );
        }
    }

    #[test]
    fn idle_session_sends_heartbeats_and_echoes_test_request_without_core_pumping() {
        let (mut session, mut peer) = running_session(1);
        let mut buffer = Vec::new();
        let heartbeat = receive(&mut peer, &mut buffer);
        assert_eq!(field(&heartbeat, 35), Some("0"));
        assert_eq!(field(&heartbeat, 34), Some("2"));
        assert_eq!(field(&heartbeat, 112), None);
        peer.write_all(peer_message("1", 2, &[(112, "probe-2".into())]).as_bytes())
            .unwrap();
        let reply = receive(&mut peer, &mut buffer);
        assert_eq!(field(&reply, 35), Some("0"));
        assert_eq!(field(&reply, 112), Some("probe-2"));
        assert_eq!(field(&reply, 34), Some("3"));
        session
            .writer
            .submit_order(
                "O-1",
                "AAPL",
                Side::Buy,
                Decimal::ONE,
                Some(Decimal::from(150)),
            )
            .unwrap();
        let order = receive(&mut peer, &mut buffer);
        assert_eq!(field(&order, 35), Some("D"));
        assert_eq!(field(&order, 34), Some("4"));
        session
            .writer
            .cancel_order("C-1", "O-1", "AAPL", Side::Buy)
            .unwrap();
        let cancel = receive(&mut peer, &mut buffer);
        assert_eq!(field(&cancel, 35), Some("F"));
        assert_eq!(field(&cancel, 34), Some("5"));
        let heartbeat = receive(&mut peer, &mut buffer);
        assert_eq!(field(&heartbeat, 35), Some("0"));
        assert_eq!(field(&heartbeat, 34), Some("6"));
        assert!(matches!(
            session.reports.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn concurrent_orders_and_probe_replies_share_contiguous_outbound_sequence() {
        let (session, mut peer) = running_session(30);
        let mut writer = session.writer.clone();
        let orders = std::thread::spawn(move || {
            for i in 0..32 {
                writer
                    .submit_order(
                        &format!("O-{i}"),
                        "AAPL",
                        Side::Buy,
                        Decimal::ONE,
                        Some(Decimal::from(150)),
                    )
                    .unwrap();
            }
        });
        for i in 0..32 {
            peer.write_all(peer_message("1", i + 2, &[(112, format!("probe-{i}"))]).as_bytes())
                .unwrap();
        }
        let mut buffer = Vec::new();
        let mut probes = Vec::new();
        let mut order_ids = Vec::new();
        for seq in 2..66 {
            let message = receive(&mut peer, &mut buffer);
            assert_eq!(field(&message, 34), Some(seq.to_string().as_str()));
            match field(&message, 35) {
                Some("0") => probes.push(field(&message, 112).unwrap().to_string()),
                Some("D") => order_ids.push(field(&message, 11).unwrap().to_string()),
                other => panic!("unexpected message {other:?}"),
            }
        }
        orders.join().unwrap();
        assert_eq!(
            probes,
            (0..32).map(|i| format!("probe-{i}")).collect::<Vec<_>>()
        );
        assert_eq!(
            order_ids,
            (0..32).map(|i| format!("O-{i}")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn session_traffic_does_not_fill_execution_queue_or_block_probe_replies() {
        let (session, mut peer) = running_session(30);
        let mut messages = String::new();
        for seq in 2..1_202 {
            messages.push_str(&peer_message("0", seq, &[]));
        }
        messages.push_str(&peer_message("1", 1_202, &[(112, "after-flood".into())]));
        peer.write_all(messages.as_bytes()).unwrap();
        let reply = receive(&mut peer, &mut Vec::new());
        assert_eq!(field(&reply, 35), Some("0"));
        assert_eq!(field(&reply, 112), Some("after-flood"));
        assert!(matches!(
            session.reports.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn fragmented_probe_survives_read_timeout_and_logout_closes_writer() {
        let (mut session, mut peer) = running_session(30);
        let probe = peer_message("1", 2, &[(112, "fragmented".into())]);
        let midpoint = probe.len() / 2;
        peer.write_all(&probe.as_bytes()[..midpoint]).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        peer.write_all(&probe.as_bytes()[midpoint..]).unwrap();
        let mut buffer = Vec::new();
        let reply = receive(&mut peer, &mut buffer);
        assert_eq!(field(&reply, 112), Some("fragmented"));
        peer.write_all(peer_message("5", 3, &[(58, "test logout".into())]).as_bytes())
            .unwrap();
        assert_eq!(field(&receive(&mut peer, &mut buffer), 35), Some("5"));
        assert!(matches!(
            session.reports.recv_timeout(TRANSPORT_TIMEOUT),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));
        assert!(
            session
                .writer
                .submit_order("late", "AAPL", Side::Buy, Decimal::ONE, None)
                .is_err()
        );
    }

    #[test]
    fn execution_queue_overflow_closes_transport_instead_of_blocking_session() {
        let (session, mut peer) = running_session(30);
        let mut messages = String::new();
        for seq in 2..1_027 {
            messages.push_str(&peer_message(
                "8",
                seq,
                &[
                    (150, "0".into()),
                    (11, format!("O-{seq}")),
                    (37, format!("V-{seq}")),
                ],
            ));
        }
        peer.write_all(messages.as_bytes()).unwrap();
        let deadline = Instant::now() + TRANSPORT_TIMEOUT;
        while !session.thread.as_ref().unwrap().is_finished() {
            assert!(
                Instant::now() < deadline,
                "reader blocked on a full execution queue"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.reports.try_iter().count(), 1_024);
        assert!(matches!(
            session.reports.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        assert!(session.writer.send_app("0", &[]).is_err());
    }

    #[test]
    fn unsplit_initiator_also_answers_test_request() {
        let (mut initiator, mut peer) = connected_pair(30);
        peer.write_all(peer_message("1", 2, &[(112, "direct".into())]).as_bytes())
            .unwrap();
        assert_eq!(
            initiator.next_event().unwrap(),
            ExecEvent::Session("1".into())
        );
        let reply = receive(&mut peer, &mut Vec::new());
        assert_eq!(field(&reply, 35), Some("0"));
        assert_eq!(field(&reply, 112), Some("direct"));
    }

    #[test]
    fn test_coalesced_fix_frames_split_without_swallowing_the_second_report() {
        let first = frame(&format!("35=8{SOH}150=0{SOH}11=O-1{SOH}37=V1{SOH}"));
        let second = frame(&format!(
            "35=8{SOH}150=2{SOH}11=O-1{SOH}37=V1{SOH}17=T1{SOH}32=1{SOH}31=10{SOH}14=1{SOH}"
        ));
        let mut buffer = format!("{first}{second}").into_bytes();
        let end = find_frame_end(&buffer).unwrap();
        assert_eq!(end, first.len());
        let decoded = String::from_utf8(buffer.drain(..end).collect()).unwrap();
        assert!(matches!(
            decode_execution_report(&parse_fields(&decoded).unwrap()).unwrap(),
            ExecEvent::New { .. }
        ));
        assert_eq!(find_frame_end(&buffer), Some(second.len()));
        assert!(
            matches!(decode_execution_report(&parse_fields(&String::from_utf8(buffer).unwrap()).unwrap()).unwrap(), ExecEvent::Fill { exec_id, .. } if exec_id == "T1")
        );
    }

    #[test]
    fn timestamp_is_valid_fix_utc() {
        let ts = timestamp_now();
        println!("TS={ts}");
        assert_eq!(ts.len(), 17, "got {ts}");
        assert!(ts.as_bytes()[8] == b'-', "got {ts}");
    }

    #[test]
    fn frame_roundtrip_parses_fields_and_checksum() {
        let body = format!("35=0{SOH}49=ORDERHUB{SOH}56=EXCHANGE{SOH}");
        let framed = frame(&body);
        let fields = parse_fields(&framed).unwrap();
        assert_eq!(field(&fields, 35), Some("0"));
        assert_eq!(field(&fields, 49), Some("ORDERHUB"));
        let check = field(&fields, 10).unwrap();
        let pos = framed.rfind("10=").expect("checksum field");
        assert_eq!(
            check,
            format!("{:03}", checksum(&framed[..pos])),
            "checksum covers everything before tag 10"
        );
    }

    #[test]
    fn submit_order_body_matches_exchange_profile() {
        let body = build_body(
            "D",
            "ORDERHUB",
            "EXCHANGE",
            5,
            &[
                (21, "1".into()),
                (11, "O-1".into()),
                (55, "AAPL".into()),
                (54, "1".into()),
                (38, "10".into()),
                (40, "2".into()),
                (44, "150.00".into()),
            ],
        );
        assert!(body.contains("21=1\u{1}"), "HandlInst=1 required");
        assert!(body.contains("49=ORDERHUB\u{1}"));
        assert!(body.contains("56=EXCHANGE\u{1}"));
        assert!(body.contains("34=5\u{1}"), "sequence number in body");
        assert!(body.contains("40=2\u{1}") && body.contains("44=150.00\u{1}"));
    }

    #[test]
    fn execution_report_decode_covers_lifecycle() {
        let report = format!(
            "8=FIX.4.2{SOH}9=1{SOH}35=8{SOH}37=V1{SOH}17=E1{SOH}11=O-1{SOH}150=2{SOH}32=10{SOH}31=150.25{SOH}14=10{SOH}"
        );
        let fields = parse_fields(&report).unwrap();
        match decode_execution_report(&fields).unwrap() {
            ExecEvent::Fill {
                cl_ord_id,
                order_id,
                complete,
                last_qty,
                last_px,
                cum_qty,
                exec_id,
            } => {
                assert_eq!(exec_id, "E1");
                assert!(complete);
                assert_eq!(cl_ord_id, "O-1");
                assert_eq!(order_id, "V1");
                assert_eq!(last_qty, Decimal::from(10));
                assert_eq!(last_px, rust_decimal::Decimal::new(15025, 2));
                assert_eq!(cum_qty, Decimal::from(10));
            }
            other => panic!("expected fill, got {other:?}"),
        }
    }

    #[test]
    fn execution_report_cancel_maps_orig_cl_ord_id() {
        // Cancel reports: tag 11 is the fresh cancel request ID, tag 41 the
        // original order; the event must key on the original.
        let report =
            format!("8=FIX.4.2{SOH}9=1{SOH}35=8{SOH}37=V1{SOH}41=O-1{SOH}11=C-7{SOH}150=4{SOH}");
        let fields = parse_fields(&report).unwrap();
        match decode_execution_report(&fields).unwrap() {
            ExecEvent::Canceled {
                cl_ord_id,
                order_id,
            } => {
                assert_eq!(cl_ord_id, "O-1");
                assert_eq!(order_id, "V1");
            }
            other => panic!("expected cancel, got {other:?}"),
        }
    }
}

/// Writer half of an initiator, sharing socket serialization with session traffic.
#[derive(Debug, Clone)]
pub struct FixWriter {
    state: Arc<Mutex<FixWriteState>>,
    log: FixLog,
}

#[derive(Debug)]
struct FixWriteState {
    stream: TcpStream,
    sender_comp_id: String,
    target_comp_id: String,
    outbound_seq: u64,
    last_sent: Instant,
    closed: bool,
    log: FixLog,
}

impl FixWriter {
    /// Closes both socket directions, including the cloned reader socket.
    ///
    /// # Errors
    /// Returns the operating system shutdown error.
    pub fn close(&self) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if !state.closed {
            self.log.event("SESSION_CLOSING local transport shutdown");
        }
        state.closed = true;
        state.stream.shutdown(std::net::Shutdown::Both)
    }
    /// Submits a new order (35=D) per the exchange profile.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent.
    pub fn submit_order(
        &mut self,
        cl_ord_id: &str,
        symbol: &str,
        side: Side,
        quantity: Decimal,
        price: Option<Decimal>,
    ) -> Result<(), FixError> {
        let order_type = if price.is_some() { "2" } else { "1" };
        let mut fields = vec![
            (21, "1".to_string()),
            (11, cl_ord_id.to_string()),
            (55, symbol.to_string()),
            (54, side.tag().to_string()),
            (60, timestamp_now()),
            (38, quantity.to_string()),
            (40, order_type.to_string()),
            (59, "0".to_string()),
        ];
        if let Some(px) = price {
            fields.push((44, px.to_string()));
        }
        self.send_app("D", &fields)
    }

    /// Requests cancellation (35=F).
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent.
    pub fn cancel_order(
        &mut self,
        cl_ord_id: &str,
        orig_cl_ord_id: &str,
        symbol: &str,
        side: Side,
    ) -> Result<(), FixError> {
        self.send_app(
            "F",
            &[
                (11, cl_ord_id.to_string()),
                (41, orig_cl_ord_id.to_string()),
                (55, symbol.to_string()),
                (54, side.tag().to_string()),
                (60, timestamp_now()),
            ],
        )
    }

    fn send_app(&self, msg_type: &str, fields: &[(u32, String)]) -> Result<(), FixError> {
        self.state
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .send(msg_type, fields)
    }

    fn heartbeat_if_idle(&self, interval: Duration) -> Result<(), FixError> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if state.closed || state.last_sent.elapsed() >= interval {
            state.send("0", &[])?;
        }
        Ok(())
    }
}

impl FixWriteState {
    fn send(&mut self, msg_type: &str, fields: &[(u32, String)]) -> Result<(), FixError> {
        if self.closed {
            return Err(FixError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "FIX session closed",
            )));
        }
        let seq = self.outbound_seq;
        self.outbound_seq += 1;
        let body = build_body(
            msg_type,
            &self.sender_comp_id,
            &self.target_comp_id,
            seq,
            fields,
        );
        let message = frame(&body);
        if let Err(e) = self
            .stream
            .write_all(message.as_bytes())
            .and_then(|()| self.stream.flush())
        {
            self.log
                .event(format!("WRITE_FAILED type={msg_type} sequence={seq} {e}"));
            self.closed = true;
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
            return Err(e.into());
        }
        self.last_sent = Instant::now();
        self.log.outgoing(&message);
        match msg_type {
            "A" => self.log.event(format!("LOGON_SENT sequence={seq}")),
            "0" => self.log.event(format!(
                "HEARTBEAT_SENT sequence={seq} test_req_id={}",
                field(fields, 112).unwrap_or("")
            )),
            "5" => self.log.event(format!("LOGOUT_SENT sequence={seq}")),
            _ => {}
        }
        Ok(())
    }
}

impl FixInitiator {
    /// Splits a logged-in initiator into a writer and a background reader
    /// thread servicing heartbeats and delivering execution events until closure.
    ///
    /// Session traffic stays in the reader. Execution queue overflow closes the
    /// transport so the core can halt instead of silently losing a report.
    #[must_use]
    pub fn split(
        self,
    ) -> (
        FixWriter,
        std::sync::mpsc::Receiver<ExecEvent>,
        std::thread::JoinHandle<()>,
    ) {
        let Self { mut reader, writer } = self;
        let (tx, rx) = std::sync::mpsc::sync_channel::<ExecEvent>(1_024);
        let handle = std::thread::Builder::new()
            .name("orderhub-fix-reader".to_string())
            .spawn(move || {
                loop {
                    match reader.next_event() {
                        Ok(ExecEvent::Session(_)) => {}
                        Ok(event) => {
                            if let Err(e) = tx.try_send(event) {
                                reader
                                    .writer
                                    .log
                                    .event(format!("EXECUTION_QUEUE_UNAVAILABLE {e}"));
                                eprintln!("FIX_READER_DIED=execution queue unavailable: {e}");
                                return;
                            }
                        }
                        Err(err) => {
                            eprintln!("FIX_READER_DIED={err:?}");
                            return;
                        }
                    }
                }
            })
            .expect("spawn FIX reader thread");
        (writer, rx, handle)
    }
}

/// Frame reader and heartbeat service used by the background thread.
#[derive(Debug)]
struct FixReader {
    stream: TcpStream,
    buffer: Vec<u8>,
    writer: FixWriter,
    heartbeat_interval: Duration,
}

impl Drop for FixReader {
    fn drop(&mut self) {
        let _ = self.writer.close();
    }
}

impl FixReader {
    fn next_event(&mut self) -> Result<ExecEvent, FixError> {
        let result = self.read_event();
        if let Err(e) = &result {
            self.writer.log.event(format!("SESSION_ERROR {e}"));
        }
        result
    }

    fn read_event(&mut self) -> Result<ExecEvent, FixError> {
        let message = loop {
            self.writer.heartbeat_if_idle(self.heartbeat_interval)?;
            if let Some(message) = poll_frame(&mut self.stream, &mut self.buffer)? {
                break message;
            }
        };
        self.writer.log.incoming(&message);
        let fields = parse_fields(&message)?;
        let msg_type =
            field(&fields, 35).ok_or_else(|| FixError::Malformed("no MsgType".into()))?;
        Ok(match msg_type {
            "0" => {
                self.writer.log.event(format!(
                    "HEARTBEAT_RECEIVED test_req_id={}",
                    field(&fields, 112).unwrap_or("")
                ));
                ExecEvent::Session(msg_type.to_string())
            }
            "1" => {
                let test_req_id = field(&fields, 112)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| FixError::Malformed("TestRequest missing TestReqID".into()))?;
                self.writer
                    .log
                    .event(format!("TEST_REQUEST_RECEIVED test_req_id={test_req_id}"));
                self.writer
                    .send_app("0", &[(112, test_req_id.to_string())])?;
                self.writer
                    .log
                    .event(format!("TEST_REQUEST_ANSWERED test_req_id={test_req_id}"));
                ExecEvent::Session(msg_type.to_string())
            }
            "5" => {
                self.writer.log.event(format!(
                    "LOGOUT_RECEIVED {}",
                    field(&fields, 58).unwrap_or("no text")
                ));
                self.writer.send_app("5", &[])?;
                return Err(FixError::Rejected(format!(
                    "logged out: {}",
                    field(&fields, 58).unwrap_or("no text")
                )));
            }
            "8" => decode_execution_report(&fields)?,
            "3" => {
                self.writer.log.event(format!(
                    "SESSION_REJECT {}",
                    field(&fields, 58).unwrap_or("unspecified")
                ));
                ExecEvent::Rejected {
                    cl_ord_id: field(&fields, 11).unwrap_or_default().to_string(),
                    reason: field(&fields, 58).unwrap_or("unspecified").to_string(),
                }
            }
            other => ExecEvent::Session(other.to_string()),
        })
    }
}
