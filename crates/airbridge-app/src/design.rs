use super::{AirBridgeApp, Event, approve, hidden_command};
use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, pos2, vec2,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

const INK: Color32 = Color32::from_rgb(29, 43, 62);
const MUTED: Color32 = Color32::from_rgb(109, 123, 143);
const BLUE: Color32 = Color32::from_rgb(39, 106, 232);
const TINT: Color32 = Color32::from_rgb(235, 242, 254);
const BACK: Color32 = Color32::from_rgb(250, 251, 253);
const LINE: Color32 = Color32::from_rgb(225, 232, 241);
const GREEN: Color32 = Color32::from_rgb(35, 139, 105);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Home,
    Receive,
    Send,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Preparing,
    Ready,
    Waking,
    Discovering,
    Consent,
    Waiting,
    Transfer,
    Complete,
    Error,
}
#[derive(Clone, Copy)]
enum Icon {
    Brand,
    Down,
    Up,
    Folder,
    Back,
    Forward,
    Phone,
    Check,
    Close,
}

pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read("C:/Windows/Fonts/SegUIVar.ttf") {
        fonts
            .font_data
            .insert("system-ui".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "system-ui".into());
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = BACK;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.override_text_color = Some(INK);
    style.visuals.selection.bg_fill = TINT;
    style.visuals.selection.stroke = Stroke::new(1.0, BLUE);
    style.visuals.widgets.inactive.bg_fill = TINT;
    style.visuals.widgets.inactive.weak_bg_fill = TINT;
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, BLUE);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widget.corner_radius = 14.into();
    }
    style.spacing.button_padding = vec2(20.0, 13.0);
    style.spacing.item_spacing = vec2(12.0, 10.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(15.0));
    ctx.set_style(style);
}

fn settings_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("AirBridge")
        .join("preferences.json")
}
pub fn load_preferences() -> (PathBuf, bool) {
    let default = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Users/Public"))
        .join("Downloads")
        .join("AirBridge");
    let saved = std::fs::read(settings_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let folder = saved
        .as_ref()
        .and_then(|v| v["folder"].as_str())
        .map(PathBuf::from)
        .filter(|p| super::windows_to_wsl(&p.display().to_string()).is_some())
        .unwrap_or(default);
    (folder, false)
}
pub fn save_preferences(folder: &Path) -> std::io::Result<()> {
    let path = settings_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::json!({"folder":folder}).to_string())
}

pub fn app_icon() -> egui::IconData {
    egui::IconData {
        rgba: include_bytes!("../assets/airbridge-64.rgba").to_vec(),
        width: 64,
        height: 64,
    }
}

fn label(ui: &mut egui::Ui, text: impl Into<String>, size: f32, color: Color32) {
    ui.label(RichText::new(text).size(size).color(color));
}
fn heading(ui: &mut egui::Ui, text: &str, sub: &str) {
    ui.label(RichText::new(text).size(29.0).strong().color(INK));
    if !sub.is_empty() {
        ui.add_space(2.0);
        label(ui, sub, 14.0, MUTED);
    }
}
fn button(ui: &mut egui::Ui, text: &str, primary: bool) -> bool {
    ui.add_sized(
        [148.0, 46.0],
        egui::Button::new(RichText::new(text).color(if primary { Color32::WHITE } else { BLUE }))
            .fill(if primary { BLUE } else { TINT })
            .corner_radius(14),
    )
    .clicked()
}
fn icon_button(ui: &mut egui::Ui, kind: Icon, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 12, TINT);
    }
    icon(ui.painter(), rect.center(), 20.0, kind, MUTED);
    response.on_hover_text(tooltip)
}

pub fn show(app: &mut AirBridgeApp, ctx: &egui::Context) {
    let time = ctx.input(|i| i.time);
    let animated = matches!(
        app.phase,
        Phase::Preparing
            | Phase::Ready
            | Phase::Waking
            | Phase::Discovering
            | Phase::Waiting
            | Phase::Transfer
    );
    ctx.request_repaint_after(Duration::from_millis(if animated && !app.reduce_motion {
        16
    } else {
        250
    }));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BACK).inner_margin(26))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if app.view != View::Home {
                    if icon_button(ui, Icon::Back, "Back · stop this session").clicked() {
                        app.stop();
                    }
                } else {
                    let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                    ui.painter().rect_filled(r, 11, BLUE);
                    icon(ui.painter(), r.center(), 23.0, Icon::Brand, Color32::WHITE);
                }
                ui.label(RichText::new("AirBridge").size(18.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Folder, "Save location").clicked() {
                        app.show_folder = !app.show_folder;
                    }
                    connection_menu(app, ui);
                    if app.preview {
                        label(ui, "Design preview", 12.0, MUTED);
                    }
                });
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let key = format!("{:?}-{:?}", app.view, app.phase);
                    let id = egui::Id::new("scene-transition");
                    let (previous, since) = ctx.data_mut(|d| {
                        d.get_temp::<(String, f64)>(id)
                            .unwrap_or((String::new(), time))
                    });
                    let since = if previous != key {
                        ctx.data_mut(|d| d.insert_temp(id, (key, time)));
                        time
                    } else {
                        since
                    };
                    let fade = if app.reduce_motion {
                        1.0
                    } else {
                        ((time - since) / 0.26).clamp(0.0, 1.0) as f32
                    };
                    if fade < 1.0 {
                        ctx.request_repaint();
                    }
                    ui.scope(|ui| {
                        ui.set_opacity(0.35 + 0.65 * (1.0 - (1.0 - fade).powi(3)));
                        ui.vertical_centered(|ui| {
                            ui.set_max_width(580.0);
                            if app.phase == Phase::Error {
                                error_view(app, ui);
                            } else {
                                match app.view {
                                    View::Home => home(app, ui),
                                    View::Receive => receiving(app, ui, time),
                                    View::Send => sending(app, ui, time),
                                }
                            }
                        });
                    });
                    if app.preview {
                        ui.add_space(20.0);
                        ui.horizontal_wrapped(|ui| {
                            for name in [
                                "Home", "Ready", "Accept", "Transfer", "Devices", "Sending", "Done",
                            ] {
                                if ui.small_button(name).clicked() {
                                    preview_state(app, name);
                                }
                            }
                        });
                        label(ui, "Sample data · no radio or file transfers", 12.0, MUTED);
                    }
                });
        });
    folder_panel(app, ctx);
}

fn home(app: &mut AirBridgeApp, ui: &mut egui::Ui) {
    scene(ui, 0.0, Scene::Home, true);
    heading(ui, "A little closer.", "Your files. From here to there.");
    ui.add_space(30.0);
    let width = ((ui.available_width() - 16.0) / 2.0).min(244.0);
    ui.add_enabled_ui(app.hardware_ready(), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - width * 2.0 - 16.0) / 2.0).max(0.0));
            if action_tile(ui, "Receive", Icon::Down, width).clicked() {
                app.start();
            }
            ui.add_space(4.0);
            if action_tile(ui, "Send files", Icon::Up, width).clicked() {
                app.view = View::Send;
                app.phase = Phase::Idle;
                app.files.clear();
                choose_files(app);
            }
        });
    });
}

fn connection_menu(app: &AirBridgeApp, ui: &mut egui::Ui) {
    let checking = app.hardware_checking && !app.preview;
    let ready = app.hardware_ready();
    let (title, color) = if checking {
        ("Connecting…", MUTED)
    } else if ready {
        ("Ready", GREEN)
    } else {
        ("Needs attention", Color32::from_rgb(173, 93, 29))
    };
    let response = ui
        .add(
            egui::Button::new(
                RichText::new(format!("   {title}    "))
                    .size(13.0)
                    .color(color),
            )
            .fill(if ready {
                Color32::from_rgb(233, 246, 240)
            } else {
                TINT
            })
            .corner_radius(18)
            .min_size(vec2(100.0, 34.0)),
        )
        .on_hover_text("Connection & compatibility");
    let rect = response.rect;
    ui.painter()
        .circle_filled(pos2(rect.left() + 14.0, rect.center().y), 3.0, color);
    let tip = pos2(rect.right() - 14.0, rect.center().y + 2.0);
    ui.painter().add(Shape::line(
        vec![tip + vec2(-3.0, -3.0), tip, tip + vec2(3.0, -3.0)],
        Stroke::new(1.2, color),
    ));
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(300.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Connection & compatibility")
                        .size(16.0)
                        .strong(),
                );
                let summary = if checking {
                    "Preparing your connection automatically…"
                } else if app.preview {
                    "Ready to send and receive"
                } else {
                    app.hardware
                        .as_ref()
                        .map_or("Checking your connection…", |r| r.summary.as_str())
                };
                ui.add(egui::Label::new(RichText::new(summary).size(14.0)).wrap());
                label(
                    ui,
                    "AirBridge reconnects automatically when your adapter is plugged in.",
                    13.0,
                    MUTED,
                );
                if let Some(report) = &app.hardware {
                    if !ready && !checking {
                        ui.separator();
                        egui::CollapsingHeader::new("Device details").show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(&report.details).size(12.0))
                                            .wrap()
                                            .selectable(true),
                                    );
                                });
                        });
                    }
                    ui.separator();
                    let id = egui::Id::new("compatibility-copied");
                    let now = ui.input(|i| i.time);
                    let copied = ui
                        .ctx()
                        .data(|d| d.get_temp::<f64>(id))
                        .is_some_and(|at| now - at < 2.0);
                    if ui
                        .add_sized(
                            [300.0, 34.0],
                            egui::Button::new(
                                RichText::new(if copied { "Copied" } else { "Copy diagnostics" })
                                    .color(BLUE),
                            )
                            .fill(TINT)
                            .corner_radius(9),
                        )
                        .clicked()
                    {
                        ui.ctx().copy_text(report.details.clone());
                        ui.ctx().data_mut(|d| d.insert_temp(id, now));
                    }
                }
                ui.add_space(6.0);
            });
        });
}
fn action_tile(ui: &mut egui::Ui, title: &str, kind: Icon, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 146.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let hovered = response.hovered() || response.has_focus();
    let r = rect;
    ui.painter().rect_filled(
        r.translate(vec2(0.0, 4.0)),
        22,
        Color32::from_black_alpha(3),
    );
    ui.painter().rect(
        r,
        22,
        if response.is_pointer_button_down_on() {
            TINT
        } else if hovered {
            Color32::from_rgb(246, 249, 255)
        } else {
            Color32::WHITE
        },
        Stroke::new(1.0, if response.has_focus() { BLUE } else { LINE }),
        egui::StrokeKind::Inside,
    );
    let c = pos2(r.left() + 44.0, r.top() + 43.0);
    ui.painter().circle_filled(c, 21.0, TINT);
    icon(ui.painter(), c, 23.0, kind, BLUE);
    ui.painter().text(
        pos2(r.left() + 24.0, r.bottom() - 30.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(18.0),
        INK,
    );
    icon(
        ui.painter(),
        pos2(r.right() - 28.0, r.bottom() - 30.0),
        15.0,
        Icon::Forward,
        MUTED,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn receiving(app: &mut AirBridgeApp, ui: &mut egui::Ui, time: f64) {
    match app.phase {
        Phase::Consent => {
            scene(ui, time, Scene::Consent, app.reduce_motion);
            heading(
                ui,
                &app.peer_name,
                &format!("wants to share {}", files_text(app.incoming_count)),
            );
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - 308.0) / 2.0).max(0.0));
                if button(ui, "Decline", false) {
                    if !app.preview {
                        approve("decline");
                    }
                    app.pending_sender = None;
                    app.phase = Phase::Ready;
                }
                if button(ui, "Accept", true) {
                    if !app.preview {
                        approve("accept");
                    }
                    app.pending_sender = None;
                    app.phase = if app.preview {
                        Phase::Transfer
                    } else {
                        Phase::Waiting
                    };
                }
            });
        }
        Phase::Transfer | Phase::Waiting => {
            scene(ui, time, Scene::Transfer(false), app.reduce_motion);
            heading(
                ui,
                &format!("Receiving {}", files_text(app.incoming_count)),
                &format!("From {}", app.peer_name),
            );
            progress(ui, app);
            ui.add_space(24.0);
            if button(ui, "Cancel", false) {
                app.stop();
            }
        }
        Phase::Complete => {
            scene(ui, time, Scene::Done, app.reduce_motion);
            heading(
                ui,
                "All yours.",
                &format!("{} saved", files_text(app.incoming_count)),
            );
            ui.add_space(26.0);
            if button(ui, "Open folder", true) {
                open_folder(app.destination.clone());
            }
            ui.add_space(8.0);
            if ui.link("Receive more").clicked() {
                app.phase = Phase::Ready;
            }
        }
        _ => {
            scene(ui, time, Scene::Listening, app.reduce_motion);
            let preparing = app.phase == Phase::Preparing;
            heading(
                ui,
                if preparing {
                    "Getting ready…"
                } else {
                    "Ready to receive"
                },
                if preparing {
                    "Connecting your Wi-Fi adapter"
                } else {
                    "On iPhone: share via AirDrop → AirBridge"
                },
            );
            ui.add_space(30.0);
            if button(ui, "Stop receiving", false) {
                app.stop();
            }
        }
    }
}
fn sending(app: &mut AirBridgeApp, ui: &mut egui::Ui, time: f64) {
    match app.phase {
        Phase::Transfer | Phase::Waiting => {
            let waiting = app.phase == Phase::Waiting;
            scene(
                ui,
                time,
                if waiting {
                    Scene::Listening
                } else {
                    Scene::Transfer(true)
                },
                app.reduce_motion,
            );
            heading(
                ui,
                if waiting {
                    "Waiting for acceptance"
                } else {
                    "Sending your files"
                },
                &format!("To {}", app.peer_name),
            );
            ui.add_space(18.0);
            if waiting {
                label(ui, "Tap Accept on your iPhone", 14.0, MUTED);
            } else {
                label(ui, files_text(app.files.len()), 14.0, MUTED);
            }
            ui.add_space(24.0);
            if button(ui, "Cancel", false) {
                app.stop();
            }
        }
        Phase::Complete => {
            scene(ui, time, Scene::Done, app.reduce_motion);
            heading(
                ui,
                "Delivered.",
                &format!("{} sent to {}", files_text(app.files.len()), app.peer_name),
            );
            ui.add_space(30.0);
            if button(ui, "Done", true) {
                app.stop();
                app.files.clear();
            }
        }
        Phase::Idle => {
            scene(ui, time, Scene::Home, app.reduce_motion);
            heading(
                ui,
                "What are we sending?",
                "Choose files, or drop them here",
            );
            ui.add_space(26.0);
            if button(ui, "Choose files", true) {
                choose_files(app);
            }
        }
        _ => {
            ui.add_space(40.0);
            heading(
                ui,
                "Someone nearby.",
                "On iPhone: AirDrop → Everyone for 10 Minutes",
            );
            ui.add_space(24.0);
            if app.send_targets.is_empty() {
                scene(ui, time, Scene::Searching, app.reduce_motion);
                label(
                    ui,
                    if app.phase == Phase::Waking {
                        "Waking nearby devices · about 20 seconds"
                    } else {
                        "Looking for receivers…"
                    },
                    14.0,
                    MUTED,
                );
            } else {
                let targets = app.send_targets.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(
                        ((ui.available_width() - targets.len().min(3) as f32 * 170.0) / 2.0)
                            .max(0.0),
                    );
                    for (id, name) in targets {
                        if device_tile(ui, &name).clicked() {
                            app.select_receiver(&id, &name);
                        }
                    }
                });
            }
            ui.add_space(24.0);
            let summary = if app.files.len() == 1 {
                app.files[0]
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                files_text(app.files.len())
            };
            ui.label(RichText::new(summary).size(13.0).color(MUTED))
                .on_hover_text(
                    app.files
                        .iter()
                        .map(|f| f.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            ui.add_space(18.0);
            if ui.link("Cancel").clicked() {
                app.stop();
            }
        }
    }
}
fn device_tile(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(vec2(158.0, 170.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Send to {name}"))
    });
    let hovered = response.hovered() || response.has_focus();
    ui.painter().rect(
        r,
        22,
        Color32::WHITE,
        Stroke::new(1.0, if hovered { BLUE } else { LINE }),
        egui::StrokeKind::Inside,
    );
    let c = pos2(r.center().x, r.top() + 54.0);
    ui.painter().circle_filled(c, 32.0, TINT);
    icon(ui.painter(), c, 32.0, Icon::Phone, BLUE);
    let text = ui
        .painter()
        .layout(name.into(), FontId::proportional(14.0), INK, 140.0);
    ui.painter().galley(
        pos2(r.center().x - text.size().x / 2.0, r.top() + 99.0),
        text,
        INK,
    );
    ui.painter().text(
        pos2(r.center().x, r.bottom() - 20.0),
        Align2::CENTER_CENTER,
        "Available",
        FontId::proportional(12.0),
        GREEN,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn progress(ui: &mut egui::Ui, app: &AirBridgeApp) {
    ui.add_space(20.0);
    if let Some(total) = app.total {
        let width = 260.0_f32.min(ui.available_width());
        let (r, _) = ui.allocate_exact_size(vec2(width, 5.0), Sense::hover());
        ui.painter().rect_filled(r, 3, LINE);
        let fraction = (app.bytes as f64 / total as f64).clamp(0.0, 1.0) as f32;
        if fraction > 0.0 {
            ui.painter().rect_filled(
                Rect::from_min_size(r.min, vec2(r.width() * fraction, r.height())),
                3,
                BLUE,
            );
        }
        ui.add_space(6.0);
        label(
            ui,
            format!("{} of {}", size(app.bytes), size(total)),
            13.0,
            MUTED,
        );
    } else {
        label(
            ui,
            if app.bytes == 0 {
                "Waiting for the file…".into()
            } else {
                format!("{} received", size(app.bytes))
            },
            13.0,
            MUTED,
        );
    }
}
fn size(n: u64) -> String {
    if n < 1_000_000 {
        format!("{:.0} KB", n as f64 / 1000.0)
    } else {
        format!("{:.1} MB", n as f64 / 1_000_000.0)
    }
}
fn files_text(n: usize) -> String {
    format!("{} file{}", n, if n == 1 { "" } else { "s" })
}
fn choose_files(app: &AirBridgeApp) {
    let tx = app.events_tx.clone();
    std::thread::spawn(move || {
        if let Some(paths) = rfd::FileDialog::new().pick_files() {
            let _ = tx.send(Event::FilesSelected(paths));
        }
    });
}
fn open_folder(path: PathBuf) {
    std::thread::spawn(move || {
        if std::fs::create_dir_all(&path).is_ok() {
            let _ = hidden_command("explorer.exe").arg(path).spawn();
        }
    });
}
fn folder_panel(app: &mut AirBridgeApp, ctx: &egui::Context) {
    if !app.show_folder {
        return;
    }
    let mut open = true;
    egui::Window::new("Save location")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::RIGHT_TOP, vec2(-26.0, 78.0))
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(app.destination.display().to_string())
                    .size(13.0)
                    .color(MUTED),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Open folder").clicked() {
                    open_folder(app.destination.clone());
                }
                if ui
                    .add_enabled(!app.listening && !app.sending, egui::Button::new("Change…"))
                    .on_disabled_hover_text("Stop the current session to change its save folder")
                    .clicked()
                {
                    let tx = app.events_tx.clone();
                    let directory = app.destination.clone();
                    std::thread::spawn(move || {
                        if let Some(folder) = rfd::FileDialog::new()
                            .set_directory(directory)
                            .pick_folder()
                        {
                            let _ = tx.send(Event::FolderSelected(folder));
                        }
                    });
                }
            });
        });
    app.show_folder = open;
}
fn error_view(app: &mut AirBridgeApp, ui: &mut egui::Ui) {
    scene(ui, 0.0, Scene::Error, true);
    heading(ui, "Let's try that again.", "The transfer could not finish");
    ui.add_space(18.0);
    egui::CollapsingHeader::new("What happened?").show(ui, |ui| {
        ui.set_max_width(440.0);
        ui.label(&app.detail);
    });
    ui.add_space(20.0);
    if button(ui, "Back", true) {
        app.stop();
    }
}

#[derive(Clone, Copy)]
enum Scene {
    Home,
    Listening,
    Searching,
    Consent,
    Transfer(bool),
    Done,
    Error,
}
fn scene(ui: &mut egui::Ui, time: f64, kind: Scene, still: bool) {
    let width = ui.available_width().min(540.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 230.0), Sense::hover());
    let p = ui.painter();
    let center = rect.center();
    let t = if still { 0.7 } else { time as f32 };
    match kind {
        Scene::Transfer(outgoing) => {
            let left = center + vec2(-150.0, 14.0);
            let right = center + vec2(150.0, 14.0);
            p.circle_filled(right, 66.0 + 3.0 * (t * 1.7).sin(), TINT);
            let start = left + vec2(12.0, 0.0);
            let end = right - vec2(12.0, 0.0);
            let curve = |v: f32| {
                let u = 1.0 - v;
                start * (u * u * u)
                    + (start + vec2(60.0, -100.0)).to_vec2() * (3.0 * u * u * v)
                    + (end - vec2(60.0, 100.0)).to_vec2() * (3.0 * u * v * v)
                    + end.to_vec2() * (v * v * v)
            };
            let points = (0..65).map(|i| curve(i as f32 / 64.0)).collect();
            p.add(Shape::line(points, Stroke::new(1.1, LINE)));
            device(p, left, !outgoing);
            device(p, right, outgoing);
            for i in 0..3 {
                let v = (t / 3.6 + i as f32 / 3.0).fract();
                let eased = v * v * (3.0 - 2.0 * v);
                let alpha = (v * 8.0).min((1.0 - v) * 8.0).clamp(0.0, 1.0);
                let scale = 0.65 + 0.35 * (std::f32::consts::PI * v).sin();
                file_card(
                    p,
                    curve(eased),
                    scale,
                    (-0.2 + 0.35 * v) * (std::f32::consts::PI * v).sin(),
                    alpha,
                );
            }
            p.text(
                left + vec2(0.0, 74.0),
                Align2::CENTER_CENTER,
                if outgoing { "This PC" } else { "iPhone" },
                FontId::proportional(12.0),
                MUTED,
            );
            p.text(
                right + vec2(0.0, 74.0),
                Align2::CENTER_CENTER,
                if outgoing { "iPhone" } else { "This PC" },
                FontId::proportional(12.0),
                MUTED,
            );
        }
        Scene::Home | Scene::Listening | Scene::Searching => {
            for i in (0..3).rev() {
                let phase = if matches!(kind, Scene::Home) || still {
                    i as f32 / 3.0
                } else {
                    (t / 4.8 + i as f32 / 3.0).fract()
                };
                let radius = 46.0 + phase * 58.0;
                p.circle_stroke(
                    center,
                    radius,
                    Stroke::new(1.2, BLUE.gamma_multiply((1.0 - phase) * 0.12)),
                );
            }
            p.circle_filled(center, 45.0, TINT);
            icon(
                p,
                center,
                43.0,
                if matches!(kind, Scene::Home) {
                    Icon::Brand
                } else {
                    Icon::Down
                },
                BLUE,
            );
        }
        Scene::Consent => {
            p.circle_filled(center, 46.0, TINT);
            icon(p, center, 45.0, Icon::Phone, BLUE);
        }
        Scene::Done => {
            p.circle_filled(center, 48.0, Color32::from_rgb(231, 247, 240));
            icon(p, center, 43.0, Icon::Check, GREEN);
        }
        Scene::Error => {
            p.circle_filled(center, 46.0, TINT);
            icon(p, center, 38.0, Icon::Close, MUTED);
        }
    }
}
fn device(p: &Painter, c: Pos2, phone: bool) {
    let dimensions = if phone {
        vec2(47.0, 94.0)
    } else {
        vec2(94.0, 63.0)
    };
    let r = Rect::from_center_size(c, dimensions);
    p.rect_filled(
        r.translate(vec2(0.0, 5.0)),
        12,
        Color32::from_black_alpha(5),
    );
    p.rect(
        r,
        12,
        Color32::from_rgb(244, 247, 252),
        Stroke::new(1.6, Color32::from_rgb(181, 197, 219)),
        egui::StrokeKind::Inside,
    );
    if phone {
        p.rect_stroke(
            r.shrink(3.5),
            8,
            Stroke::new(0.7, Color32::from_rgb(215, 225, 239)),
            egui::StrokeKind::Inside,
        );
        p.rect_filled(
            Rect::from_center_size(c + vec2(0.0, -36.0), vec2(13.0, 4.0)),
            3,
            Color32::from_rgb(181, 197, 219),
        );
        p.line_segment(
            [c + vec2(-7.0, 39.0), c + vec2(7.0, 39.0)],
            Stroke::new(1.6, Color32::from_rgb(181, 197, 219)),
        );
        p.line_segment(
            [c + vec2(24.0, -14.0), c + vec2(24.0, 0.0)],
            Stroke::new(1.4, Color32::from_rgb(181, 197, 219)),
        );
    } else {
        p.line_segment(
            [c + vec2(0.0, 32.0), c + vec2(0.0, 43.0)],
            Stroke::new(3.0, Color32::from_rgb(181, 197, 219)),
        );
        p.line_segment(
            [c + vec2(-22.0, 44.0), c + vec2(22.0, 44.0)],
            Stroke::new(3.0, Color32::from_rgb(181, 197, 219)),
        );
    }
    if !phone {
        icon(p, c, 24.0, Icon::Brand, Color32::from_rgb(161, 184, 215));
    }
}
fn file_card(p: &Painter, c: Pos2, scale: f32, angle: f32, alpha: f32) {
    let transform = |x: f32, y: f32| {
        c + vec2(
            x * angle.cos() - y * angle.sin(),
            x * angle.sin() + y * angle.cos(),
        ) * scale
    };
    let corners = [
        (-12.0, -20.0),
        (7.0, -20.0),
        (16.0, -11.0),
        (16.0, 17.0),
        (13.0, 20.0),
        (-12.0, 20.0),
        (-16.0, 16.0),
        (-16.0, -16.0),
    ];
    p.add(Shape::convex_polygon(
        corners
            .iter()
            .map(|&(x, y)| transform(x + 1.0, y + 4.0))
            .collect(),
        Color32::from_black_alpha((10.0 * alpha) as u8),
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        corners.iter().map(|&(x, y)| transform(x, y)).collect(),
        Color32::WHITE.gamma_multiply(alpha),
        Stroke::new(1.2, BLUE.gamma_multiply(alpha * 0.5)),
    ));
    for (y, w) in [(0.0, 8.0), (6.0, 4.0)] {
        p.line_segment(
            [transform(-8.0, y), transform(w, y)],
            Stroke::new(2.0, BLUE.gamma_multiply(alpha * 0.45)),
        );
    }
    p.add(Shape::line(
        vec![
            transform(7.0, -19.0),
            transform(7.0, -11.0),
            transform(15.0, -11.0),
        ],
        Stroke::new(1.0, BLUE.gamma_multiply(alpha * 0.4)),
    ));
}
fn icon(p: &Painter, c: Pos2, size: f32, kind: Icon, color: Color32) {
    let s = size / 24.0;
    let pt = |x: f32, y: f32| c + vec2(x, y) * s;
    let line = |coords: &[(f32, f32)]| {
        p.add(Shape::line(
            coords.iter().map(|&(x, y)| pt(x, y)).collect(),
            Stroke::new(1.7 * s, color),
        ));
    };
    match kind {
        Icon::Brand => {
            // Center the complete symbol's bounds, including both endpoint dots.
            let pt = |x: f32, y: f32| pt(x - 1.778175, y + 1.778175);
            for radius in [8.0, 14.0] {
                let arc: Vec<_> = (0..=32)
                    .map(|i| {
                        let angle = -std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / 32.0);
                        pt(-6.0 + radius * angle.cos(), 6.0 + radius * angle.sin())
                    })
                    .collect();
                p.add(Shape::line(arc, Stroke::new(1.8 * s, color)));
            }
            p.circle_filled(pt(-6.0, 6.0), 1.9 * s, color);
            // Both dots sit 8 units from their nearest signal arc.
            p.circle_filled(pt(9.55635, -9.55635), 1.9 * s, color);
        }
        Icon::Down => {
            line(&[(0.0, -9.0), (0.0, 5.0)]);
            line(&[(-5.0, 0.0), (0.0, 5.0), (5.0, 0.0)]);
            line(&[(-8.0, 5.0), (-8.0, 10.0), (8.0, 10.0), (8.0, 5.0)]);
        }
        Icon::Up => {
            line(&[(0.0, 8.0), (0.0, -7.0)]);
            line(&[(-5.0, -2.0), (0.0, -7.0), (5.0, -2.0)]);
        }
        Icon::Back => {
            line(&[(5.0, -7.0), (-3.0, 0.0), (5.0, 7.0)]);
        }
        Icon::Forward => {
            line(&[(-3.0, -7.0), (5.0, 0.0), (-3.0, 7.0)]);
        }
        Icon::Folder => {
            line(&[
                (-10.0, -7.0),
                (-3.0, -7.0),
                (0.0, -4.0),
                (10.0, -4.0),
                (10.0, 8.0),
                (-10.0, 8.0),
                (-10.0, -7.0),
            ]);
        }
        Icon::Phone => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(12.0, 24.0) * s),
                3,
                Stroke::new(1.7 * s, color),
                egui::StrokeKind::Inside,
            );
            p.rect_filled(
                Rect::from_center_size(pt(0.0, -8.0), vec2(3.5, 1.4) * s),
                1,
                color,
            );
            line(&[(-2.0, 8.5), (2.0, 8.5)]);
        }
        Icon::Check => {
            line(&[(-8.0, 0.0), (-2.0, 6.0), (9.0, -7.0)]);
        }
        Icon::Close => {
            line(&[(-6.0, -6.0), (6.0, 6.0)]);
            line(&[(6.0, -6.0), (-6.0, 6.0)]);
        }
    }
}

pub fn preview_state(app: &mut AirBridgeApp, name: &str) {
    app.listening = false;
    app.sending = false;
    app.pending_sender = None;
    app.send_targets.clear();
    app.peer_name = "Nearby iPhone".into();
    app.incoming_count = 3;
    app.bytes = 24_800_000;
    app.total = Some(42_600_000);
    app.view = View::Receive;
    app.phase = match name {
        "Home" => {
            app.view = View::Home;
            Phase::Idle
        }
        "Ready" => Phase::Ready,
        "Accept" => {
            app.pending_sender = Some(app.peer_name.clone());
            Phase::Consent
        }
        "Devices" => {
            app.view = View::Send;
            app.files = vec![PathBuf::from("Sample photo.heic")];
            app.send_targets = vec![("1".into(), app.peer_name.clone())];
            Phase::Discovering
        }
        "Sending" => {
            app.view = View::Send;
            Phase::Transfer
        }
        "Done" => Phase::Complete,
        _ => Phase::Transfer,
    };
}
