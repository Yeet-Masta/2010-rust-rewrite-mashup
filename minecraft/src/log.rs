//! The game's log: each line goes to the console and to `logs/latest.log`
//! in the data folder, as vanilla keeps one. A crash leaves its report, with
//! a backtrace, in `crash-report.txt` beside it.
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

/// Starts the log in `data`, and the crash report for a panic.
pub fn open(data: &Path) {
    let dir = data.join("logs");
    if std::fs::create_dir_all(&dir).is_ok()
        && let Ok(file) = std::fs::File::create(dir.join("latest.log"))
    {
        let _ = FILE.set(Mutex::new(file));
    }
    let report = data.join("crash-report.txt");
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        line(&format!("The game crashed: {info}"));
        if std::fs::write(&report, format!("{info}\n\n{backtrace}")).is_ok() {
            line(&format!("The crash report is in {}", report.display()));
        }
    }));
}

pub fn line(text: &str) {
    eprintln!("{text}");
    if let Some(file) = FILE.get()
        && let Ok(mut file) = file.lock()
    {
        let _ = writeln!(file, "{text}");
    }
}
