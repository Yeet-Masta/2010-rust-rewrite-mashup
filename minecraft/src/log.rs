//! The game's log: each line goes to the console and to `logs/latest.log`
//! in the data folder, as vanilla keeps one, with the last session's kept as
//! `logs/previous.log`. A crash leaves its report, with a backtrace, in
//! `crash-reports`.
//!
//! The console is written from a thread of its own: Windows' console stops
//! a writer while text in it is being selected, and the game shouldn't stop
//! with it.
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static CONSOLE: OnceLock<Sender<String>> = OnceLock::new();

/// Starts the log in `data`, and the crash report for a panic.
pub fn open(data: &Path) {
    let dir = data.join("logs");
    let latest = dir.join("latest.log");
    let _ = std::fs::rename(&latest, dir.join("previous.log"));
    if std::fs::create_dir_all(&dir).is_ok()
        && let Ok(file) = std::fs::File::create(&latest)
    {
        let _ = FILE.set(Mutex::new(file));
    }
    let (send, receive) = std::sync::mpsc::channel::<String>();
    if std::thread::Builder::new()
        .name("Log".into())
        .spawn(move || {
            for text in receive {
                eprintln!("{text}");
            }
        })
        .is_ok()
    {
        let _ = CONSOLE.set(send);
    }
    let reports = data.join("crash-reports");
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_owned();
        line_now(&format!("The game crashed on the {name} thread: {info}"));
        // Each crash its own report, so a second one doesn't hide the first.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let file = name.replace(|c: char| !c.is_ascii_alphanumeric(), "-");
        let report = reports.join(format!("crash-{stamp}-{file}.txt"));
        if std::fs::create_dir_all(&reports).is_ok()
            && std::fs::write(&report, format!("{name} thread: {info}\n\n{backtrace}")).is_ok()
        {
            line_now(&format!("The crash report is in {}", report.display()));
        }
        // Started from Explorer, the console would close before it is read.
        #[cfg(windows)]
        if name == "main" {
            eprintln!("Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
    }));
}

/// A line for the console and the log file.
pub fn line(text: &str) {
    if CONSOLE
        .get()
        .is_none_or(|console| console.send(text.to_owned()).is_err())
    {
        eprintln!("{text}");
    }
    write_file(text);
}

/// A line written to the console before this returns, for the last words
/// before the program ends.
pub fn line_now(text: &str) {
    eprintln!("{text}");
    write_file(text);
}

fn write_file(text: &str) {
    if let Some(file) = FILE.get()
        && let Ok(mut file) = file.lock()
    {
        let _ = writeln!(file, "{text}");
    }
}
