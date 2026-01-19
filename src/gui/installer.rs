use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use crate::cli;
use crate::desktop::install_self;
use crate::manifest::parse_manifest;
use crate::storage::paths;

// Theme colors - Black with red accents
const BG_COLOR: Color32 = Color32::from_rgb(18, 18, 18);
const PANEL_COLOR: Color32 = Color32::from_rgb(28, 28, 28);
const ACCENT_COLOR: Color32 = Color32::from_rgb(220, 50, 50);
const ACCENT_HOVER: Color32 = Color32::from_rgb(255, 70, 70);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(240, 240, 240);
const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 160, 160);
const SUCCESS_COLOR: Color32 = Color32::from_rgb(80, 200, 120);
const ERROR_COLOR: Color32 = Color32::from_rgb(255, 80, 80);

pub enum InstallType {
    SelfInstall,
    AppInstall {
        name: String,
        display_name: String,
        manifest_content: String,
    },
}

pub struct InstallerApp {
    install_type: InstallType,
    state: InstallerState,
    recv: Receiver<InstallStatus>,
    sender: Sender<InstallStatus>, // Kept to clone for the thread
}

enum InstallerState {
    Confirmation,
    Installing { progress: f32, message: String },
    Done { message: String },
    Error { message: String },
}

enum InstallStatus {
    Progress(f32, String),
    Success(String),
    Error(String),
}

impl InstallerApp {
    pub fn new(install_type: InstallType) -> Self {
        let (sender, recv) = channel();
        Self {
            install_type,
            state: InstallerState::Confirmation,
            recv,
            sender,
        }
    }

    fn start_installation(&mut self) {
        let sender = self.sender.clone();
        let install_type = match &self.install_type {
            InstallType::SelfInstall => InstallType::SelfInstall,
            InstallType::AppInstall {
                name,
                display_name,
                manifest_content,
            } => InstallType::AppInstall {
                name: name.clone(),
                display_name: display_name.clone(),
                manifest_content: manifest_content.clone(),
            },
        };

        self.state = InstallerState::Installing {
            progress: 0.0,
            message: "Starting installation...".to_string(),
        };

        thread::spawn(
            move || match perform_installation(install_type, sender.clone()) {
                Ok(msg) => {
                    let _ = sender.send(InstallStatus::Success(msg));
                }
                Err(e) => {
                    let _ = sender.send(InstallStatus::Error(e.to_string()));
                }
            },
        );
    }
}

fn perform_installation(
    install_type: InstallType,
    sender: Sender<InstallStatus>,
) -> Result<String, Box<dyn std::error::Error>> {
    match install_type {
        InstallType::SelfInstall => {
            let _ = sender.send(InstallStatus::Progress(
                0.1,
                "Creating directories...".to_string(),
            ));
            paths::ensure_dirs()?;

            let _ = sender.send(InstallStatus::Progress(
                0.5,
                "Copying binary...".to_string(),
            ));
            install_self()?;

            let _ = sender.send(InstallStatus::Progress(1.0, "Done!".to_string()));
            Ok(format!(
                "Voidbox v{} has been installed successfully!\n\nYou can now use 'voidbox' from your terminal.",
                crate::VERSION
            ))
        }
        InstallType::AppInstall {
            name,
            display_name,
            manifest_content,
        } => {
            let _ = sender.send(InstallStatus::Progress(
                0.1,
                format!("Preparing to install {}...", display_name),
            ));

            // Ensure runtime is installed first
            if !paths::install_path().exists() {
                let _ = sender.send(InstallStatus::Progress(
                    0.2,
                    "Installing Voidbox runtime...".to_string(),
                ));
                paths::ensure_dirs()?;
                install_self()?;
            }

            let _ = sender.send(InstallStatus::Progress(
                0.3,
                "Parsing manifest...".to_string(),
            ));
            let manifest = parse_manifest(&manifest_content)?;
            let manifest_path = paths::manifest_path(&name);

            // Save manifest
            paths::ensure_dirs()?;
            std::fs::write(&manifest_path, manifest_content)?;

            // We can't easily get granular progress from the CLI functions yet without refactoring,
            // so we'll just show indeterminate progress or "Installing..."
            let _ = sender.send(InstallStatus::Progress(
                0.5,
                "Downloading and extracting...".to_string(),
            ));

            // Install the app
            // Note: This blocks until done
            cli::install_app_from_manifest(&manifest, false)?;

            let _ = sender.send(InstallStatus::Progress(1.0, "Done!".to_string()));
            Ok(format!("{} has been installed successfully!", display_name))
        }
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll for updates from the thread
        while let Ok(status) = self.recv.try_recv() {
            match status {
                InstallStatus::Progress(p, msg) => {
                    self.state = InstallerState::Installing {
                        progress: p,
                        message: msg,
                    };
                }
                InstallStatus::Success(msg) => {
                    self.state = InstallerState::Done { message: msg };
                }
                InstallStatus::Error(msg) => {
                    self.state = InstallerState::Error { message: msg };
                }
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_COLOR).inner_margin(30.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // Header with logo/title
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("VOIDBOX")
                            .size(28.0)
                            .color(ACCENT_COLOR)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("v{}", crate::VERSION))
                            .size(12.0)
                            .color(TEXT_SECONDARY),
                    );
                    ui.add_space(25.0);

                    // Separator line
                    ui.add(egui::Separator::default().spacing(10.0));
                    ui.add_space(15.0);

                    match &self.state {
                        InstallerState::Confirmation => {
                            match &self.install_type {
                                InstallType::SelfInstall => {
                                    ui.label(
                                        RichText::new("Install Voidbox?")
                                            .size(18.0)
                                            .color(TEXT_PRIMARY),
                                    );
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("Universal Linux App Platform")
                                            .size(13.0)
                                            .color(TEXT_SECONDARY),
                                    );
                                    ui.add_space(5.0);
                                    ui.label(
                                        RichText::new("~/.local/bin/voidbox")
                                            .size(11.0)
                                            .color(TEXT_SECONDARY)
                                            .italics(),
                                    );
                                }
                                InstallType::AppInstall { display_name, .. } => {
                                    ui.label(
                                        RichText::new(format!("Install {}?", display_name))
                                            .size(18.0)
                                            .color(TEXT_PRIMARY),
                                    );
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("Download and install application container")
                                            .size(13.0)
                                            .color(TEXT_SECONDARY),
                                    );
                                }
                            }
                            ui.add_space(35.0);

                            ui.horizontal(|ui| {
                                let button_width = 120.0;
                                let total_width = ui.available_width();
                                let spacing = (total_width - button_width * 2.0) / 3.0;

                                ui.add_space(spacing);
                                if ui.add_sized([button_width, 35.0], egui::Button::new(RichText::new("Cancel").size(14.0))).clicked() {
                                    std::process::exit(0);
                                }
                                ui.add_space(spacing);
                                if ui.add_sized([button_width, 35.0], egui::Button::new(RichText::new("Install").size(14.0))).clicked() {
                                    self.start_installation();
                                }
                            });
                        }
                        InstallerState::Installing { progress, message } => {
                            ui.add_space(20.0);
                            ui.label(
                                RichText::new("Installing...")
                                    .size(18.0)
                                    .color(TEXT_PRIMARY),
                            );
                            ui.add_space(15.0);
                            ui.label(
                                RichText::new(message)
                                    .size(13.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(20.0);
                            ui.add(
                                egui::ProgressBar::new(*progress)
                                    .animate(true)
                                    .fill(ACCENT_COLOR),
                            );
                        }
                        InstallerState::Done { message } => {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("✓")
                                    .size(40.0)
                                    .color(SUCCESS_COLOR),
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("Installation Complete")
                                    .size(18.0)
                                    .color(SUCCESS_COLOR),
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(message)
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(25.0);
                            if ui.button(RichText::new("Close").size(14.0)).clicked() {
                                std::process::exit(0);
                            }
                        }
                        InstallerState::Error { message } => {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("✗")
                                    .size(40.0)
                                    .color(ERROR_COLOR),
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("Installation Failed")
                                    .size(18.0)
                                    .color(ERROR_COLOR),
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(message)
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(25.0);
                            if ui.button(RichText::new("Close").size(14.0)).clicked() {
                                std::process::exit(1);
                            }
                        }
                    }
                });
            });
    }
}

fn setup_custom_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Dark background
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = PANEL_COLOR;
    style.visuals.window_fill = BG_COLOR;
    style.visuals.extreme_bg_color = BG_COLOR;

    // Button styling
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 50);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.inactive.rounding = Rounding::same(6.0);

    style.visuals.widgets.hovered.bg_fill = ACCENT_COLOR;
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.hovered.rounding = Rounding::same(6.0);

    style.visuals.widgets.active.bg_fill = ACCENT_HOVER;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.active.rounding = Rounding::same(6.0);

    // Progress bar
    style.visuals.selection.bg_fill = ACCENT_COLOR;

    // Text colors
    style.visuals.override_text_color = Some(TEXT_PRIMARY);

    // Spacing
    style.spacing.button_padding = Vec2::new(16.0, 8.0);
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);

    ctx.set_style(style);
}

pub fn run_installer(install_type: InstallType) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([450.0, 320.0])
            .with_resizable(false)
            .with_decorations(true),
        ..Default::default()
    };

    eframe::run_native(
        "Voidbox",
        options,
        Box::new(|cc| {
            setup_custom_style(&cc.egui_ctx);
            Ok(Box::new(InstallerApp::new(install_type)))
        }),
    )
}
