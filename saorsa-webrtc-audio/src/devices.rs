//! Audio device enumeration helpers.

use crate::{AudioError, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// A named audio device of one direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// OS-reported device name.
    pub name: String,
    /// Whether this is the host default for its direction.
    pub is_default: bool,
}

fn list(input: bool) -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_name = if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    }
    .and_then(|d| d.name().ok());

    let devices = if input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|e| AudioError::Enumeration(e.to_string()))?;

    Ok(devices
        .filter_map(|d| d.name().ok())
        .map(|name| DeviceInfo {
            is_default: Some(&name) == default_name.as_ref(),
            name,
        })
        .collect())
}

/// Enumerate input (capture) devices.
pub fn input_devices() -> Result<Vec<DeviceInfo>> {
    list(true)
}

/// Enumerate output (playout) devices.
pub fn output_devices() -> Result<Vec<DeviceInfo>> {
    list(false)
}

pub(crate) fn find_input(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_input_device()
            .ok_or(AudioError::NoDefaultDevice("input")),
        Some(wanted) => host
            .input_devices()
            .map_err(|e| AudioError::Enumeration(e.to_string()))?
            .find(|d| d.name().map(|n| n == wanted).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(wanted.to_string())),
    }
}

pub(crate) fn find_output(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_output_device()
            .ok_or(AudioError::NoDefaultDevice("output")),
        Some(wanted) => host
            .output_devices()
            .map_err(|e| AudioError::Enumeration(e.to_string()))?
            .find(|d| d.name().map(|n| n == wanted).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(wanted.to_string())),
    }
}
