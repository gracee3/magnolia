//! Module Picker - Egui popup for selecting modules to place on empty grid cells

use nannou_egui::egui::{self, Context, Window, Button, ScrollArea};
use talisman_core::{ModuleSchema, LayoutConfig};


/// State for the module picker popup
#[derive(Debug, Default)]
pub struct ModulePicker {
    /// Whether the picker is visible
    pub visible: bool,
    /// Target cell where the module will be placed
    pub target_cell: (usize, usize),
    /// Currently selected index in the list
    pub selected_index: usize,
}

impl ModulePicker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the picker for a specific cell
    pub fn open(&mut self, col: usize, row: usize) {
        self.visible = true;
        self.target_cell = (col, row);
        self.selected_index = 0;
    }

    /// Close the picker
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Get available modules (filter out already placed ones)
    fn get_available_modules<'a>(
        available: &'a [ModuleSchema],
        layout: &LayoutConfig,
    ) -> Vec<&'a ModuleSchema> {
        let placed_modules: std::collections::HashSet<_> = layout.tiles
            .iter()
            .map(|t| t.module.as_str())
            .collect();

        available
            .iter()
            .filter(|m| !placed_modules.contains(m.id.as_str()))
            .collect()
    }

    /// Render the picker UI
    /// Returns Some(module_id) if user selected a module
    pub fn render(
        &mut self,
        ctx: &Context,
        available_modules: &[ModuleSchema],
        layout: &LayoutConfig,
    ) -> Option<String> {
        if !self.visible {
            return None;
        }

        let available = Self::get_available_modules(available_modules, layout);
        let mut selected_module = None;
        let mut should_close = false;

        Window::new("Select Module")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Place module at ({}, {})", self.target_cell.0, self.target_cell.1));
                ui.separator();

                if available.is_empty() {
                    ui.label("No modules available to place.");
                } else {
                    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, module) in available.iter().enumerate() {
                            let is_selected = i == self.selected_index;
                            let button = Button::new(&module.name)
                                .fill(if is_selected {
                                    egui::Color32::from_rgb(0, 100, 100)
                                } else {
                                    egui::Color32::TRANSPARENT
                                });

                            if ui.add(button).clicked() {
                                selected_module = Some(module.id.clone());
                                should_close = true;
                            }

                            if is_selected {
                                ui.label(&module.description);
                            }
                        }
                    });
                }

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });

        // Handle keyboard navigation
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if self.selected_index < available.len().saturating_sub(1) {
                self.selected_index += 1;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !available.is_empty() {
            selected_module = Some(available[self.selected_index].id.clone());
            should_close = true;
        }

        if should_close {
            self.close();
        }

        selected_module
    }
}
