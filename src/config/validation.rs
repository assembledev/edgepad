//! Scalar bounds shared by TOML parsing and CLI width validation.
//! Compatibility between bindings is checked after the whole config is parsed.

pub fn parse_edge_width(raw_value: &str) -> Result<f32, String> {
    let parsed = raw_value
        .parse::<f32>()
        .map_err(|_| "--edge-width must be > 0 and < 0.5".to_string())?;
    validate_edge_width(parsed, "--edge-width")
}

pub(super) fn validate_edge_width(parsed: f32, name: &str) -> Result<f32, String> {
    if !(parsed > 0.0 && parsed < 0.5) {
        return Err(format!("{name} must be > 0 and < 0.5"));
    }
    Ok(parsed)
}

pub(super) fn validate_optional_edge_width(
    value: Option<f32>,
    name: &str,
) -> Result<Option<f32>, String> {
    value
        .map(|width| validate_edge_width(width, name))
        .transpose()
}

pub(super) fn validate_slider_step(parsed: f32, name: &str) -> Result<f32, String> {
    if !(parsed > 0.0 && parsed <= 1.0) {
        return Err(format!("{name} must be > 0 and <= 1"));
    }
    Ok(parsed)
}

pub(super) fn validate_tap_min_duration_ms(parsed: u64, name: &str) -> Result<u64, String> {
    if parsed > 10_000 {
        return Err(format!("{name} must be <= 10000"));
    }
    Ok(parsed)
}

pub(super) fn validate_tap_max_duration_ms(parsed: u64, name: &str) -> Result<u64, String> {
    if parsed == 0 || parsed > 10000 {
        return Err(format!("{name} must be > 0 and <= 10000"));
    }
    Ok(parsed)
}

pub(super) fn validate_double_tap_timeout_ms(parsed: u64, name: &str) -> Result<u64, String> {
    if parsed == 0 || parsed > 10000 {
        return Err(format!("{name} must be > 0 and <= 10000"));
    }
    Ok(parsed)
}

pub(super) fn validate_double_tap_max_distance(parsed: f32, name: &str) -> Result<f32, String> {
    if !parsed.is_finite() || parsed <= 0.0 || parsed > 1.0 {
        return Err(format!("{name} must be > 0 and <= 1"));
    }
    Ok(parsed)
}

pub(super) fn validate_swipe_min_distance(parsed: f32, name: &str) -> Result<f32, String> {
    if !(parsed > 0.0 && parsed <= 1.0) {
        return Err(format!("{name} must be > 0 and <= 1"));
    }
    Ok(parsed)
}
