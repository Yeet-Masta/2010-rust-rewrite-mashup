//! The console window the game starts with on Windows: closing it, or
//! Ctrl+C in it, saves the world before the game ends, as closing the game's
//! own window does.
use std::sync::atomic::{AtomicBool, Ordering};

use winit::event_loop::EventLoopProxy;

static CLOSING: AtomicBool = AtomicBool::new(false);
static SAVED: AtomicBool = AtomicBool::new(false);

/// Whether the console asked the game to end.
pub fn closing() -> bool {
    CLOSING.load(Ordering::SeqCst)
}

/// The world is saved: the console may end the game.
pub fn saved() {
    SAVED.store(true, Ordering::SeqCst);
}

/// Has the console's close and Ctrl+C wake the window loop to save.
#[cfg(windows)]
pub fn watch(wake: EventLoopProxy<()>) {
    use std::sync::{Mutex, OnceLock};

    static WAKE: OnceLock<Mutex<EventLoopProxy<()>>> = OnceLock::new();

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    unsafe extern "system" fn handler(_event: u32) -> i32 {
        CLOSING.store(true, Ordering::SeqCst);
        if let Some(wake) = WAKE.get()
            && let Ok(wake) = wake.lock()
        {
            let _ = wake.send_event(());
        }
        // Windows ends the game when this returns, and gives it five
        // seconds: wait that long for the save.
        for _ in 0..90 {
            if SAVED.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        1
    }

    let _ = WAKE.set(Mutex::new(wake));
    // SAFETY: `handler` is a plain function that lives for the program.
    unsafe {
        SetConsoleCtrlHandler(Some(handler), 1);
    }
}

#[cfg(not(windows))]
pub fn watch(_: EventLoopProxy<()>) {}
