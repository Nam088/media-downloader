use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Caps memory use for a long-running app instead of growing forever —
/// plenty of scrollback for "what just happened" without needing to persist
/// logs to disk.
const MAX_LOG_ENTRIES: usize = 500;

/// User-facing debug log, surfaced in the frontend's Logs page. This exists
/// because most of this app's actual failure modes (TikTok/gallery-dl
/// transient blocking, retries, a fallback silently choosing not to kick in)
/// are invisible in a packaged production build — there is no terminal for
/// `eprintln!` to reach once the app isn't running under `pnpm tauri dev`.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[derive(Default)]
pub struct LogBuffer(Mutex<VecDeque<LogEntry>>);

impl LogBuffer {
    fn push(&self, entry: LogEntry) {
        let mut buf = self.0.lock().expect("log buffer mutex poisoned");
        if buf.len() >= MAX_LOG_ENTRIES {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.0.lock().expect("log buffer mutex poisoned").iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.0.lock().expect("log buffer mutex poisoned").clear();
    }
}

/// Records one log line: appended to the in-memory buffer, emitted live as a
/// `log:new_entry` event (so an open Logs page updates without polling), and
/// still printed to stderr for `pnpm tauri dev`'s terminal as before.
pub fn log_event(app: &AppHandle, level: &str, message: impl Into<String>) {
    let message = message.into();
    eprintln!("[{level}] {message}");

    let entry = LogEntry {
        timestamp: Utc::now().to_rfc3339(),
        level: level.to_string(),
        message,
    };

    if let Some(buffer) = app.try_state::<LogBuffer>() {
        buffer.push(entry.clone());
    }
    let _ = app.emit("log:new_entry", entry);
}

#[tauri::command]
pub fn get_logs(logs: tauri::State<'_, LogBuffer>) -> Vec<LogEntry> {
    logs.snapshot()
}

#[tauri::command]
pub fn clear_logs(logs: tauri::State<'_, LogBuffer>) {
    logs.clear();
}
