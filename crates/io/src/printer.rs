//! Talking to a printer over a serial line, as a Marlin-style host.
//!
//! One command is in flight at a time: the host sends a line and waits for the
//! firmware's `ok` before the next. While printing, lines carry numbers and a
//! checksum (`N12 G1 X10*85`) so the firmware can ask for a line again
//! (`Resend: 12`). Temperatures are asked for with `M105` when the line is
//! idle and read from every reply that carries them.
//!
//! The protocol runs on its own thread over any `Read + Write` link, so tests
//! drive it with a simulated printer; `connect` opens a real serial port.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Longest line read from the printer; longer input is cut.
const MAX_LINE: usize = 4096;
/// Ask for temperatures this often while idle.
const POLL: Duration = Duration::from_secs(2);
/// Most lines kept for a resend.
const HISTORY: usize = 256;

/// What the window or an agent asks of the printer.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCommand {
    /// Send one line now (terminal, jog, temperature); queued before a print's lines.
    Send(String),
    /// Print these G-code lines, in order.
    Print(Vec<String>),
    Pause,
    Resume,
    Cancel,
    Disconnect,
}

/// What the printer said or did.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// A line from the printer (or "> sent" echoes of what was sent).
    Line(String),
    Temperatures {
        nozzle: f64,
        nozzle_target: f64,
        bed: f64,
        bed_target: f64,
    },
    Progress {
        done: usize,
        total: usize,
    },
    PrintFinished,
    Error(String),
    Disconnected,
}

/// The firmware's checksum: XOR of every byte before the `*`.
pub fn checksum(line: &str) -> u8 {
    line.bytes().fold(0u8, |a, b| a ^ b)
}

/// `T:200.0 /210.0 B:60.0 /60.0` → (nozzle, nozzle target, bed, bed target).
pub fn parse_temperatures(line: &str) -> Option<(f64, f64, f64, f64)> {
    let mut out = (None, None, None, None);
    let words: Vec<&str> = line.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let target = || {
            words.get(i + 1).and_then(|t| t.strip_prefix('/')).and_then(|t| t.parse::<f64>().ok())
        };
        if let Some(v) = w.strip_prefix("T:").and_then(|v| v.parse::<f64>().ok()) {
            if out.0.is_none() {
                out.0 = Some(v);
                out.1 = target();
            }
        } else if let Some(v) = w.strip_prefix("B:").and_then(|v| v.parse::<f64>().ok()) {
            out.2 = Some(v);
            out.3 = target();
        }
    }
    let nozzle = out.0?;
    Some((nozzle, out.1.unwrap_or(0.0), out.2.unwrap_or(0.0), out.3.unwrap_or(0.0)))
}

/// G-code text as the lines a printer needs: comments and blanks dropped.
pub fn printable_lines(gcode: &str) -> Vec<String> {
    gcode
        .lines()
        .map(|l| l.split(';').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// A running connection: send commands, drain events.
pub struct Connection {
    tx: Sender<HostCommand>,
    rx: Receiver<HostEvent>,
    pub port: String,
}

impl Connection {
    pub fn send(&self, cmd: HostCommand) -> bool {
        self.tx.send(cmd).is_ok()
    }

    /// Every event since the last call.
    pub fn events(&self) -> Vec<HostEvent> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(e) => out.push(e),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    out.push(HostEvent::Disconnected);
                    break;
                }
            }
        }
        out
    }
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Connection({})", self.port)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.tx.send(HostCommand::Disconnect);
    }
}

/// Serial ports this machine has.
pub fn list_ports() -> Vec<String> {
    serial2::SerialPort::available_ports()
        .map(|v| v.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Open `port` at `baud` and start the host.
pub fn connect(port: &str, baud: u32) -> Result<Connection, String> {
    let mut serial =
        serial2::SerialPort::open(port, baud).map_err(|e| format!("cannot open {port}: {e}"))?;
    serial.set_read_timeout(Duration::from_millis(50)).map_err(|e| e.to_string())?;
    Ok(spawn_host(serial, port.to_string()))
}

/// Start the host thread over any link.
pub fn spawn_host<L: Read + Write + Send + 'static>(link: L, name: String) -> Connection {
    let (tx_cmd, rx_cmd) = channel();
    let (tx_ev, rx_ev) = channel();
    let _ =
        std::thread::Builder::new().name("printer".into()).spawn(move || host(link, rx_cmd, tx_ev));
    Connection { tx: tx_cmd, rx: rx_ev, port: name }
}

struct Job {
    lines: Vec<String>,
    next: usize,
    paused: bool,
}

fn host<L: Read + Write>(mut link: L, rx: Receiver<HostCommand>, tx: Sender<HostEvent>) {
    let mut manual: VecDeque<String> = VecDeque::new();
    let mut job: Option<Job> = None;
    let mut line_no: u64 = 0;
    let mut history: VecDeque<(u64, String)> = VecDeque::new();
    let mut awaiting = true;
    let mut last_send = Instant::now();
    let mut last_poll = Instant::now();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    // the first line resets the firmware's line numbers; wait for its ok
    let _ = link.write_all(b"M110 N0\n");
    loop {
        // commands from the window
        loop {
            match rx.try_recv() {
                Ok(HostCommand::Send(s)) => manual.push_back(s),
                Ok(HostCommand::Print(lines)) => {
                    let total = lines.len();
                    job = Some(Job { lines, next: 0, paused: false });
                    let _ = tx.send(HostEvent::Progress { done: 0, total });
                }
                Ok(HostCommand::Pause) => {
                    if let Some(j) = &mut job {
                        j.paused = true;
                    }
                }
                Ok(HostCommand::Resume) => {
                    if let Some(j) = &mut job {
                        j.paused = false;
                    }
                }
                Ok(HostCommand::Cancel) => {
                    job = None;
                    // heaters off and fan off, so nothing keeps cooking
                    manual.extend(["M104 S0", "M140 S0", "M107"].map(String::from));
                }
                Ok(HostCommand::Disconnect) | Err(TryRecvError::Disconnected) => {
                    let _ = tx.send(HostEvent::Disconnected);
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        // what the printer says
        match link.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => buf.extend_from_slice(chunk.get(..n).unwrap_or(&[])),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(e) => {
                let _ = tx.send(HostEvent::Error(format!("the printer link failed: {e}")));
                let _ = tx.send(HostEvent::Disconnected);
                return;
            }
        }
        if buf.len() > MAX_LINE * 4 {
            buf.clear();
        }
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = buf.drain(..=pos).collect();
            let text =
                String::from_utf8_lossy(&raw).trim().chars().take(MAX_LINE).collect::<String>();
            if text.is_empty() {
                continue;
            }
            if let Some((n, nt, b, bt)) = parse_temperatures(&text) {
                let _ = tx.send(HostEvent::Temperatures {
                    nozzle: n,
                    nozzle_target: nt,
                    bed: b,
                    bed_target: bt,
                });
            }
            let lower = text.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("resend:").or_else(|| lower.strip_prefix("rs:"))
            {
                // send that numbered line again
                if let Ok(n) = rest.trim().parse::<u64>()
                    && let Some((_, line)) = history.iter().find(|(k, _)| *k == n)
                {
                    let _ = link.write_all(line.as_bytes());
                    last_send = Instant::now();
                }
                continue;
            }
            if lower.starts_with("ok") {
                awaiting = false;
            } else if lower.starts_with("error") || lower.starts_with("!!") {
                let _ = tx.send(HostEvent::Error(text.clone()));
            }
            if !lower.starts_with("ok t:") && lower != "ok" {
                let _ = tx.send(HostEvent::Line(text));
            }
        }
        // the next line, when the firmware is ready
        if !awaiting {
            let next = if let Some(m) = manual.pop_front() {
                Some((m, true))
            } else if let Some(j) = job.as_mut().filter(|j| !j.paused) {
                match j.lines.get(j.next).cloned() {
                    Some(l) => {
                        j.next += 1;
                        if j.next % 50 == 0 || j.next == j.lines.len() {
                            let _ =
                                tx.send(HostEvent::Progress { done: j.next, total: j.lines.len() });
                        }
                        Some((l, false))
                    }
                    None => {
                        job = None;
                        let _ = tx.send(HostEvent::PrintFinished);
                        None
                    }
                }
            } else if last_poll.elapsed() >= POLL {
                last_poll = Instant::now();
                Some(("M105".to_string(), false))
            } else {
                None
            };
            if let Some((cmd, echo)) = next {
                line_no += 1;
                let body = format!("N{line_no} {cmd}");
                let wire = format!("{body}*{}\n", checksum(&body));
                if link.write_all(wire.as_bytes()).is_err() {
                    let _ = tx.send(HostEvent::Error("the printer stopped answering".into()));
                    let _ = tx.send(HostEvent::Disconnected);
                    return;
                }
                history.push_back((line_no, wire));
                if history.len() > HISTORY {
                    history.pop_front();
                }
                if echo {
                    let _ = tx.send(HostEvent::Line(format!("> {cmd}")));
                }
                awaiting = true;
                last_send = Instant::now();
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        } else if last_send.elapsed() > Duration::from_secs(120) {
            // a heat-and-wait can take minutes; past two, say so once
            let _ =
                tx.send(HostEvent::Error("the printer has not answered for two minutes".into()));
            last_send = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A simulated printer: answers every numbered line with `ok`, temperatures
    /// to M105, and asks once for line 3 again.
    #[derive(Clone)]
    struct FakePrinter {
        inbox: Arc<Mutex<Vec<String>>>,
        outbox: Arc<Mutex<VecDeque<u8>>>,
        resend_asked: Arc<Mutex<bool>>,
    }

    impl FakePrinter {
        fn new() -> Self {
            Self {
                inbox: Arc::new(Mutex::new(Vec::new())),
                outbox: Arc::new(Mutex::new(VecDeque::new())),
                resend_asked: Arc::new(Mutex::new(false)),
            }
        }
        fn said(&self) -> Vec<String> {
            self.inbox.lock().map(|v| v.clone()).unwrap_or_default()
        }
    }

    impl Write for FakePrinter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(b).trim().to_string();
            let reply = if text.contains("M105") {
                "ok T:200.5 /210.0 B:59.0 /60.0\n".to_string()
            } else if text.starts_with("N3 ")
                && !self.resend_asked.lock().map(|g| *g).unwrap_or(true)
            {
                if let Ok(mut g) = self.resend_asked.lock() {
                    *g = true;
                }
                "Error:checksum mismatch\nResend: 3\n".to_string()
            } else {
                "ok\n".to_string()
            };
            if let Ok(mut v) = self.inbox.lock() {
                v.push(text);
            }
            if let Ok(mut o) = self.outbox.lock() {
                o.extend(reply.bytes());
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Read for FakePrinter {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let mut o = self.outbox.lock().map_err(|_| std::io::Error::other("poisoned"))?;
            if o.is_empty() {
                drop(o);
                std::thread::sleep(Duration::from_millis(2));
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"));
            }
            let n = out.len().min(o.len());
            for slot in out.iter_mut().take(n) {
                *slot = o.pop_front().unwrap_or(0);
            }
            Ok(n)
        }
    }

    fn wait_for(conn: &Connection, mut done: impl FnMut(&HostEvent) -> bool) -> Vec<HostEvent> {
        let mut seen = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            for e in conn.events() {
                let stop = done(&e);
                seen.push(e);
                if stop {
                    return seen;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        seen
    }

    #[test]
    fn checksums_and_temperatures_parse() {
        // the firmware documentation example: N3 T0*57
        assert_eq!(checksum("N3 T0"), 57);
        assert_eq!(
            parse_temperatures("ok T:200.5 /210.0 B:59.0 /60.0 @:0"),
            Some((200.5, 210.0, 59.0, 60.0))
        );
        assert_eq!(parse_temperatures("T:25.1 /0.0"), Some((25.1, 0.0, 0.0, 0.0)));
        assert_eq!(parse_temperatures("echo:busy"), None);
        assert_eq!(printable_lines("; head\nG28 ; home\n\nG1 X1\n"), vec!["G28", "G1 X1"]);
    }

    #[test]
    fn a_print_runs_in_order_with_a_resend_and_temperatures() {
        let fake = FakePrinter::new();
        let conn = spawn_host(fake.clone(), "fake".into());
        conn.send(HostCommand::Print(vec![
            "G28".into(),
            "G1 X10".into(),
            "G1 X20".into(),
            "M84".into(),
        ]));
        let events = wait_for(&conn, |e| *e == HostEvent::PrintFinished);
        assert!(events.contains(&HostEvent::PrintFinished), "{events:?}");
        let said = fake.said();
        // every numbered line carries a correct checksum
        for l in said.iter().filter(|l| l.starts_with('N')) {
            let (body, cs) = l.rsplit_once('*').unwrap_or((l, "x"));
            assert_eq!(cs.parse::<u8>().ok(), Some(checksum(body)), "{l}");
        }
        // the lines went in order, and line 3 went twice (the resend)
        let bodies: Vec<String> = said
            .iter()
            .filter_map(|l| {
                l.split_once(' ').map(|(_, r)| r.split('*').next().unwrap_or("").to_string())
            })
            .collect();
        let printed: Vec<&String> = bodies.iter().filter(|b| *b != "M105").collect();
        assert_eq!(
            printed.iter().filter(|b| b.as_str() == "G1 X20").count()
                + printed.iter().filter(|b| b.as_str() == "G1 X10").count(),
            3,
            "{said:?}"
        );
        // a manual command gets the temperatures back
        conn.send(HostCommand::Send("M105".into()));
        let events = wait_for(&conn, |e| matches!(e, HostEvent::Temperatures { .. }));
        assert!(events.iter().any(|e| matches!(e, HostEvent::Temperatures { nozzle, .. } if (*nozzle - 200.5).abs() < 1e-9)));
    }

    #[test]
    fn cancel_turns_the_heaters_off_and_disconnect_ends_the_thread() {
        let fake = FakePrinter::new();
        let conn = spawn_host(fake.clone(), "fake".into());
        conn.send(HostCommand::Pause);
        conn.send(HostCommand::Print((0..100).map(|i| format!("G1 X{i}")).collect()));
        conn.send(HostCommand::Pause);
        conn.send(HostCommand::Cancel);
        std::thread::sleep(Duration::from_millis(300));
        assert!(fake.said().iter().any(|l| l.contains("M104 S0")));
        assert!(fake.said().iter().filter(|l| l.contains("G1 X")).count() < 100);
        conn.send(HostCommand::Disconnect);
        let events = wait_for(&conn, |e| *e == HostEvent::Disconnected);
        assert!(events.contains(&HostEvent::Disconnected));
    }
}
