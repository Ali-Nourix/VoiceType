#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod audio;
mod config;
mod hotkeys;
mod inject;
mod speech;
mod visual;

use audio::{Event, Source};
use config::Settings;
use eframe::egui;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, HotKeyState,
};
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

#[derive(PartialEq)]
enum State {
    Ready,
    Starting,
    Recording,
    Processing,
    Cancelling,
}
struct App {
    settings: Settings,
    draft: Settings,
    manager: Option<hotkeys::Manager>,
    hotkey: Option<HotKey>,
    tray: Option<TrayIcon>,
    state: State,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    stop: Option<Sender<()>>,
    cancelled: Arc<AtomicBool>,
    escape: Option<HotKey>,
    bar_position: [f32; 2],
    wave: Vec<f32>,
    smoothed_level: f32,
    wave_tick: Instant,
    history: Vec<String>,
    level: Arc<AtomicU32>,
    started: Instant,
    hot_rx: Receiver<GlobalHotKeyEvent>,
    menu_rx: Receiver<MenuEvent>,
    tray_rx: Receiver<TrayIconEvent>,
    target: Option<isize>,
    last_text: String,
    message: String,
    settings_open: bool,
    preview_bar: bool,
    ui_test: bool,
    first_frame: bool,
    hide_at_start: bool,
    quitting: bool,
}
fn hotkey(s: &Settings) -> Result<HotKey, String> {
    let mut m = Modifiers::empty();
    if s.ctrl {
        m |= Modifiers::CONTROL;
    }
    if s.alt {
        m |= Modifiers::ALT;
    }
    if s.shift {
        m |= Modifiers::SHIFT;
    }
    if m.is_empty() {
        return Err("Choose at least one modifier.".into());
    }
    let code =
        Code::from_str(&format!("Key{}", s.key)).map_err(|_| "Invalid shortcut key".to_string())?;
    Ok(HotKey::new(Some(m), code))
}
fn tray_icon() -> Result<TrayIcon, String> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id("open", "Open VoiceType", true, None))
        .map_err(|e| e.to_string())?;
    menu.append(&MenuItem::with_id("quit", "Quit", true, None))
        .map_err(|e| e.to_string())?;
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0i32..32 {
        for x in 0i32..32 {
            let i = (y as usize * 32 + x as usize) * 4;
            let dx = x - 16;
            let dy = y - 16;
            if dx * dx + dy * dy <= 225 {
                rgba[i..i + 4].copy_from_slice(&[34, 39, 43, 255]);
            }
            let mic = (x >= 13 && x <= 18 && y >= 7 && y <= 18)
                || (y >= 18 && y <= 21 && x >= 10 && x <= 21)
                || (x >= 15 && x <= 16 && y >= 21 && y <= 25)
                || (x >= 12 && x <= 19 && y == 25);
            if mic {
                rgba[i..i + 4].copy_from_slice(&[156, 232, 194, 255]);
            }
        }
    }
    let icon = tray_icon::Icon::from_rgba(rgba, 32, 32).map_err(|e| e.to_string())?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("VoiceType")
        .with_menu_on_left_click(false)
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        visual::setup(&cc.egui_ctx);
        let (hot_tx, hot_rx) = mpsc::channel();
        let manager_result = hotkeys::Manager::new(hot_tx, cc.egui_ctx.clone());
        let (menu_tx, menu_rx) = mpsc::channel();
        let wake = cc.egui_ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            wake_root_native(event.id.0 == "open");
            let _ = menu_tx.send(event);
            wake.request_repaint();
        }));
        let (tray_tx, tray_rx) = mpsc::channel();
        let wake = cc.egui_ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if matches!(
                &event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                wake_root_native(true);
            }
            let _ = tray_tx.send(event);
            wake.request_repaint();
        }));
        let settings = config::load();
        let manager = manager_result.ok();
        let mut current = None;
        let mut message = "Ready when you are.".to_string();
        if let (Some(m), Ok(h)) = (&manager, hotkey(&settings)) {
            if m.register(h).is_ok() {
                current = Some(h);
            } else {
                message = "Shortcut unavailable. Choose another in Settings.".into();
            }
        } else {
            message = "Global shortcut unavailable. Check Settings.".into();
        }
        let tray = tray_icon().ok();
        if tray.is_none() {
            message = "System tray unavailable. Window will remain open.".into();
        }
        let (tx, rx) = mpsc::channel();
        diag(&format!(
            "ready hotkey={} tray={}",
            current.is_some(),
            tray.is_some()
        ));
        Self {
            draft: settings.clone(),
            settings,
            manager,
            hotkey: current,
            tray,
            state: State::Ready,
            tx,
            rx,
            stop: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            escape: None,
            bar_position: [30., 30.],
            wave: vec![0.; 14],
            smoothed_level: 0.,
            wave_tick: Instant::now(),
            history: Vec::new(),
            level: Arc::new(AtomicU32::new(0)),
            started: Instant::now(),
            hot_rx,
            menu_rx,
            tray_rx,
            target: None,
            last_text: String::new(),
            message,
            settings_open: false,
            preview_bar: false,
            ui_test: std::env::var_os("VOICETYPE_UI_TEST_FAKE").is_some(),
            first_frame: true,
            hide_at_start: std::env::args().any(|a| a == "--minimized"),
            quitting: false,
        }
    }
    fn toggle(&mut self, ctx: &egui::Context) {
        match self.state {
            State::Ready => {
                self.preview_bar = false;
                self.target = inject::target();
                self.cancelled.store(false, Ordering::Release);
                self.wave = vec![0.; 14];
                self.smoothed_level = 0.;
                self.wave_tick = Instant::now();
                self.bar_position = bar_position(ctx.pixels_per_point());
                let escape = HotKey::new(None, Code::Escape);
                if let Some(mgr) = &self.manager {
                    if mgr.register(escape).is_ok() {
                        self.escape = Some(escape);
                    }
                }
                self.state = State::Starting;
                self.message = "Opening audio device…".into();
                self.started = Instant::now();
                let (stop_tx, stop_rx) = mpsc::channel();
                self.stop = Some(stop_tx);
                let source = if self.settings.source == "System audio" {
                    Source::System
                } else {
                    Source::Microphone
                };
                let lang = self.settings.language.clone();
                let tx = self.tx.clone();
                let level = self.level.clone();
                let wake = ctx.clone();
                let cancelled = self.cancelled.clone();
                let ui_test = self.ui_test;
                std::thread::spawn(move || {
                    if ui_test {
                        let _ = tx.send(Event::Started);
                        wake.request_repaint();
                        let _ = stop_rx.recv_timeout(Duration::from_secs(30));
                        if cancelled.load(Ordering::Acquire) {
                            let _ = tx.send(Event::Cancelled);
                        } else {
                            let _ = tx.send(Event::Finished("سلام، این یک متن آزمایشی است.".into()));
                        }
                        wake.request_repaint();
                    } else {
                        audio::run(
                            source,
                            lang,
                            speech::DEFAULT_KEY.to_owned(),
                            stop_rx,
                            cancelled,
                            tx,
                            level,
                            wake,
                        )
                    }
                });
            }
            State::Starting | State::Recording => {
                if let Some(stop) = self.stop.take() {
                    let _ = stop.send(());
                }
                self.state = State::Processing;
                self.message = "Finishing recording…".into();
            }
            State::Processing | State::Cancelling => {}
        }
    }
    fn cancel(&mut self) {
        if self.state == State::Ready || self.state == State::Cancelling {
            return;
        }
        self.cancelled.store(true, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.state = State::Cancelling;
        self.message = "Cancelling… audio already sent cannot be recalled.".into();
    }
    fn clear_escape(&mut self) {
        if let (Some(mgr), Some(key)) = (&self.manager, self.escape.take()) {
            let _ = mgr.unregister(key);
        }
    }
    fn restore_hotkey(&mut self, old: Option<HotKey>, next: HotKey) -> bool {
        if old == Some(next) {
            return true;
        }
        let Some(mgr) = &self.manager else {
            self.hotkey = None;
            return false;
        };
        let _ = mgr.unregister(next);
        self.hotkey = old.filter(|key| mgr.register(*key).is_ok());
        old.is_none() || self.hotkey.is_some()
    }
    fn apply(&mut self) {
        if self.state != State::Ready {
            self.message = "Stop recording before changing settings.".into();
            return;
        }
        let next = match hotkey(&self.draft) {
            Ok(h) => h,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        let old = self.hotkey;
        if Some(next) != old {
            let Some(mgr) = &self.manager else {
                self.message = "Global shortcut manager unavailable.".into();
                return;
            };
            if let Some(previous) = old {
                let _ = mgr.unregister(previous);
            }
            if let Err(e) = mgr.register(next) {
                self.hotkey = old.filter(|previous| mgr.register(*previous).is_ok());
                self.message = format!("Shortcut unavailable: {e}");
                return;
            }
            self.hotkey = Some(next);
        }
        if let Err(e) = config::set_startup(self.draft.startup) {
            let restored = self.restore_hotkey(old, next);
            self.message = format!(
                "Startup setting failed: {e}{}",
                if restored {
                    ""
                } else {
                    "; shortcut also needs reconfiguration"
                }
            );
            return;
        }
        if let Err(e) = config::save(&self.draft) {
            let startup_restored = config::set_startup(self.settings.startup).is_ok();
            let restored = self.restore_hotkey(old, next);
            self.message = format!(
                "Couldn't save settings: {e}{}",
                if restored && startup_restored {
                    ""
                } else {
                    "; check shortcut and startup settings"
                }
            );
            return;
        }
        self.settings = self.draft.clone();
        self.settings_open = false;
        self.message = "Settings saved.".into();
    }
    fn events(&mut self, ctx: &egui::Context) {
        while let Ok(e) = self.hot_rx.try_recv() {
            diag(&format!("hotkey {:?} id={}", e.state, e.id));
            if self.escape.map(|h| h.id()) == Some(e.id) && e.state == HotKeyState::Pressed {
                self.cancel();
            }
            if self.hotkey.map(|h| h.id()) == Some(e.id) {
                if e.state == HotKeyState::Pressed {
                    if !self.settings.hold_to_talk || self.state == State::Ready {
                        self.toggle(ctx);
                    }
                } else if self.settings.hold_to_talk
                    && (self.state == State::Starting || self.state == State::Recording)
                {
                    self.toggle(ctx);
                }
            }
        }
        while let Ok(e) = self.menu_rx.try_recv() {
            diag(&format!("menu {}", e.id.0));
            match e.id.0.as_str() {
                "open" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                "quit" => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }
        while let Ok(e) = self.tray_rx.try_recv() {
            diag("tray event");
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Started => {
                    diag("recording started");
                    if self.state == State::Starting {
                        self.state = State::Recording;
                        self.message = "Listening. Press your shortcut to finish.".into();
                    }
                }
                Event::Processing => {
                    if self.state != State::Cancelling {
                        self.state = State::Processing;
                        self.message = "Sending audio to Google…".into();
                    }
                }
                Event::Finished(text) => {
                    diag("recording finished");
                    if self.cancelled.load(Ordering::Acquire) {
                        self.state = State::Ready;
                        self.stop = None;
                        self.clear_escape();
                        self.message = "Cancelled.".into();
                        continue;
                    }
                    self.clear_escape();
                    let text =
                        if self.settings.language == "fa-IR" && self.settings.normalize_persian {
                            voicetype::core::normalize_persian(&text)
                        } else {
                            text
                        };
                    self.state = State::Ready;
                    self.stop = None;
                    self.message = if self.ui_test {
                        "Test mode: no audio or text was sent.".into()
                    } else if self.settings.auto_type {
                        match inject::type_if_same(self.target, &text) {
                            Ok(()) => "Typed into the focused app.".into(),
                            Err(e) => {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                                e
                            }
                        }
                    } else {
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            "Transcript ready to copy.".into()
                        }
                    };
                    self.last_text = text.clone();
                    self.history.insert(0, text);
                    self.history.truncate(20);
                }
                Event::Cancelled => {
                    self.clear_escape();
                    self.state = State::Ready;
                    self.stop = None;
                    self.message = "Cancelled.".into();
                }
                Event::Failed(e) => {
                    diag("recording failed");
                    self.clear_escape();
                    self.state = State::Ready;
                    self.stop = None;
                    self.message = e;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
            }
        }
    }
    fn shortcut(&self) -> String {
        format!(
            "{}{}{}{}",
            if self.settings.ctrl { "Ctrl + " } else { "" },
            if self.settings.alt { "Alt + " } else { "" },
            if self.settings.shift { "Shift + " } else { "" },
            self.settings.key
        )
    }
}
// Re-show the native root window before requesting a repaint: a hidden window
// may not receive a winit redraw request until it is visible again.
fn diag(line: &str) {
    use std::io::Write;
    if let Some(path) = std::env::var_os("VOICETYPE_UI_TEST_LOG") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}
fn wake_root_native(activate: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_SHOW, SW_SHOWNOACTIVATE,
    };
    let title: Vec<u16> = "VoiceType".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let h = FindWindowW(std::ptr::null(), title.as_ptr());
        if !h.is_null() {
            let mut pid = 0;
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(h, &mut pid);
            if pid == std::process::id() {
                ShowWindow(h, if activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
                if activate {
                    SetForegroundWindow(h);
                }
            }
        }
    }
}
fn bar_position(scale: f32) -> [f32; 2] {
    use windows_sys::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::GetCursorPos,
    };
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut info) != 0 {
            let r = info.rcWork;
            return [
                ((r.left + r.right) as f32 / 2.0 / scale - 125.0),
                (r.bottom as f32 / scale - 78.0),
            ];
        }
    }
    [30., 30.]
}
fn make_bar_nonactivating() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };
    let title: Vec<u16> = "VoiceType Floating Bar"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let h = FindWindowW(std::ptr::null(), title.as_ptr());
        if !h.is_null() {
            let style = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(
                h,
                GWL_EXSTYLE,
                style | WS_EX_NOACTIVATE as isize | WS_EX_TOOLWINDOW as isize,
            );
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.first_frame {
            self.first_frame = false;
            diag("first frame");
        }
        self.events(ctx);
        if self.hide_at_start {
            self.hide_at_start = false;
            if self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if self.state != State::Ready || self.preview_bar {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self.state == State::Recording {
            let target = (self.level.load(Ordering::Relaxed) as f32 / 1000.).sqrt();
            self.smoothed_level += (target - self.smoothed_level)
                * if target > self.smoothed_level {
                    0.35
                } else {
                    0.14
                };
            if self.wave_tick.elapsed() >= Duration::from_millis(55) {
                self.wave.push(self.smoothed_level);
                if self.wave.len() > 14 {
                    self.wave.remove(0);
                }
                self.wave_tick = Instant::now();
            }
        }
        let label = match self.state {
            State::Ready => "Ready",
            State::Starting => "Starting",
            State::Recording => "Listening",
            State::Processing => "Transcribing",
            State::Cancelling => "Cancelling",
        };
        let shortcut = self.shortcut();
        let display = visual::Display {
            state: label,
            shortcut_ok: self.hotkey.is_some(),
            hold_to_talk: self.settings.hold_to_talk,
            status: &self.message,
            source: &self.settings.source,
            shortcut: &shortcut,
            history: &self.history,
            elapsed: self.started.elapsed().as_secs(),
            phase: (self.wave_tick.elapsed().as_secs_f32() / 0.055).min(1.),
            opacity: (self.started.elapsed().as_secs_f32() / 0.14).min(1.),
            wave: &self.wave,
        };
        let mut bar_action = None;
        if self.state != State::Ready || self.preview_bar {
            let demo_wave = [
                0.10, 0.26, 0.40, 0.16, 0.55, 0.32, 0.67, 0.45, 0.28, 0.52, 0.33, 0.19, 0.40, 0.12,
            ];
            let preview_display = visual::Display {
                state: "Listening",
                wave: &demo_wave,
                elapsed: 4,
                opacity: 1.,
                ..display
            };
            let bar_display = if self.preview_bar {
                &preview_display
            } else {
                &display
            };
            let bar_width = if bar_display.state == "Listening" {
                250.
            } else {
                190.
            };
            let bar_position = [
                self.bar_position[0] + (250. - bar_width) / 2.,
                self.bar_position[1],
            ];
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("recording-pill"),
                egui::ViewportBuilder::default()
                    .with_title("VoiceType Floating Bar")
                    .with_inner_size([bar_width, 56.])
                    .with_position(bar_position)
                    .with_decorations(false)
                    .with_taskbar(false)
                    .with_active(false)
                    .with_transparent(true)
                    .with_always_on_top(),
                |ctx, _| {
                    bar_action = visual::bar(ctx, bar_display);
                },
            );
            make_bar_nonactivating();
        }
        let actions = visual::hub(ctx, &display, &mut self.draft, &mut self.settings_open);
        for action in actions.into_iter().chain(bar_action) {
            match action {
                visual::Action::Toggle => {
                    if self.preview_bar {
                        self.preview_bar = false;
                    } else {
                        self.toggle(ctx)
                    }
                }
                visual::Action::Cancel => {
                    if self.preview_bar {
                        self.preview_bar = false;
                    } else {
                        self.cancel()
                    }
                }
                visual::Action::Preview => {
                    self.preview_bar = true;
                    self.bar_position = bar_position(ctx.pixels_per_point());
                    self.message = "Preview only — no audio is recorded or sent.".into();
                }
                visual::Action::Copy(i) => {
                    if let Some(t) = self.history.get(i) {
                        ctx.copy_text(t.clone());
                        self.message = "Copied to clipboard.".into();
                    }
                }
                visual::Action::Save => self.apply(),
                visual::Action::Hide => {
                    if self.tray.is_some() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    } else {
                        self.message = "System tray unavailable.".into();
                    }
                }
            }
        }
    }
}

struct SingleInstance(windows_sys::Win32::Foundation::HANDLE);
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}
fn single_instance() -> Option<SingleInstance> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE},
    };
    let name: Vec<u16> = "Local\\VoiceType.Desktop.Singleton"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return Some(SingleInstance(h));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let title: Vec<u16> = "VoiceType".encode_utf16().chain(Some(0)).collect();
            let window = FindWindowW(std::ptr::null(), title.as_ptr());
            if !window.is_null() {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            CloseHandle(h);
            return None;
        }
        Some(SingleInstance(h))
    }
}
fn main() -> eframe::Result {
    let Some(_single_instance) = single_instance() else {
        return Ok(());
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([650.0, 510.0]),
        ..Default::default()
    };
    eframe::run_native(
        "VoiceType",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
