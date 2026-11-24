use crate::hid_worker::PID_BLUETOOTH;
use crate::i18n::tr;
use crate::keycodes::{detect_edition_from_keymap, BoardEdition};
use crate::ota::{
    flash_firmware_async, is_known_pro_rgb_source_crc, OtaFlashPolicy, OtaProgress,
};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{win_sys, Crush80App, PID_USB};
use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use std::path::PathBuf;

fn find_bundled_firmware(rel_candidates: &[&str]) -> Option<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            bases.push(dir.to_path_buf());
            if let Some(p1) = dir.parent() {
                bases.push(p1.to_path_buf());
                if let Some(p2) = p1.parent() {
                    bases.push(p2.to_path_buf());
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        bases.push(cwd);
    }

    for base in bases {
        for rel in rel_candidates {
            let p = base.join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

impl Crush80App {
    pub fn ui_ota(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let prog = self
            .ota_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let is_usb_wired = self.status.connected && self.status.pid == PID_USB;
        let detected_ed = detect_edition_from_keymap(&self.layers);
        let is_lite_locked =
            self.edition == BoardEdition::LiteNoRgb || detected_ed == Some(BoardEdition::LiteNoRgb);

        ui.label(RichText::new(s.ota_title).size(19.0).strong().color(TEXT_PRI));
        ui.label(RichText::new(s.ota_sub).size(12.0).color(TEXT_SEC));
        ui.add_space(12.0);

        // Current device & USB-only safety card
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let conn_lbl = if !self.status.connected {
                    s.offline
                } else if is_usb_wired {
                    s.usb_wired
                } else if self.status.pid == PID_BLUETOOTH {
                    s.bluetooth
                } else {
                    s.wireless_24g
                };
                pill_badge(ui, conn_lbl, if is_usb_wired { MINT } else { ROSE });
                if self.status.connected {
                    pill_badge(ui, &self.status.firmware_name, ACCENT);
                    if let Some(crc) = self.status.firmware_crc {
                        pill_badge(ui, &format!("CRC 0x{:08X}", crc), CYAN);
                    }
                }
            });

            if is_lite_locked {
                ui.add_space(10.0);
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(251, 113, 133, 28))
                    .stroke(Stroke::new(1.5_f32, ROSE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(s.ota_norgb_blocked)
                                .size(12.5)
                                .strong()
                                .color(ROSE),
                        );
                    });
            }

            if !is_usb_wired {
                ui.add_space(10.0);
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(251, 113, 133, 22))
                    .stroke(Stroke::new(1.0_f32, ROSE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(s.ota_usb_only).size(12.5).strong().color(ROSE));
                    });
            }

            ui.add_space(8.0);
            ui.label(RichText::new(s.ota_warning).size(11.5).color(ACCENT));
        });

        ui.add_space(12.0);

        // Firmware image selection & validation card
        card_frame().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if primary_button(ui, s.ota_select_bin, 130.0) {
                    if let Some(path) =
                        win_sys::open_file_dialog(s.ota_select_bin, "Firmware BIN (*.bin)", "*.bin")
                    {
                        self.load_ota_file(path);
                    }
                }

                if let Some(ember_path) = find_bundled_firmware(&[
                    "Crush80-RGB-Ember-v1.3.0.bin",
                    "Crush80-RGB-Ember-v1.2.0.bin",
                    "Crush80-RGB-Ember.bin",
                ]) {
                    if chip_button(ui, s.ota_prosuite_btn, false) {
                        self.load_ota_file(ember_path);
                    }
                }

                if let Some(official_path) = find_bundled_firmware(&[
                    r"wobkey-crush80-reboot-pro-firmware\firmware\official\Crush80-RGB-20260618-official.bin",
                    r"firmware\official\Crush80-RGB-20260618-official.bin",
                ]) {
                    if chip_button(ui, s.ota_rollback_official, false) {
                        self.load_ota_file(official_path);
                    }
                }
            });

            if let Some(ref path) = self.ota_image_path {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(path.display().to_string())
                        .size(11.5)
                        .color(TEXT_SEC),
                );
            }

            if let Some(ref res) = self.ota_image_info {
                ui.add_space(10.0);
                match res {
                    Ok(info) => {
                        ui.horizontal_wrapped(|ui| {
                            pill_badge(ui, s.ota_valid, MINT);
                            pill_badge(ui, &info.name, ACCENT);
                            pill_badge(ui, &format!("CRC 0x{:08X}", info.ota_crc), CYAN);
                            pill_badge(ui, &format!("{} B", info.size), TEXT_SEC);
                            if info.sha256_verified {
                                pill_badge(ui, "SHA-256 ✓", MINT);
                            }
                        });
                    }
                    Err(err) => {
                        ui.horizontal_wrapped(|ui| {
                            pill_badge(ui, s.ota_invalid, ROSE);
                            ui.label(RichText::new(err).size(12.0).color(ROSE));
                        });
                    }
                }
            }
        });

        ui.add_space(12.0);

        // Flash trigger + live progress card
        card_frame().show(ui, |ui| {
            let valid_img =
                matches!(self.ota_image_info, Some(Ok(_))) && self.ota_image_bytes.is_some();
            let can_flash = is_usb_wired && valid_img && !prog.running && !is_lite_locked;

            ui.horizontal(|ui| {
                let btn_lbl = if prog.running {
                    s.ota_flashing
                } else {
                    s.ota_flash_btn
                };
                let flash_btn = egui::Button::new(
                    RichText::new(btn_lbl)
                        .size(13.0)
                        .strong()
                        .color(if can_flash { BG_CANVAS } else { TEXT_DIM }),
                )
                .fill(if can_flash { ACCENT } else { BG_SURFACE })
                .stroke(Stroke::new(
                    1.0_f32,
                    if can_flash { ACCENT } else { HAIRLINE },
                ))
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(160.0, 34.0));

                if ui.add_enabled(can_flash, flash_btn).clicked() {
                    self.ota_confirm_open = true;
                    self.ota_risk_acked = false;
                }

                if prog.done && prog.error.is_none() {
                    pill_badge(ui, s.ota_success, MINT);
                    if let Some(crc) = prog.new_crc {
                        pill_badge(ui, &format!("0x{:08X}", crc), CYAN);
                    }
                }
            });

            if prog.running || prog.done || prog.error.is_some() {
                ui.add_space(12.0);
                ui.add(
                    egui::ProgressBar::new(prog.fraction.clamp(0.0, 1.0))
                        .show_percentage()
                        .animate(prog.running),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&prog.stage)
                        .size(12.0)
                        .color(if prog.error.is_some() { ROSE } else { TEXT_SEC }),
                );
                if let Some(ref err) = prog.error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(err).size(12.0).strong().color(ROSE));
                }
            }
        });

        // Pre-flash confirmation modal (shows current -> target CRC + explicit risk acknowledgement)
        if self.ota_confirm_open {
            if let Some(Ok(info)) = self.ota_image_info.clone() {
                let unknown_src = self
                    .status
                    .firmware_crc
                    .map(|c| !is_known_pro_rgb_source_crc(c))
                    .unwrap_or(true);
                let unknown_img = !info.is_known;
                let needs_risk_ack = unknown_src || unknown_img;

                let mut close_modal = false;
                let mut start_flash = false;

                egui::Window::new(s.ota_confirm_title)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                    .max_width(460.0)
                    .show(ui.ctx(), |ui| {
                        let cur_crc_str = self
                            .status
                            .firmware_crc
                            .map(|c| format!("0x{:08X} ({})", c, self.status.firmware_name))
                            .unwrap_or_else(|| "Unknown".into());
                        ui.label(
                            RichText::new(format!(
                                "{}  →  0x{:08X} ({})",
                                cur_crc_str, info.ota_crc, info.name
                            ))
                            .size(13.0)
                            .strong()
                            .color(TEXT_PRI),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("SHA-256: {}…", &info.sha256[..24]))
                                .size(11.0)
                                .color(TEXT_DIM),
                        );

                        if unknown_src {
                            ui.add_space(8.0);
                            ui.label(RichText::new(s.ota_unknown_src_warn).size(12.0).strong().color(ROSE));
                        }
                        if unknown_img {
                            ui.add_space(4.0);
                            ui.label(RichText::new(s.ota_unknown_img_warn).size(12.0).strong().color(ROSE));
                        }
                        if needs_risk_ack {
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.ota_risk_acked, s.ota_risk_ack);
                        }

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let allow_go = !is_lite_locked && (!needs_risk_ack || self.ota_risk_acked);
                            ui.add_enabled_ui(allow_go, |ui| {
                                if primary_button(ui, s.ota_confirm_btn, 140.0) {
                                    start_flash = true;
                                    close_modal = true;
                                }
                            });
                            if chip_button(ui, s.ota_cancel_btn, false) {
                                close_modal = true;
                            }
                        });
                    });

                if close_modal {
                    self.ota_confirm_open = false;
                }
                if start_flash {
                    if let Some(bytes) = self.ota_image_bytes.clone() {
                        if let Ok(mut p) = self.ota_progress.lock() {
                            *p = OtaProgress::default();
                            p.running = true;
                            p.stage = s.ota_flashing.to_string();
                        }
                        self.ota_was_running = true;
                        let policy = OtaFlashPolicy {
                            detected_edition: detected_ed,
                            allow_unknown_source_crc: unknown_src && self.ota_risk_acked,
                            allow_unknown_image: unknown_img && self.ota_risk_acked,
                        };
                        flash_firmware_async(
                            self.hid.clone(),
                            bytes,
                            policy,
                            self.ota_progress.clone(),
                        );
                    }
                }
            } else {
                self.ota_confirm_open = false;
            }
        }
    }
}
