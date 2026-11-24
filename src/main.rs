#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod audio;
mod config;
mod inject;
mod speech;
mod visual;

use audio::{Event, Source};
use config::Settings;
use eframe::egui;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
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
    manager: Option<GlobalHotKeyManager>,
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
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        // Tahoma ships with Windows and includes Arabic presentation forms.
        let font_path = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts")
            .join("tahoma.ttf");
        if let Ok(bytes) = std::fs::read(font_path) {
            fonts
                .font_data
                .insert("arabic".into(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .push("arabic".into());
        }
        cc.egui_ctx.set_fonts(fonts);
        visual::setup(&cc.egui_ctx);
        let (hot_tx, hot_rx) = mpsc::channel();
        let wake = cc.egui_ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event| {
            let _ = hot_tx.send(event);
            wake.request_repaint();
        }));
        let (menu_tx, menu_rx) = mpsc::channel();
        let wake = cc.egui_ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = menu_tx.send(event);
            wake.request_repaint();
        }));
        let (tray_tx, tray_rx) = mpsc::channel();
        let wake = cc.egui_ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = tray_tx.send(event);
            wake.request_repaint();
        }));
        let settings = config::load();
        let manager = GlobalHotKeyManager::new().ok();
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
            hide_at_start: std::env::args().any(|a| a == "--minimized"),
            quitting: false,
        }
    }
    fn toggle(&mut self, ctx: &egui::Context) {
        match self.state {
            State::Ready => {
                self.target = inject::target();
                self.cancelled.store(false, Ordering::Release);
                self.wave = vec![0.; 14];
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
                std::thread::spawn(move || {
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
                    self.message = if self.settings.auto_type {
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
                ((r.left + r.right) as f32 / 2.0 / scale - 135.0),
                (r.bottom as f32 / scale - 82.0),
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
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.state == State::Recording {
            self.wave
                .push((self.level.load(Ordering::Relaxed) as f32 / 1000.).sqrt());
            if self.wave.len() > 14 {
                self.wave.remove(0);
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
            hold_to_talk: self.settings.hold_to_talk,
            status: &self.message,
            source: &self.settings.source,
            shortcut: &shortcut,
            history: &self.history,
            elapsed: self.started.elapsed().as_secs(),
            wave: &self.wave,
        };
        let mut bar_action = None;
        if self.state != State::Ready {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("recording-pill"),
                egui::ViewportBuilder::default()
                    .with_title("VoiceType Floating Bar")
                    .with_inner_size([270., 56.])
                    .with_position(self.bar_position)
                    .with_decorations(false)
                    .with_taskbar(false)
                    .with_active(false)
                    .with_transparent(true)
                    .with_always_on_top(),
                |ctx, _| {
                    bar_action = visual::bar(ctx, &display);
                },
            );
            make_bar_nonactivating();
        }
        let actions = visual::hub(ctx, &display, &mut self.draft, &mut self.settings_open);
        for action in actions.into_iter().chain(bar_action) {
            match action {
                visual::Action::Toggle => self.toggle(ctx),
                visual::Action::Cancel => self.cancel(),
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

fn main() -> eframe::Result {
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
