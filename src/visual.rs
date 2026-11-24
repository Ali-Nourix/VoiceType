use crate::config::Settings;
use eframe::egui::{self, Color32, RichText, Vec2};

pub const BG: Color32 = Color32::from_rgb(20, 21, 20);
const SIDE: Color32 = Color32::from_rgb(25, 26, 25);
const CARD: Color32 = Color32::from_rgb(31, 33, 31);
const LINE: Color32 = Color32::from_rgb(55, 58, 54);
const WHITE: Color32 = Color32::from_rgb(239, 241, 233);
const MUTED: Color32 = Color32::from_rgb(150, 155, 146);
const GREEN: Color32 = Color32::from_rgb(189, 220, 143);

#[derive(Debug)]
pub enum Action {
    Toggle,
    Cancel,
    Copy(usize),
    Save,
    Hide,
}
pub struct Display<'a> {
    pub state: &'a str,
    pub hold_to_talk: bool,
    pub status: &'a str,
    pub source: &'a str,
    pub shortcut: &'a str,
    pub history: &'a [String],
    pub elapsed: u64,
    pub wave: &'a [f32],
}
pub fn setup(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(WHITE);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = BG;
    style.visuals.widgets.noninteractive.bg_fill = CARD;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(38, 40, 37);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(56, 60, 53);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(68, 74, 62);
    style.visuals.selection.bg_fill = Color32::from_rgb(72, 90, 57);
    style.spacing.item_spacing = Vec2::new(9., 10.);
    style.spacing.button_padding = Vec2::new(12., 8.);
    ctx.set_style(style);
}
fn small(text: impl Into<String>) -> RichText {
    RichText::new(text).size(12.).color(MUTED)
}
fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12., 16.), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4., color);
}
fn logo(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(24., 24.), egui::Sense::hover());
    for (i, h) in [10., 18., 23., 14.].iter().enumerate() {
        let x = r.left() + 3.0 + i as f32 * 6.0;
        ui.painter().line_segment(
            [
                egui::pos2(x, r.center().y - h / 2.),
                egui::pos2(x, r.center().y + h / 2.),
            ],
            egui::Stroke::new(3.0_f32, GREEN),
        );
    }
}
fn section(ui: &mut egui::Ui, title: &str, caption: &str) {
    ui.label(RichText::new(title).size(16.).strong());
    ui.label(small(caption));
    ui.add_space(7.);
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
pub fn hub(
    ctx: &egui::Context,
    d: &Display<'_>,
    draft: &mut Settings,
    settings: &mut bool,
) -> Vec<Action> {
    let mut actions = Vec::new();
    egui::SidePanel::left("sidebar")
        .exact_width(192.)
        .frame(
            egui::Frame::new()
                .fill(SIDE)
                .inner_margin(egui::Margin::symmetric(16, 22)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                logo(ui);
                ui.label(RichText::new("VoiceType").size(21.).strong());
            });
            ui.add_space(32.);
            if ui
                .add_sized(
                    [ui.available_width(), 40.],
                    egui::Button::new(RichText::new("Dictation").size(14.))
                        .fill(if !*settings {
                            Color32::from_rgb(48, 53, 45)
                        } else {
                            SIDE
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(8),
                )
                .clicked()
            {
                *settings = false;
            }
            if ui
                .add_sized(
                    [ui.available_width(), 40.],
                    egui::Button::new(RichText::new("Settings").size(14.))
                        .fill(if *settings {
                            Color32::from_rgb(48, 53, 45)
                        } else {
                            SIDE
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(8),
                )
                .clicked()
            {
                *settings = true;
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(small("VOICE TO TEXT  /  WINDOWS"));
                ui.add_space(8.);
                if ui.button("Hide to tray").clicked() {
                    actions.push(Action::Hide);
                }
            });
        });
    egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(34, 28))).show(ctx, |ui| {
      egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if *settings {
            ui.label(RichText::new("Settings").size(29.).strong());
            ui.label(small("Adjust how VoiceType listens and types.")); ui.add_space(22.);
            section(ui,"Shortcut", "Choose a modifier and a letter for your global shortcut.");
            ui.horizontal(|ui| { ui.checkbox(&mut draft.ctrl,"Ctrl"); ui.checkbox(&mut draft.alt,"Alt"); ui.checkbox(&mut draft.shift,"Shift");
                egui::ComboBox::from_id_salt("key").selected_text(&draft.key).show_ui(ui, |ui| { for c in 'A'..='Z' { let k=c.to_string(); ui.selectable_value(&mut draft.key,k.clone(),k); } }); });
            ui.horizontal(|ui| { ui.radio_value(&mut draft.hold_to_talk, false, "Press to toggle"); ui.radio_value(&mut draft.hold_to_talk, true, "Hold to talk"); });
            ui.add_space(20.); ui.separator(); ui.add_space(16.);
            section(ui,"Audio & language", "System audio captures sound playing through your default output.");
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("source").selected_text(&draft.source).show_ui(ui,|ui| { ui.selectable_value(&mut draft.source,"Microphone".into(),"Microphone"); ui.selectable_value(&mut draft.source,"System audio".into(),"System audio"); });
                egui::ComboBox::from_id_salt("lang").selected_text(if draft.language=="fa-IR" {"Persian · fa-IR"} else {"English · en-US"}).show_ui(ui,|ui| { ui.selectable_value(&mut draft.language,"fa-IR".into(),"Persian · fa-IR"); ui.selectable_value(&mut draft.language,"en-US".into(),"English · en-US"); });
            });
            ui.add_space(18.); ui.separator(); ui.add_space(14.);
            section(ui,"Preferences", "Changes apply after saving.");
            ui.checkbox(&mut draft.auto_type,"Type into the original focused app");
            ui.checkbox(&mut draft.normalize_persian,"Normalize Persian letters");
            ui.checkbox(&mut draft.startup,"Start with Windows, minimized to tray");
            ui.add_space(14.);
            if ui.add(egui::Button::new(RichText::new("Save changes").color(BG).strong()).fill(GREEN).corner_radius(7).min_size(Vec2::new(140.,40.))).clicked() { actions.push(Action::Save); }
            ui.add_space(14.); ui.label(small("Audio is sent to Google’s unofficial Chromium speech endpoint. It may stop working. Recordings are limited to 30 seconds."));
        } else {
            ui.horizontal(|ui| { ui.label(RichText::new("Dictation").size(29.).strong()); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(small("SESSION HISTORY")); }); });
            ui.label(small("Your words, wherever you write.")); ui.add_space(25.);
            egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32,LINE)).corner_radius(14).inner_margin(egui::Margin::same(22)).show(ui,|ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| { let color=if d.state=="Ready" {GREEN} else {Color32::from_rgb(244, 172, 141)}; dot(ui,color); ui.label(RichText::new(d.state.to_uppercase()).size(11.).strong().color(MUTED)); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {ui.label(small(format!("{}  ·  30 sec max",d.source)));}); });
                ui.add_space(15.);
                ui.label(RichText::new(if d.state=="Ready" {"Ready when you are."} else if d.state=="Listening" {"Listening for audio."} else {"Working on your words."}).size(21.).strong());
                ui.label(small(format!("{}  ·  {}",d.shortcut, if d.hold_to_talk {"hold to talk"} else {"press to start or stop"})));
                ui.add_space(16.);
                let label=if d.state=="Ready" {"Start recording"} else if d.state=="Listening" || d.state=="Starting" {"Stop & transcribe"} else {"Transcribing…"};
                if ui.add_enabled(d.state=="Ready" || d.state=="Listening" || d.state=="Starting", egui::Button::new(RichText::new(label).color(BG).strong()).fill(GREEN).corner_radius(8).min_size(Vec2::new(170.,39.))).clicked() {actions.push(Action::Toggle);}
            });
            ui.add_space(28.);
            ui.horizontal(|ui| {ui.label(RichText::new("Recent transcripts").size(17.).strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(small("THIS SESSION ONLY"));});});
            ui.add_space(7.);
            if d.history.is_empty() {
                egui::Frame::new().stroke(egui::Stroke::new(1.0_f32,LINE)).corner_radius(12).inner_margin(egui::Margin::same(22)).show(ui, |ui| {ui.set_min_width(ui.available_width()); ui.label(RichText::new("Your first transcript will appear here.").size(14.)); ui.label(small("Use the shortcut in any app to get started."));});
            } else { for (i, text) in d.history.iter().enumerate() { egui::Frame::new().stroke(egui::Stroke::new(1.0_f32,LINE)).corner_radius(9).inner_margin(egui::Margin::same(14)).show(ui,|ui| {ui.set_min_width(ui.available_width());ui.horizontal(|ui| {let copy=ui.add(egui::Button::new("Copy").small()); if copy.clicked(){actions.push(Action::Copy(i));} ui.label(RichText::new(visual_text(text)).size(14.));});}); ui.add_space(4.); } }
        }
        ui.add_space(20.); ui.label(small(d.status));
      });
    });
    actions
}
pub fn bar(ctx: &egui::Context, d: &Display<'_>) -> Option<Action> {
    let mut action = None;
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(Color32::TRANSPARENT)
                .inner_margin(egui::Margin::symmetric(5, 5)),
        )
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(31, 34, 30))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(91, 103, 80)))
                .corner_radius(25)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(234.);
                    ui.horizontal(|ui| {
                        dot(
                            ui,
                            if d.state == "Listening" {
                                GREEN
                            } else {
                                Color32::from_rgb(239, 191, 135)
                            },
                        );
                        if d.state == "Listening" {
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::new(94., 24.), egui::Sense::hover());
                            let n = 14;
                            let w = rect.width() / n as f32;
                            for i in 0..n {
                                let v = d
                                    .wave
                                    .get(d.wave.len().saturating_sub(n) + i)
                                    .copied()
                                    .unwrap_or(0.);
                                let h = 4. + v.clamp(0., 1.) * 19.;
                                let x = rect.left() + w * (i as f32 + 0.5);
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(x, rect.center().y - h / 2.),
                                        egui::pos2(x, rect.center().y + h / 2.),
                                    ],
                                    egui::Stroke::new(3.5_f32, GREEN),
                                );
                            }
                            ui.label(
                                RichText::new(format!("{}:{:02}", d.elapsed / 60, d.elapsed % 60))
                                    .monospace()
                                    .size(12.)
                                    .color(WHITE),
                            );
                            if ui
                                .add(egui::Button::new("■").small().frame(false))
                                .on_hover_text("Stop and transcribe")
                                .clicked()
                            {
                                action = Some(Action::Toggle);
                            }
                        } else {
                            ui.label(RichText::new(d.state).size(13.));
                            ui.add_space(5.);
                        }
                        if ui
                            .add(egui::Button::new("×").small().frame(false))
                            .on_hover_text("Cancel")
                            .clicked()
                        {
                            action = Some(Action::Cancel);
                        }
                    });
                });
        });
    action
}
