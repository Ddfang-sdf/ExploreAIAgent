use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;

pub struct RawModeGuard;

impl RawModeGuard {
    pub fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        Ok(RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Drain any pending terminal events (e.g. keys pressed during agent execution).
pub fn drain_events() {
    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let _ = event::read();
    }
}

/// Read a line in raw mode. Enter submits; Ctrl+Enter / Ctrl+J inserts newline.
pub fn read_line_raw(prompt: &str, mut writer: impl Write) -> io::Result<String> {
    write!(writer, "{}", prompt)?;
    writer.flush()?;

    let mut buffer = String::new();

    loop {
        match event::read()? {
            Event::Key(key) => {
                match key.code {
                    KeyCode::Enter => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            buffer.push('\n');
                            write!(writer, "\r\n")?;
                        } else {
                            write!(writer, "\r\n")?;
                            return Ok(buffer);
                        }
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            match c {
                                'c' | 'C' => {
                                    write!(writer, "^C\r\n")?;
                                    return Err(io::Error::new(io::ErrorKind::Interrupted, "Ctrl+C"));
                                }
                                'j' | 'J' => {
                                    buffer.push('\n');
                                    write!(writer, "\r\n")?;
                                }
                                _ => {}
                            }
                        } else {
                            buffer.push(c);
                            write!(writer, "{}", c)?;
                        }
                    }
                    KeyCode::Backspace => {
                        if !buffer.is_empty() {
                            buffer.pop();
                            write!(writer, "\x08 \x08")?;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        writer.flush()?;
    }
}

/// Spawn a background thread that polls for ESC key and sets `cancel` flag.
/// Returns a JoinHandle and a running flag — set running=false to stop.
pub fn spawn_esc_listener(cancel: Arc<AtomicBool>) -> (thread::JoinHandle<()>, Arc<AtomicBool>) {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let handle = thread::spawn(move || {
        while r.load(Ordering::Relaxed) {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    match key.code {
                        KeyCode::Esc => {
                            cancel.store(true, Ordering::SeqCst);
                        }
                        KeyCode::Char('c') | KeyCode::Char('C')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            cancel.store(true, Ordering::SeqCst);
                        }
                        _ => {}
                    }
                }
            }
        }
    });
    (handle, running)
}
