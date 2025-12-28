//! Modal Layer - Centralized modal management for egui dialogs
//!
//! This module provides a unified system for managing modal dialogs,
//! ensuring proper z-ordering, focus trapping, and keyboard handling.

use nannou_egui::egui::{self, Context};

/// The type of modal currently active
#[derive(Debug, Clone, PartialEq)]
pub enum ModalType {
    /// Close confirmation dialog (triggered by ESC in Normal mode)
    CloseConfirmation,
    /// Module picker for placing modules in empty grid cells
    ModulePicker { col: usize, row: usize },
    /// Tile settings (right-click context)
    TileSettings { tile_id: String },
    /// Global application settings
    GlobalSettings,
    /// Patch bay editor
    PatchBay,
}

/// Result of a modal action
#[derive(Debug, Clone)]
pub enum ModalAction {
    /// User confirmed/selected something
    Confirm(ModalResult),
    /// User cancelled the modal
    Cancel,
    /// Modal is still active, no action yet
    None,
}

/// Specific results from different modal types
#[derive(Debug, Clone)]
pub enum ModalResult {
    /// Exit application
    ExitApp,
    /// Place a module at a cell
    PlaceModule { module_id: String, col: usize, row: usize },
    /// Toggle tile enabled state
    ToggleTile { tile_id: String },
    /// Delete a tile
    DeleteTile { tile_id: String },
}

/// Centralized modal layer for the application
#[derive(Debug, Default)]
pub struct ModalLayer {
    /// Currently active modal, if any
    pub active_modal: Option<ModalType>,
}

impl ModalLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any modal is currently active
    pub fn is_active(&self) -> bool {
        self.active_modal.is_some()
    }

    /// Open a specific modal
    pub fn open(&mut self, modal: ModalType) {
        log::debug!("Opening modal: {:?}", modal);
        self.active_modal = Some(modal);
    }

    /// Close the current modal
    pub fn close(&mut self) {
        log::debug!("Closing modal");
        self.active_modal = None;
    }

    /// Render the current modal and return any action taken
    ///
    /// This should be called at the end of the egui update to ensure
    /// modals render on top of other UI elements.
    pub fn render(&mut self, ctx: &Context) -> ModalAction {
        let Some(modal_type) = &self.active_modal.clone() else {
            return ModalAction::None;
        };

        match modal_type {
            ModalType::CloseConfirmation => self.render_close_confirmation(ctx),
            ModalType::ModulePicker { col, row } => {
                // Placeholder - will be integrated with existing ModulePicker
                self.render_placeholder(ctx, &format!("Module Picker ({}, {})", col, row))
            }
            ModalType::TileSettings { tile_id } => {
                self.render_placeholder(ctx, &format!("Tile Settings: {}", tile_id))
            }
            ModalType::GlobalSettings => {
                self.render_placeholder(ctx, "Global Settings")
            }
            ModalType::PatchBay => {
                self.render_placeholder(ctx, "Patch Bay Editor")
            }
        }
    }

    /// Render the close confirmation dialog
    fn render_close_confirmation(&mut self, ctx: &Context) -> ModalAction {
        let screen_rect = ctx.screen_rect();
        let width = 300.0;
        let height = 120.0;
        let x = screen_rect.center().x - width / 2.0;
        let y = screen_rect.center().y - height / 2.0;

        let mut action = ModalAction::None;

        egui::Window::new("Confirm Close")
            .fixed_pos(egui::pos2(x, y))
            .fixed_size(egui::vec2(width, height))
            .collapsible(false)
            .resizable(false)
            .frame(egui::Frame {
                fill: egui::Color32::from_rgba_unmultiplied(20, 20, 20, 250),
                stroke: egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 100, 100)),
                inner_margin: egui::Margin::same(20.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Exit Talisman?")
                            .heading()
                            .color(egui::Color32::from_rgb(255, 200, 200)),
                    );

                    ui.add_space(15.0);

                    ui.horizontal(|ui| {
                        ui.add_space(30.0);
                        if ui
                            .button(
                                egui::RichText::new("  Quit  ")
                                    .color(egui::Color32::from_rgb(255, 100, 100)),
                            )
                            .clicked()
                        {
                            action = ModalAction::Confirm(ModalResult::ExitApp);
                        }
                        ui.add_space(20.0);
                        if ui.button("Cancel").clicked() {
                            action = ModalAction::Cancel;
                        }
                    });
                });
            });

        // Handle keyboard shortcuts
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            action = ModalAction::Confirm(ModalResult::ExitApp);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = ModalAction::Cancel;
        }

        // Close modal on Cancel
        if matches!(action, ModalAction::Cancel) {
            self.close();
        }

        action
    }

    /// Render a placeholder modal for unimplemented types
    fn render_placeholder(&mut self, ctx: &Context, title: &str) -> ModalAction {
        let screen_rect = ctx.screen_rect();
        let width = 400.0;
        let height = 200.0;
        let x = screen_rect.center().x - width / 2.0;
        let y = screen_rect.center().y - height / 2.0;

        let mut action = ModalAction::None;

        egui::Window::new(title)
            .fixed_pos(egui::pos2(x, y))
            .fixed_size(egui::vec2(width, height))
            .collapsible(false)
            .resizable(false)
            .frame(egui::Frame {
                fill: egui::Color32::from_rgba_unmultiplied(30, 30, 30, 250),
                stroke: egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 100, 100)),
                inner_margin: egui::Margin::same(20.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(format!("{} - Coming Soon", title));
                    ui.add_space(20.0);
                    if ui.button("Close").clicked() {
                        action = ModalAction::Cancel;
                    }
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = ModalAction::Cancel;
        }

        if matches!(action, ModalAction::Cancel) {
            self.close();
        }

        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_layer_default_inactive() {
        let layer = ModalLayer::new();
        assert!(!layer.is_active());
    }

    #[test]
    fn test_modal_layer_open_close() {
        let mut layer = ModalLayer::new();
        
        layer.open(ModalType::CloseConfirmation);
        assert!(layer.is_active());
        assert!(matches!(layer.active_modal, Some(ModalType::CloseConfirmation)));
        
        layer.close();
        assert!(!layer.is_active());
    }
}
