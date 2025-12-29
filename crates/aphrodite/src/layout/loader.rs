use crate::layout::types::WheelDefinitionWithPresets;
use serde_json;
use thiserror::Error;

/// Errors that can occur when loading wheel definitions
#[derive(Error, Debug)]
pub enum WheelDefinitionError {
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid field value: {0}")]
    InvalidFieldValue(String),
}

/// Load a wheel definition from JSON string
pub fn load_wheel_definition_from_json(
    json: &str,
) -> Result<WheelDefinitionWithPresets, WheelDefinitionError> {
    let parsed: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| WheelDefinitionError::InvalidJson(e.to_string()))?;

    validate_wheel_definition(&parsed)?;

    serde_json::from_value(parsed)
        .map_err(|e| WheelDefinitionError::ValidationError(e.to_string()))
}

/// Validate a wheel definition
fn validate_wheel_definition(
    definition: &serde_json::Value,
) -> Result<(), WheelDefinitionError> {
    // Check that it's an object
    if !definition.is_object() {
        return Err(WheelDefinitionError::ValidationError(
            "Wheel definition must be an object".to_string(),
        ));
    }

    let obj = definition.as_object().unwrap();

    // Validate name
    if !obj.contains_key("name") {
        return Err(WheelDefinitionError::MissingField("name".to_string()));
    }
    if let Some(name) = obj.get("name") {
        if !name.is_string() || name.as_str().unwrap().is_empty() {
            return Err(WheelDefinitionError::InvalidFieldValue(
                "name must be a non-empty string".to_string(),
            ));
        }
    }

    // Validate rings
    if !obj.contains_key("rings") {
        return Err(WheelDefinitionError::MissingField("rings".to_string()));
    }
    let rings = obj
        .get("rings")
        .ok_or_else(|| WheelDefinitionError::MissingField("rings".to_string()))?;
    if !rings.is_array() {
        return Err(WheelDefinitionError::InvalidFieldValue(
            "rings must be an array".to_string(),
        ));
    }
    let rings_array = rings.as_array().unwrap();
    if rings_array.is_empty() {
        return Err(WheelDefinitionError::InvalidFieldValue(
            "rings must have at least one ring".to_string(),
        ));
    }

    // Validate each ring
    for (index, ring) in rings_array.iter().enumerate() {
        validate_ring_definition(ring, index)?;
    }

    // Validate optional fields
    if let Some(description) = obj.get("description") {
        if !description.is_null() && !description.is_string() {
            return Err(WheelDefinitionError::InvalidFieldValue(
                "description must be a string".to_string(),
            ));
        }
    }

    if let Some(version) = obj.get("version") {
        if !version.is_null() && !version.is_string() {
            return Err(WheelDefinitionError::InvalidFieldValue(
                "version must be a string".to_string(),
            ));
        }
        if let Some(version_str) = version.as_str() {
            // Validate version format (semver-like: major.minor.patch)
            let version_regex = regex::Regex::new(r"^\d+\.\d+\.\d+$")
                .map_err(|_| WheelDefinitionError::ValidationError("Regex error".to_string()))?;
            if !version_regex.is_match(version_str) {
                return Err(WheelDefinitionError::InvalidFieldValue(format!(
                    "version must be in format major.minor.patch (e.g., \"1.0.0\"), got: {}",
                    version_str
                )));
            }
        }
    }

    if let Some(author) = obj.get("author") {
        if !author.is_null() && !author.is_string() {
            return Err(WheelDefinitionError::InvalidFieldValue(
                "author must be a string".to_string(),
            ));
        }
    }

    if let Some(tags) = obj.get("tags") {
        if !tags.is_null() {
            if !tags.is_array() {
                return Err(WheelDefinitionError::InvalidFieldValue(
                    "tags must be an array".to_string(),
                ));
            }
            let tags_array = tags.as_array().unwrap();
            for tag in tags_array {
                if !tag.is_string() {
                    return Err(WheelDefinitionError::InvalidFieldValue(
                        "tags must be an array of strings".to_string(),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validate a ring definition
fn validate_ring_definition(
    ring: &serde_json::Value,
    index: usize,
) -> Result<(), WheelDefinitionError> {
    if !ring.is_object() {
        return Err(WheelDefinitionError::InvalidFieldValue(format!(
            "Ring at index {} must be an object",
            index
        )));
    }

    let ring_obj = ring.as_object().unwrap();

    // Validate slug
    if !ring_obj.contains_key("slug") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].slug",
            index
        )));
    }
    if let Some(slug) = ring_obj.get("slug") {
        if !slug.is_string() || slug.as_str().unwrap().is_empty() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].slug must be a non-empty string",
                index
            )));
        }
    }

    // Validate type
    if !ring_obj.contains_key("type") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].type",
            index
        )));
    }
    if let Some(ring_type) = ring_obj.get("type") {
        if !ring_type.is_string() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].type must be a string",
                index
            )));
        }
        let type_str = ring_type.as_str().unwrap();
        if !["signs", "houses", "planets", "aspects"].contains(&type_str) {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].type must be one of: signs, houses, planets, aspects",
                index
            )));
        }
    }

    // Validate label
    if !ring_obj.contains_key("label") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].label",
            index
        )));
    }
    if let Some(label) = ring_obj.get("label") {
        if !label.is_string() || label.as_str().unwrap().is_empty() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].label must be a non-empty string",
                index
            )));
        }
    }

    // Validate orderIndex
    if !ring_obj.contains_key("orderIndex") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].orderIndex",
            index
        )));
    }
    if let Some(order_index) = ring_obj.get("orderIndex") {
        if !order_index.is_number() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].orderIndex must be a number",
                index
            )));
        }
    }

    // Validate radiusInner
    if !ring_obj.contains_key("radiusInner") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].radiusInner",
            index
        )));
    }
    if let Some(radius_inner) = ring_obj.get("radiusInner") {
        if !radius_inner.is_number() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].radiusInner must be a number",
                index
            )));
        }
        let radius_val = radius_inner.as_f64().unwrap();
        if radius_val < 0.0 || radius_val > 1.0 {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].radiusInner must be between 0 and 1",
                index
            )));
        }
    }

    // Validate radiusOuter
    if !ring_obj.contains_key("radiusOuter") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].radiusOuter",
            index
        )));
    }
    if let Some(radius_outer) = ring_obj.get("radiusOuter") {
        if !radius_outer.is_number() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].radiusOuter must be a number",
                index
            )));
        }
        let radius_val = radius_outer.as_f64().unwrap();
        if radius_val < 0.0 || radius_val > 1.0 {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].radiusOuter must be between 0 and 1",
                index
            )));
        }
    }

    // Validate radiusInner < radiusOuter
    if let (Some(inner), Some(outer)) = (
        ring_obj.get("radiusInner"),
        ring_obj.get("radiusOuter"),
    ) {
        let inner_val = inner.as_f64().unwrap();
        let outer_val = outer.as_f64().unwrap();
        if inner_val >= outer_val {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].radiusInner must be less than radiusOuter",
                index
            )));
        }
    }

    // Validate dataSource
    if !ring_obj.contains_key("dataSource") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].dataSource",
            index
        )));
    }
    let data_source = ring_obj
        .get("dataSource")
        .ok_or_else(|| WheelDefinitionError::MissingField(format!("rings[{}].dataSource", index)))?;
    if !data_source.is_object() {
        return Err(WheelDefinitionError::InvalidFieldValue(format!(
            "rings[{}].dataSource must be an object",
            index
        )));
    }

    let data_source_obj = data_source.as_object().unwrap();
    if !data_source_obj.contains_key("kind") {
        return Err(WheelDefinitionError::MissingField(format!(
            "rings[{}].dataSource.kind",
            index
        )));
    }

    if let Some(kind) = data_source_obj.get("kind") {
        if !kind.is_string() {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].dataSource.kind must be a string",
                index
            )));
        }
        let kind_str = kind.as_str().unwrap();
        let valid_kinds = [
            "static_zodiac",
            "static_nakshatras",
            "layer_houses",
            "layer_planets",
            "layer_varga_planets",
            "aspect_set",
        ];
        if !valid_kinds.contains(&kind_str) {
            return Err(WheelDefinitionError::InvalidFieldValue(format!(
                "rings[{}].dataSource.kind must be one of: {}",
                index,
                valid_kinds.join(", ")
            )));
        }

        // Validate layer-specific requirements
        if kind_str == "layer_houses" || kind_str == "layer_planets" {
            if !data_source_obj.contains_key("layerId") {
                return Err(WheelDefinitionError::MissingField(format!(
                    "rings[{}].dataSource.layerId (required for {})",
                    index, kind_str
                )));
            }
            if let Some(layer_id) = data_source_obj.get("layerId") {
                if !layer_id.is_string() || layer_id.as_str().unwrap().is_empty() {
                    return Err(WheelDefinitionError::InvalidFieldValue(format!(
                        "rings[{}].dataSource.layerId must be a non-empty string",
                        index
                    )));
                }
            }
        }

        if kind_str == "layer_varga_planets" {
            if !data_source_obj.contains_key("layerId") || !data_source_obj.contains_key("vargaId") {
                return Err(WheelDefinitionError::MissingField(format!(
                    "rings[{}].dataSource.layerId and vargaId (required for layer_varga_planets)",
                    index
                )));
            }
        }

        if kind_str == "aspect_set" {
            if !data_source_obj.contains_key("aspectSetId") {
                return Err(WheelDefinitionError::MissingField(format!(
                    "rings[{}].dataSource.aspectSetId (required for aspect_set)",
                    index
                )));
            }
            if let Some(aspect_set_id) = data_source_obj.get("aspectSetId") {
                if !aspect_set_id.is_string() || aspect_set_id.as_str().unwrap().is_empty() {
                    return Err(WheelDefinitionError::InvalidFieldValue(format!(
                        "rings[{}].dataSource.aspectSetId must be a non-empty string",
                        index
                    )));
                }
            }
        }
    }

    Ok(())
}

