use nannou_egui::egui;
use serde_json::Value;

/// Render controls for a settings object based on a JSON schema
pub fn render_schema_ui(ui: &mut egui::Ui, settings: &mut Value, schema: &Value) -> bool {
    let mut changed = false;
    if let Some(obj) = schema.as_object() {
        if let Some(props) = obj.get("properties").and_then(|p| p.as_object()) {
            for (key, field_schema) in props {
                ui.horizontal(|ui| {
                    // Label
                    let label = field_schema.get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or(key);
                    ui.label(label);
                    
                    // Control
                    if let Some(current_value) = settings.get_mut(key) {
                        changed |= render_field(ui, current_value, field_schema);
                    } else {
                        // Value missing, insert default if available?
                        // For now just show warning
                        ui.label("Missing value");
                    }
                });
            }
        }
    }
    changed
}

fn render_field(ui: &mut egui::Ui, value: &mut Value, schema: &Value) -> bool {
    let type_name = schema.get("type").and_then(|t| t.as_str()).unwrap_or("string");
    let mut changed = false;
    
    match type_name {
        "string" => {
            if let Some(s) = value.as_str() {
                let mut text = s.to_string();
                
                // Check if it's an enum
                if let Some(variants) = schema.get("enum").and_then(|v| v.as_array()) {
                    egui::ComboBox::from_id_source(schema.get("title").map(|t| t.to_string()).unwrap_or_else(|| "combo".to_string()))
                        .selected_text(&text)
                        .show_ui(ui, |ui| {
                            for variant in variants {
                                if let Some(var_str) = variant.as_str() {
                                    if ui.selectable_value(&mut text, var_str.to_string(), var_str).changed() {
                                        *value = Value::String(text.clone());
                                        changed = true;
                                    }
                                }
                            }
                        });
                } else {
                    if ui.text_edit_singleline(&mut text).changed() {
                        *value = Value::String(text);
                        changed = true;
                    }
                }
            }
        },
        "boolean" => {
            if let Some(b) = value.as_bool() {
                let mut val = b;
                if ui.checkbox(&mut val, "").changed() {
                    *value = Value::Bool(val);
                    changed = true;
                }
            }
        },
        "integer" => {
            if let Some(n) = value.as_i64() {
                let mut num = n;
                // Check bounds
                let min = schema.get("minimum").and_then(|m| m.as_i64()).unwrap_or(i64::MIN);
                let max = schema.get("maximum").and_then(|m| m.as_i64()).unwrap_or(i64::MAX);
                
                if ui.add(egui::DragValue::new(&mut num).clamp_range(min..=max)).changed() {
                    *value = serde_json::from_str(&num.to_string()).unwrap_or(Value::Null); // Hacky conversion back to Value::Number
                    changed = true;
                }
            }
        },
        "number" => {
             if let Some(n) = value.as_f64() {
                let mut num = n;
                let min = schema.get("minimum").and_then(|m| m.as_f64()).unwrap_or(f64::MIN);
                let max = schema.get("maximum").and_then(|m| m.as_f64()).unwrap_or(f64::MAX);
                
                if ui.add(egui::DragValue::new(&mut num).clamp_range(min..=max)).changed() {
                     *value = serde_json::from_str(&num.to_string()).unwrap_or(Value::Null);
                     changed = true;
                }
             }
        }
        "array" => {
            if let Some(list) = value.as_array_mut() {
                if let Some(item_schema) = schema.get("items") {
                    ui.vertical(|ui| {
                       for (i, item) in list.iter_mut().enumerate() {
                           ui.horizontal(|ui| {
                               ui.label(format!("#{}", i));
                               changed |= render_field(ui, item, item_schema);
                           });
                       }
                    });
                }
            }
        },
        _ => {
            ui.label(format!("Unsupported type: {}", type_name));
        }
    }
    changed
}
