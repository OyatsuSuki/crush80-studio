use crate::hid_win::FullSnapshot;
use crate::hid_worker::HidCmd;
use crate::i18n::tr;
use crate::storage::{resolve_profile_path, sanitize_profile_name};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{win_sys, Crush80App};
use eframe::egui::{self, RichText, Rounding, Stroke};

impl Crush80App {
    pub fn ui_profiles(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let can_write = self.can_via_write();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.prof_title).size(19.0).strong().color(TEXT_PRI));
                    pill_badge(ui, s.prof_includes_hw, MINT);
                });
                ui.label(RichText::new(s.prof_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(can_write, |ui| {
                    if chip_button(ui, s.prof_export, false) {
                        if let Some(path) = win_sys::save_file_dialog(
                            s.prof_export,
                            "crush80-profile.json",
                            "JSON Profile (*.json)",
                            "*.json",
                        ) {
                            let label = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Exported")
                                .to_string();
                            self.worker.send(HidCmd::ExportSnapshot {
                                path,
                                edition: self.edition,
                                label,
                            });
                        }
                    }
                });

                if chip_button(ui, s.prof_import, false) {
                    if let Some(path) =
                        win_sys::open_file_dialog(s.prof_import, "JSON Profile (*.json)", "*.json")
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(txt) => match serde_json::from_str::<FullSnapshot>(&txt) {
                                Ok(snap) => {
                                    let raw_stem = path
                                        .file_stem()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("imported");
                                    match sanitize_profile_name(raw_stem)
                                        .and_then(|clean| {
                                            resolve_profile_path(&self.profiles_dir, &clean)
                                                .map(|dest| (clean, dest))
                                        })
                                        .and_then(|(clean, dest)| {
                                            serde_json::to_string_pretty(&snap)
                                                .map_err(|e| format!("序列化失败: {}", e))
                                                .and_then(|pretty| {
                                                    std::fs::write(&dest, pretty)
                                                        .map_err(|e| format!("写入失败: {}", e))
                                                })
                                                .map(|()| clean)
                                        }) {
                                        Ok(clean) => {
                                            self.refresh_profiles();
                                            self.set_toast(
                                                format!("{} · {}", s.prof_import, clean),
                                                false,
                                            );
                                        }
                                        Err(e) => self.set_toast(e, true),
                                    }
                                }
                                Err(e) => self.set_toast(format!("Invalid JSON: {}", e), true),
                            },
                            Err(e) => self.set_toast(e.to_string(), true),
                        }
                    }
                }
            });
        });
        ui.add_space(14.0);

        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_profile_name)
                        .hint_text(s.prof_hint)
                        .desired_width(340.0),
                );
                ui.add_enabled_ui(can_write, |ui| {
                    if primary_button(ui, s.save_prof, 130.0) {
                        match sanitize_profile_name(&self.new_profile_name).and_then(|clean| {
                            resolve_profile_path(&self.profiles_dir, &clean)
                                .map(|dest| (clean, dest))
                        }) {
                            Ok((clean, dest)) => {
                                self.new_profile_name.clear();
                                self.worker.send(HidCmd::ExportSnapshot {
                                    path: dest,
                                    edition: self.edition,
                                    label: clean,
                                });
                            }
                            Err(e) => self.set_toast(e, true),
                        }
                    }
                });
            });

            ui.add_space(14.0);
            let names = self.profile_names.clone();
            for name in names {
                egui::Frame::none()
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&name).size(13.5).strong().color(TEXT_PRI));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if danger_button(ui, s.delete) {
                                    match resolve_profile_path(&self.profiles_dir, &name)
                                        .and_then(|p| {
                                            std::fs::remove_file(p)
                                                .map_err(|e| format!("删除方案失败: {}", e))
                                        }) {
                                        Ok(()) => {
                                            self.refresh_profiles();
                                            self.set_toast(
                                                format!("{} · {}", s.delete, name),
                                                false,
                                            );
                                        }
                                        Err(e) => self.set_toast(e, true),
                                    }
                                }
                                if chip_button(ui, s.prof_export, false) {
                                    let default_name = format!("{}.json", name);
                                    if let Some(dest) = win_sys::save_file_dialog(
                                        s.prof_export,
                                        &default_name,
                                        "JSON Profile (*.json)",
                                        "*.json",
                                    ) {
                                        match resolve_profile_path(&self.profiles_dir, &name)
                                            .and_then(|src| {
                                                std::fs::copy(&src, &dest)
                                                    .map_err(|e| format!("导出失败: {}", e))
                                            }) {
                                            Ok(_) => self.set_toast(default_name, false),
                                            Err(e) => self.set_toast(e, true),
                                        }
                                    }
                                }
                                ui.add_enabled_ui(can_write, |ui| {
                                    if primary_button(ui, s.apply_prof, 76.0) {
                                        self.mark_ui_edit();
                                        match resolve_profile_path(&self.profiles_dir, &name)
                                            .and_then(|p| {
                                                std::fs::read_to_string(p)
                                                    .map_err(|e| format!("读取方案失败: {}", e))
                                            })
                                            .and_then(|t| {
                                                serde_json::from_str::<FullSnapshot>(&t)
                                                    .map_err(|e| format!("方案 JSON 损坏: {}", e))
                                            }) {
                                            Ok(snap) => {
                                                self.worker.send(HidCmd::ApplySnapshot {
                                                    snap,
                                                    edition: Some(self.edition),
                                                    save_to_flash: true,
                                                    label: name.clone(),
                                                });
                                            }
                                            Err(e) => self.set_toast(e, true),
                                        }
                                    }
                                });
                            });
                        });
                    });
                ui.add_space(6.0);
            }
        });
    }
}
