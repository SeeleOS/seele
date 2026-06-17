mod integration;
mod unit;

use anyhow::Result;

use crate::reporter::{FinishStatus, WorkflowReporter, finished, started};

pub fn test(reporter: &dyn WorkflowReporter, test: Option<&str>) -> Result<i32> {
    started(reporter, "test")?;

    if test.is_none() {
        let unit_exit = unit::run(reporter)?;
        if unit_exit != 0 {
            finished(reporter, "test", unit_exit, FinishStatus::Failed)?;
            return Ok(unit_exit);
        }
    }
    let integration_exit = integration::run(reporter, test)?;
    finished(
        reporter,
        "test",
        integration_exit,
        FinishStatus::from_exit_code(integration_exit),
    )?;
    Ok(integration_exit)
}
