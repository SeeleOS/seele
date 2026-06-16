mod integration;
mod unit;

use anyhow::Result;

pub fn test() -> Result<i32> {
    let unit_exit = unit::run()?;
    if unit_exit != 0 {
        return Ok(unit_exit);
    }
    integration::run()
}
