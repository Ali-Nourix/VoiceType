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
    Preview,
}
pub struct Display<'a> {
    pub state: &'a str,
    pub shortcut_ok: bool,
    pub hold_to_talk: bool,
    pub status: &'a str,
    pub source: &'a str,
    pub shortcut: &'a str,
    pub history: &'a [String],
    pub elapsed: u64,
    pub phase: f32,
    pub opacity: f32,
    pub wave: &'a [f32],
}
// Vazirmatn v33.003, subset for Latin/Persian, SIL OFL 1.1.
// Kept as ASCII chunks so the repository can restore the exact embedded font.
fn embedded_font() -> Vec<u8> {
    let chunks = [
        include_str!("../assets/Vazirmatn-1.b64"),
        include_str!("../assets/Vazirmatn-2.b64"),
        include_str!("../assets/Vazirmatn-3.b64"),
    ];
    let mut out = Vec::with_capacity(80_000);
    let (mut bits, mut count) = (0u32, 0u8);
    for c in chunks.into_iter().flat_map(|s| s.bytes()) {
        let n = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        bits = (bits << 6) | n as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out
}
pub fn setup(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "vazirmatn".into(),
        std::sync::Arc::new(egui::FontData::from_owned(embedded_font())),
    );
    fonts.families.insert(
        egui::FontFamily::Name("persian".into()),
        vec!["vazirmatn".into()],
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("vazirmatn".into());
    ctx.set_fonts(fonts);
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
// Wrap in logical order before bidi reordering. Reordering a whole paragraph
// and then wrapping it reverses the order of Persian lines.
fn transcript(ui: &mut egui::Ui, text: &str) {
    let font = egui::FontId::new(16., egui::FontFamily::Name("persian".into()));
    if !text.chars().any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)) {
        ui.add(egui::Label::new(RichText::new(text).font(font)).wrap());
        return;
    }
    let width = ui.available_width().max(1.);
    let width_of = |line: &str| {
        ui.painter()
            .layout_no_wrap(visual_text(line), font.clone(), WHITE)
            .size()
            .x
    };
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!("{current} {word}")
            };
            if !current.is_empty() && width_of(&candidate) > width {
                lines.push(std::mem::take(&mut current));
            }
            if width_of(word) > width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                for ch in word.chars() {
                    let mut next = current.clone();
                    next.push(ch);
                    if !current.is_empty() && width_of(&next) > width {
                        lines.push(std::mem::take(&mut current));
                    }
                    current.push(ch);
                }
            } else if current.is_empty() {
                current = word.to_owned();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }
        lines.push(current);
    }
    for line in lines {
        let visual = visual_text(&line);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.add(egui::Label::new(RichText::new(visual).font(font.clone())).truncate());
        });
    }
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
            ui.label(small("Your words, wherever you write."));
            if d.status != "Ready when you are." && d.state == "Ready" { ui.add_space(8.); ui.label(small(d.status)); }
            if !d.shortcut_ok { ui.add_space(10.); ui.colored_label(Color32::from_rgb(247,174,140), "Shortcut unavailable — open Settings and choose another combination."); }
            ui.add_space(20.);
            egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32,LINE)).corner_radius(14).inner_margin(egui::Margin::same(22)).show(ui,|ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| { let color=if d.state=="Ready" {GREEN} else {Color32::from_rgb(244, 172, 141)}; dot(ui,color); ui.label(RichText::new(d.state.to_uppercase()).size(11.).strong().color(MUTED)); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {ui.label(small(format!("{}  ·  30 sec max",d.source)));}); });
                ui.add_space(15.);
                ui.label(RichText::new(if d.state=="Ready" {"Ready when you are."} else if d.state=="Listening" {"Listening for audio."} else {"Working on your words."}).size(21.).strong());
                ui.label(small(format!("{}  ·  {}",d.shortcut, if d.hold_to_talk {"hold to talk"} else {"press to start or stop"})));
                ui.add_space(16.);
                let label=if d.state=="Ready" {"Start recording"} else if d.state=="Listening" || d.state=="Starting" {"Stop & transcribe"} else {"Transcribing…"};
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(d.state=="Ready" || d.state=="Listening" || d.state=="Starting", egui::Button::new(RichText::new(label).color(BG).strong()).fill(GREEN).corner_radius(8).min_size(Vec2::new(170.,44.))).clicked() {actions.push(Action::Toggle);}
                    if d.state=="Ready" && ui.add(egui::Button::new("Preview floating bar").min_size(Vec2::new(150.,44.))).clicked() { actions.push(Action::Preview); }
                });
                ui.add_space(4.); ui.label(small("Audio is sent to Google after you stop recording."));
            });
            ui.add_space(28.);
            ui.horizontal(|ui| {ui.label(RichText::new("Recent transcripts").size(17.).strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(small("THIS SESSION ONLY"));});});
            ui.add_space(7.);
            if d.history.is_empty() {
                egui::Frame::new().stroke(egui::Stroke::new(1.0_f32,LINE)).corner_radius(12).inner_margin(egui::Margin::same(22)).show(ui, |ui| {ui.set_min_width(ui.available_width()); ui.label(RichText::new("Your first transcript will appear here.").size(14.)); ui.label(small("Use the shortcut in any app to get started."));});
            } else { for (i, text) in d.history.iter().enumerate() { egui::Frame::new().stroke(egui::Stroke::new(1.0_f32,LINE)).corner_radius(9).inner_margin(egui::Margin::same(14)).show(ui,|ui| {ui.set_min_width(ui.available_width());ui.horizontal(|ui| {let copy=ui.add(egui::Button::new("Copy").small()); if copy.clicked(){actions.push(Action::Copy(i));} ui.vertical(|ui| { ui.set_width(ui.available_width()); transcript(ui,text); });});}); ui.add_space(4.); } }
        }
        ui.add_space(20.);
      });
    });
    actions
}
pub fn bar(ctx: &egui::Context, d: &Display<'_>) -> Option<Action> {
    let mut action = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::TRANSPARENT))
        .show(ctx, |ui| {
            let width = if d.state == "Listening" { 250. } else { 190. };
            let (bounds, _) = ui.allocate_exact_size(Vec2::new(width, 56.), egui::Sense::hover());
            let pill = egui::Rect::from_min_size(
                bounds.min + Vec2::new(3., 4.),
                Vec2::new(width - 6., 48.),
            );
            let a = d.opacity.clamp(0., 1.);
            let ink = WHITE.gamma_multiply(a);
            let accent = GREEN.gamma_multiply(a);
            let painter = ui.painter();
            painter.rect_filled(pill, 24., Color32::from_rgb(28, 30, 27).gamma_multiply(a));
            painter.rect_stroke(
                pill,
                24.,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(65, 70, 62).gamma_multiply(a)),
                egui::StrokeKind::Inside,
            );
            let x = pill.left();
            let y = pill.center().y;
            painter.circle_filled(
                egui::pos2(x + 16., y),
                3.5,
                if d.state == "Listening" {
                    accent
                } else {
                    Color32::from_rgb(241, 193, 147).gamma_multiply(a)
                },
            );
            if d.state == "Listening" {
                let n = 12;
                let phase = d.phase.clamp(0., 1.);
                let start = d.wave.len().saturating_sub(n + 1);
                for i in 0..n {
                    let left = d.wave.get(start + i).copied().unwrap_or(0.);
                    let right = d.wave.get(start + i + 1).copied().unwrap_or(left);
                    let level = (left + (right - left) * phase).clamp(0., 1.);
                    let h = 4. + level * 20.;
                    let r = egui::Rect::from_center_size(
                        egui::pos2(x + 34. + i as f32 * 5.6, y),
                        Vec2::new(3.8, h),
                    );
                    painter.rect_filled(r, 2., accent);
                }
                painter.text(
                    egui::pos2(x + 111., y),
                    egui::Align2::LEFT_CENTER,
                    format!("{}:{:02}", d.elapsed / 60, d.elapsed % 60),
                    egui::FontId::monospace(12.5),
                    ink,
                );
                painter.line_segment(
                    [egui::pos2(x + 151., y - 10.), egui::pos2(x + 151., y + 10.)],
                    egui::Stroke::new(1.0_f32, Color32::from_rgb(68, 72, 65).gamma_multiply(a)),
                );
                let stop = egui::Rect::from_center_size(egui::pos2(x + 172., y), Vec2::splat(34.));
                let response = ui
                    .interact(stop, egui::Id::new("pill-stop"), egui::Sense::click())
                    .on_hover_text("Stop and transcribe");
                if response.hovered() {
                    painter.circle_filled(
                        stop.center(),
                        15.,
                        Color32::from_rgb(60, 65, 57).gamma_multiply(a),
                    );
                }
                painter.rect_filled(
                    egui::Rect::from_center_size(stop.center(), Vec2::splat(9.)),
                    2.,
                    ink,
                );
                if response.clicked() {
                    action = Some(Action::Toggle);
                }
            } else {
                painter.text(
                    egui::pos2(x + 34., y),
                    egui::Align2::LEFT_CENTER,
                    d.state,
                    egui::FontId::proportional(13.5),
                    ink,
                );
            }
            let cancel =
                egui::Rect::from_center_size(egui::pos2(pill.right() - 30., y), Vec2::splat(34.));
            let response = ui
                .interact(cancel, egui::Id::new("pill-cancel"), egui::Sense::click())
                .on_hover_text("Cancel");
            if response.hovered() {
                painter.circle_filled(
                    cancel.center(),
                    15.,
                    Color32::from_rgb(60, 65, 57).gamma_multiply(a),
                );
            }
            let c = cancel.center();
            let v = 5.;
            painter.line_segment(
                [egui::pos2(c.x - v, c.y - v), egui::pos2(c.x + v, c.y + v)],
                egui::Stroke::new(1.7_f32, ink),
            );
            painter.line_segment(
                [egui::pos2(c.x - v, c.y + v), egui::pos2(c.x + v, c.y - v)],
                egui::Stroke::new(1.7_f32, ink),
            );
            if response.clicked() {
                action = Some(Action::Cancel);
            }
        });
    action
}
