#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod audio;
mod config;
mod inject;
mod speech;

use audio::{Event, Source};
use config::Settings;
use eframe::egui::{self, Color32, RichText, Vec2};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

const BG: Color32 = Color32::from_rgb(18, 20, 25);
const PANEL: Color32 = Color32::from_rgb(29, 32, 39);
const TEXT: Color32 = Color32::from_rgb(239, 240, 243);
const MUTED: Color32 = Color32::from_rgb(163, 169, 181);
const ACCENT: Color32 = Color32::from_rgb(156, 232, 194);

#[derive(PartialEq)]
enum State {
    Ready,
    Starting,
    Recording,
    Processing,
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
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.panel_fill = BG;
        style.visuals.widgets.noninteractive.bg_fill = PANEL;
        style.visuals.widgets.inactive.bg_fill = PANEL;
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(44, 49, 55);
        style.visuals.selection.bg_fill = ACCENT;
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        cc.egui_ctx.set_style(style);
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
                std::thread::spawn(move || {
                    audio::run(
                        source,
                        lang,
                        speech::DEFAULT_KEY.to_owned(),
                        stop_rx,
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
            State::Processing => {}
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
            if e.state == HotKeyState::Pressed && self.hotkey.map(|h| h.id()) == Some(e.id) {
                self.toggle(ctx);
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
                    self.state = State::Processing;
                    self.message = "Sending audio to Google…".into();
                }
                Event::Finished(text) => {
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
                    self.last_text = text;
                }
                Event::Failed(e) => {
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
fn visual_text(s: &str) -> String {
    if !s.chars().any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)) {
        return s.to_owned();
    }
    let shaped = arabic_reshaper::arabic_reshape(s);
    let bidi = unicode_bidi::BidiInfo::new(&shaped, None);
    bidi.paragraphs
        .iter()
        .map(|p| bidi.reorder_line(p, p.range.clone()).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
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
            let source = self.settings.source.clone();
            let level = self.level.load(Ordering::Relaxed) as f32 / 1000.0;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("recording-pill"),
                egui::ViewportBuilder::default()
                    .with_title("VoiceType listening")
                    .with_inner_size([240.0, 56.0])
                    .with_position([30.0, 30.0])
                    .with_decorations(false)
                    .with_taskbar(false)
                    .with_active(false)
                    .with_always_on_top(),
                |ctx, _| {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::new()
                                .fill(PANEL)
                                .inner_margin(egui::Margin::same(12)),
                        )
                        .show(ctx, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("●").color(Color32::from_rgb(255, 158, 141)),
                                );
                                ui.label(RichText::new("Listening").strong());
                                ui.label(RichText::new(source.clone()).small().color(MUTED));
                                ui.label(
                                    RichText::new(if level > 0.03 {
                                        "▮▮▮"
                                    } else {
                                        "▮▮"
                                    })
                                    .color(ACCENT),
                                );
                            });
                        });
                },
            );
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(24))).show(ctx, |ui| {
   ui.horizontal(|ui| {
    ui.label(RichText::new("◉").size(22.0).color(ACCENT)); ui.label(RichText::new("VoiceType").size(20.0).strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| { if ui.button(if self.settings_open { "Done" } else { "Settings" }).clicked() { self.draft=self.settings.clone(); self.settings_open=!self.settings_open; } });
   });
   ui.add_space(14.0); ui.separator(); ui.add_space(14.0);
   if self.settings_open {
    egui::ScrollArea::vertical().show(ui,|ui| {
     ui.label(RichText::new("Settings").size(24.0).strong()); ui.label(RichText::new("Make VoiceType yours.").color(MUTED)); ui.add_space(12.0);
     ui.label(RichText::new("GLOBAL SHORTCUT").small().color(MUTED));
     ui.horizontal(|ui| { ui.checkbox(&mut self.draft.ctrl,"Ctrl"); ui.checkbox(&mut self.draft.alt,"Alt"); ui.checkbox(&mut self.draft.shift,"Shift");
      egui::ComboBox::from_id_salt("key").selected_text(&self.draft.key).show_ui(ui,|ui| { for c in 'A'..='Z' { let k=c.to_string(); ui.selectable_value(&mut self.draft.key,k.clone(),k); } }); });
     ui.add_space(8.0); ui.label(RichText::new("LANGUAGE").small().color(MUTED));
     egui::ComboBox::from_id_salt("language").selected_text(if self.draft.language=="fa-IR"{"Persian · fa-IR"}else{"English · en-US"}).show_ui(ui,|ui| {ui.selectable_value(&mut self.draft.language,"fa-IR".into(),"Persian · fa-IR");ui.selectable_value(&mut self.draft.language,"en-US".into(),"English · en-US");});
     ui.add_space(8.0); ui.label(RichText::new("AUDIO SOURCE").small().color(MUTED));
     egui::ComboBox::from_id_salt("source").selected_text(&self.draft.source).show_ui(ui,|ui| {ui.selectable_value(&mut self.draft.source,"Microphone".into(),"Microphone");ui.selectable_value(&mut self.draft.source,"System audio".into(),"System audio · loopback");});
     ui.add_space(8.0); ui.checkbox(&mut self.draft.auto_type,"Type into the focused app"); ui.checkbox(&mut self.draft.normalize_persian,"Normalize Persian ی and ک"); ui.checkbox(&mut self.draft.startup,"Launch at Windows startup");
     ui.add_space(10.0); if ui.add_sized([130.0,40.0],egui::Button::new(RichText::new("Save changes").color(BG).strong()).fill(ACCENT)).clicked() {self.apply();}
     ui.add_space(8.0); ui.label(RichText::new("Audio is sent to Google's Chromium speech endpoint. This unofficial endpoint may stop working. VoiceType · GPL-3.0 · No warranty.").size(12.0).color(MUTED));
    });
   } else {
    ui.label(RichText::new("Less typing.\nMore flow.").size(31.0).strong());
    ui.label(RichText::new("Your voice, wherever you write.").size(15.0).color(MUTED)); ui.add_space(22.0);
    egui::Frame::new().fill(PANEL).corner_radius(12).inner_margin(egui::Margin::same(20)).show(ui,|ui| {
     ui.set_min_width(ui.available_width());
     ui.horizontal(|ui| { let (dot,label)=match self.state {State::Ready=>(ACCENT,"READY"),State::Starting|State::Recording=>(Color32::from_rgb(255,158,141),"LISTENING"),State::Processing=>(Color32::from_rgb(242,201,133),"PROCESSING")}; ui.label(RichText::new("●").color(dot));ui.label(RichText::new(label).size(12.0).strong().color(MUTED));
      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {ui.label(RichText::new(self.settings.source.clone()).small().color(MUTED));}); });
     ui.add_space(14.0);
     let (rect,_) = ui.allocate_exact_size(Vec2::new(ui.available_width(),42.0),egui::Sense::hover());
     let amp=self.level.load(Ordering::Relaxed) as f32/1000.0; let painter=ui.painter();
     let bars=36; let width=rect.width()/bars as f32;
     for i in 0..bars {let t=(i as f32*0.6+self.started.elapsed().as_secs_f32()*6.0).sin().abs(); let h=if self.state==State::Recording { 4.0+amp*30.0*(0.3+0.7*t) } else {4.0}; let x=rect.left()+width*(i as f32+0.5); let y=rect.center().y; painter.line_segment([egui::pos2(x,y-h/2.0),egui::pos2(x,y+h/2.0)],egui::Stroke::new(3.0_f32,if self.state==State::Recording{ACCENT}else{Color32::from_rgb(78,85,91)}));}
     ui.add_space(12.0);
     let title=match self.state {State::Ready=>"Start recording",State::Starting|State::Recording=>"Stop & transcribe",State::Processing=>"Transcribing…"};
     if ui.add_enabled(self.state!=State::Processing,egui::Button::new(RichText::new(title).color(BG).strong()).fill(ACCENT).min_size(Vec2::new(ui.available_width(),44.0))).clicked() {self.toggle(ctx);}
     ui.add_space(6.0); ui.label(RichText::new(format!("{}  ·  Up to 30 seconds",self.shortcut())).size(12.0).color(MUTED));
    });
    ui.add_space(18.0); ui.horizontal(|ui| {ui.label(RichText::new("LATEST TRANSCRIPT").size(11.0).strong().color(MUTED));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if !self.last_text.is_empty() && ui.button("Copy").clicked(){ctx.copy_text(self.last_text.clone());self.message="Copied to clipboard.".into();}});});
    ui.add_space(5.0);
    egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {if self.last_text.is_empty(){ui.label(RichText::new("Your words will appear here.").color(MUTED));}else{ui.label(RichText::new(visual_text(&self.last_text)).size(16.0));}});
   }
   ui.add_space(12.0); ui.separator(); ui.label(RichText::new(&self.message).size(12.0).color(MUTED));
  });
    }
}
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 650.0])
            .with_min_inner_size([390.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "VoiceType",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
