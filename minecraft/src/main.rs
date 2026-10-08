//! Minecraft on its own: a MinecraftOSS world in a window, played as
//! vanilla survival or creative. Minecraft's own files are fetched from
//! Mojang on the first run; nothing of Mojang's ships with the program.
/// A line for the console and the log file.
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log::line(&format!($($arg)*))
    };
}

mod ambient;
mod console;
mod creative;
mod emitters;
mod entities;
mod font;
mod game;
mod gui;
mod hand;
mod inventory_player;
mod log;
mod mining;
mod particles;
mod placement;
mod render;
mod save;
mod setup;
mod sounds;
mod target;
mod world;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use game::{Game, Input, Key, Options};

const USAGE: &str = "\
Usage: minecraft [options]

  --world NAME          the saved world to play (default: world)
  --seed SEED           the seed of a new world: a number, or any text
  --creative            creative mode: flying, instant breaking
  --view-distance N     chunks to render around the player (default: 8)
  --time TICKS          the time of day to start at (1000 morning, 6000 noon,
                        13000 dusk, 18000 midnight)
  --temporary           play a fresh world that is not saved
  --no-vsync            draw frames without waiting for the display (up to
                        120 a second), as vanilla's VSync setting turned off
  --data DIR            where Minecraft's files and saves live (default:
                        minecraft-data where the game is run if it is there,
                        else next to the program)
  --screenshot FILE     save a screenshot once the world has loaded, and quit

Controls: WASD move, mouse look, Space jump (twice to fly in creative),
Shift sneak, Ctrl or W twice sprint, left click mine and attack, right click
place and use, middle click pick block, 1-9 or the wheel select, E inventory,
Q drop, F1 hide the HUD, F2 screenshot, F3 debug, F11 fullscreen, Esc menu.";

enum Phase {
    Loading(std::sync::mpsc::Receiver<Result<world::World, String>>),
    Playing(Box<Game>),
    Failed(String),
}

struct App {
    options: Options,
    root: PathBuf,
    data: PathBuf,
    capture: Option<PathBuf>,
    window: Option<Arc<Window>>,
    renderer: Option<render::Renderer>,
    gui: Option<gui::Gui>,
    phase: Option<Phase>,
    input: Input,
    last: Instant,
    /// Seconds played, for a screenshot once the world has settled.
    played: f64,
    grabbed: bool,
    focused: bool,
    /// Why the game could not start, once the window loop has ended.
    failed: Option<String>,
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let attributes = Window::default_attributes()
            .with_title("Minecraft")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let mut renderer = render::Renderer::new(window.clone(), self.options.vsync)?;
        let packs =
            minecraft_terrain::pack::PackStack::open(vec![self.root.join(setup::RESOURCE_PACK)])?;
        self.gui = Some(gui::Gui::load(&packs, &mut renderer)?);
        let (send, receive) = std::sync::mpsc::channel();
        let (root, seed, view_distance, save) = (
            self.root.clone(),
            self.options.seed,
            self.options.view_distance,
            self.options.save.clone(),
        );
        std::thread::Builder::new()
            .name("world-load".into())
            .spawn(move || {
                let _ = send.send(world::World::load(
                    &root,
                    seed,
                    view_distance,
                    save.as_deref(),
                ));
            })?;
        self.phase = Some(Phase::Loading(receive));
        self.renderer = Some(renderer);
        self.window = Some(window);
        Ok(())
    }

    fn grab(&mut self, grab: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if grab == self.grabbed {
            return;
        }
        // A window in the background is grabbed once it is in front.
        if grab && !self.focused {
            return;
        }
        self.grabbed = grab;
        let size = window.inner_size();
        let center = winit::dpi::PhysicalPosition::new(size.width / 2, size.height / 2);
        if grab {
            // Locked where the platform has it (Windows only confines), with
            // the hidden cursor in the middle so a click stays in the window.
            let _ = window.set_cursor_position(center);
            let _ = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            window.set_cursor_visible(false);
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
            // Vanilla's menus open with the cursor in the middle, but a
            // window in the background leaves the cursor where it is.
            if self.focused {
                let _ = window.set_cursor_position(center);
                self.input.mouse = (center.x as f32, center.y as f32);
            }
        }
    }

    /// Saves and ends the game.
    fn close(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(Phase::Playing(mut game)) = self.phase.take() {
            game.shutdown();
            // The world's chunks and mobs are written as it goes.
            drop(game);
        }
        console::saved();
        event_loop.exit();
    }

    fn key(&mut self, code: KeyCode, pressed: bool, repeat: bool) {
        let input = &mut self.input;
        match code {
            KeyCode::KeyW => input.forward = pressed,
            KeyCode::KeyS => input.back = pressed,
            KeyCode::KeyA => input.left = pressed,
            KeyCode::KeyD => input.right = pressed,
            KeyCode::Space => {
                if pressed && !input.jump {
                    input.jump_taps += 1;
                }
                input.jump = pressed;
            }
            KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                input.shift = pressed;
                if code == KeyCode::ShiftLeft {
                    input.sneak = pressed;
                }
            }
            KeyCode::ControlLeft | KeyCode::ControlRight => {
                input.ctrl = pressed;
                if code == KeyCode::ControlLeft {
                    input.sprint = pressed;
                }
            }
            // The hotbar save and load activators.
            KeyCode::KeyC => input.save_hotbar = pressed,
            KeyCode::KeyX => input.load_hotbar = pressed,
            _ => {}
        }
        if !pressed || repeat && !matches!(code, KeyCode::KeyQ | KeyCode::Backspace) {
            return;
        }
        let key = match code {
            KeyCode::KeyE => Key::Inventory,
            KeyCode::Escape => Key::Escape,
            KeyCode::KeyQ => Key::Drop,
            KeyCode::KeyW => Key::Forward,
            KeyCode::F1 => Key::HideHud,
            KeyCode::F2 => Key::Screenshot,
            KeyCode::F3 => Key::Debug,
            KeyCode::KeyT => Key::Chat,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::F11 => {
                if let Some(window) = self.window.as_ref() {
                    window.set_fullscreen(if window.fullscreen().is_some() {
                        None
                    } else {
                        Some(Fullscreen::Borderless(None))
                    });
                }
                return;
            }
            KeyCode::Digit1 => Key::Hotbar(0),
            KeyCode::Digit2 => Key::Hotbar(1),
            KeyCode::Digit3 => Key::Hotbar(2),
            KeyCode::Digit4 => Key::Hotbar(3),
            KeyCode::Digit5 => Key::Hotbar(4),
            KeyCode::Digit6 => Key::Hotbar(5),
            KeyCode::Digit7 => Key::Hotbar(6),
            KeyCode::Digit8 => Key::Hotbar(7),
            KeyCode::Digit9 => Key::Hotbar(8),
            _ => return,
        };
        input.keys.push(key);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let dt = self.last.elapsed().as_secs_f64().min(0.25);
        self.last = Instant::now();
        let (Some(renderer), Some(gui)) = (self.renderer.as_mut(), self.gui.as_mut()) else {
            return;
        };
        let phase = self.phase.take();
        let phase = match phase {
            Some(Phase::Loading(receive)) => match receive.try_recv() {
                Ok(Ok(world)) => {
                    renderer.set_world(world.atlas.clone(), &world.celestial, &world.crack_texture);
                    log!("World ready: seed {}", world.seed);
                    Phase::Playing(Box::new(Game::new(world, &self.options)))
                }
                Ok(Err(error)) => {
                    log!("The world could not be loaded: {error}");
                    Phase::Failed(error)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Nothing pressed while loading carries into the game.
                    self.input.keys.clear();
                    self.input.clicks.clear();
                    let mut ui = render::UiList::default();
                    gui.layout(renderer.size(), self.input.mouse);
                    gui.loading_screen(&mut ui, "Generating world...", None);
                    renderer.render(None, &ui, None);
                    Phase::Loading(receive)
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Phase::Failed("the world loader stopped".into())
                }
            },
            Some(Phase::Playing(mut game)) => {
                // Vanilla pauses a game left in the background, including
                // one that finished loading there.
                if !self.focused {
                    game.pause();
                }
                let (draw, ui) = game.frame(dt, &mut self.input, renderer, gui);
                self.input.look = (0.0, 0.0);
                self.input.middle_click = false;
                self.played += dt;
                let screenshot = std::mem::take(&mut game.screenshot).then(|| {
                    let dir = self.data.join("screenshots");
                    let _ = std::fs::create_dir_all(&dir);
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    dir.join(format!("{stamp}.png"))
                });
                // A requested capture, once the terrain around has had time.
                let capture = self
                    .capture
                    .as_ref()
                    .filter(|_| self.played > 25.0)
                    .cloned();
                renderer.render(
                    draw.as_ref(),
                    &ui,
                    screenshot.as_deref().or(capture.as_deref()),
                );
                if capture.is_some() {
                    game.quit = true;
                }
                if game.quit {
                    self.phase = Some(Phase::Playing(game));
                    self.close(event_loop);
                    return;
                }
                let grab = game.captures_mouse();
                self.phase = Some(Phase::Playing(game));
                self.grab(grab);
                return;
            }
            Some(Phase::Failed(error)) => {
                let mut ui = render::UiList::default();
                gui.layout(renderer.size(), self.input.mouse);
                gui.loading_screen(&mut ui, &format!("Could not load the world: {error}"), None);
                renderer.render(None, &ui, None);
                Phase::Failed(error)
            }
            None => return,
        };
        self.phase = Some(phase);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.start(event_loop) {
            self.failed = Some(format!("could not start: {error:#}"));
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.close(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                // The confined area follows the window: grab it again.
                if self.grabbed {
                    self.grab(false);
                }
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if !focused {
                    if let Some(Phase::Playing(game)) = self.phase.as_mut() {
                        game.pause();
                    }
                    self.input = Input::default();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.key(code, pressed, event.repeat);
                }
                // What the key types, for a text box (`charTyped`).
                if pressed && let Some(text) = event.text.as_ref() {
                    self.input.keys.extend(text.chars().map(Key::Char));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.mouse = (position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                // Held only from a press in play: the click on a menu's
                // button that resumes the game doesn't go on to mine.
                let playing =
                    matches!(&self.phase, Some(Phase::Playing(game)) if game.captures_mouse());
                match button {
                    MouseButton::Left => {
                        self.input.attack = pressed && playing;
                        self.input.clicks.push((false, pressed));
                    }
                    MouseButton::Right => {
                        self.input.use_item = pressed && playing;
                        self.input.clicks.push((true, pressed));
                    }
                    MouseButton::Middle if pressed => self.input.middle_click = true,
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.input.scroll += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.grabbed
        {
            self.input.look.0 += delta.0;
            self.input.look.1 += delta.1;
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        if console::closing() {
            self.close(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if console::closing() {
            self.close(event_loop);
            return;
        }
        // Without vsync, vanilla's default framerate limit.
        if !self.options.vsync {
            let next = self.last + std::time::Duration::from_secs_f64(1.0 / 120.0);
            if Instant::now() < next {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                return;
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

/// Vanilla's seed from the world screen's text: a number as itself,
/// anything else by its `String.hashCode`.
fn parse_seed(text: &str) -> i64 {
    text.trim().parse::<i64>().unwrap_or_else(|_| {
        let hash = text.encode_utf16().fold(0i32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(i32::from(unit))
        });
        i64::from(hash)
    })
}

fn random_seed() -> i64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (nanos as i64) ^ 0x5DEE_CE66_D1CE_4E5B
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut world_name = "world".to_owned();
    let mut vsync = true;
    let (mut seed, mut creative, mut view_distance, mut time, mut temporary) =
        (None, false, 8, None, false);
    let (mut data, mut capture) = (None, None);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| exit_with(&format!("{arg} needs a value")))
        };
        match arg.as_str() {
            "--world" => world_name = value(),
            "--seed" => seed = Some(parse_seed(&value())),
            "--creative" => creative = true,
            "--view-distance" => {
                view_distance = value()
                    .parse::<i32>()
                    .unwrap_or_else(|_| exit_with("--view-distance needs a number"))
                    .clamp(2, 32)
            }
            "--time" => {
                time = Some(
                    value()
                        .parse::<f64>()
                        .unwrap_or_else(|_| exit_with("--time needs a number")),
                )
            }
            "--temporary" => temporary = true,
            "--no-vsync" => vsync = false,
            "--data" => data = Some(PathBuf::from(value())),
            "--screenshot" => capture = Some(PathBuf::from(value())),
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => exit_with(&format!("unknown option {other}")),
        }
    }
    let data = data.unwrap_or_else(default_data);
    let data = std::path::absolute(&data).unwrap_or(data);
    log::open(&data);
    log!("Minecraft's files and the saves are in {}", data.display());
    let root = setup::ensure(&data).unwrap_or_else(|error| {
        exit_with(&format!(
            "Minecraft's files could not be fetched from Mojang: {error}"
        ))
    });
    let save = (!temporary).then(|| data.join("saves").join(&world_name));
    // A saved world keeps the seed it was made with.
    let saved_seed = save.as_deref().and_then(save::read).map(|saved| saved.seed);
    let seed = saved_seed.or(seed).unwrap_or_else(random_seed);
    if let Some(save) = save.as_ref() {
        log!("World `{world_name}` in {} (seed {seed})", save.display());
    }
    let options = Options {
        seed,
        view_distance,
        creative,
        save,
        time,
        vsync,
    };
    let event_loop =
        EventLoop::new().unwrap_or_else(|error| exit_with(&format!("no window system: {error}")));
    console::watch(event_loop.create_proxy());
    let mut app = App {
        options,
        root,
        data,
        capture,
        window: None,
        renderer: None,
        gui: None,
        phase: None,
        input: Input::default(),
        last: Instant::now(),
        played: 0.0,
        grabbed: false,
        focused: true,
        failed: None,
    };
    if let Err(error) = event_loop.run_app(&mut app) {
        exit_with(&error.to_string());
    }
    if let Some(error) = app.failed {
        exit_with(&error);
    }
}

/// `minecraft-data` where the game is run from if it is there, else next to
/// the program, so the same folder is found however the game is started.
fn default_data() -> PathBuf {
    let here = PathBuf::from("minecraft-data");
    if here.is_dir() {
        return here;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("minecraft-data")))
        .unwrap_or(here)
}

fn exit_with(message: &str) -> ! {
    log::line_now(&format!("minecraft: {message}"));
    // Started from Explorer, the console would close before it is read.
    #[cfg(windows)]
    {
        eprintln!("Press Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    std::process::exit(1)
}
