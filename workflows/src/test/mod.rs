mod integration;
mod unit;

use anyhow::Result;

use crate::reporter::{FinishStatus, WorkflowReporter, finished, started};

const FULL_TEST_FILTER: &str = "full";

pub fn test(reporter: &dyn WorkflowReporter, test: Option<&str>) -> Result<i32> {
    started(reporter, "test")?;

    if run_unit_tests(test) {
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

fn run_unit_tests(test: Option<&str>) -> bool {
    test.is_none_or(|test| test == FULL_TEST_FILTER)
}
