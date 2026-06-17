mod integration;
mod unit;

use anyhow::Result;

use crate::json_output::{JsonEvent, OutputMode, emit};

pub fn test(output_mode: OutputMode, test: Option<&str>) -> Result<i32> {
    if output_mode.is_json() {
        emit(&JsonEvent::started("test"))?;
    }

    if test.is_none() {
        let unit_exit = unit::run(output_mode)?;
        if unit_exit != 0 {
            if output_mode.is_json() {
                emit(&JsonEvent::finished("test", unit_exit, "failed"))?;
            }
            return Ok(unit_exit);
        }
    }
    let integration_exit = integration::run(output_mode, test)?;
    if output_mode.is_json() {
        emit(&JsonEvent::finished(
            "test",
            integration_exit,
            if integration_exit == 0 {
                "ok"
            } else {
                "failed"
            },
        ))?;
    }
    Ok(integration_exit)
}
