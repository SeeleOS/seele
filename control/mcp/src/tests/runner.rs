use super::{config::RunTestsConfig, kernel_unit, ltp};
use crate::{Event, JobContext, Report, TestEvent, TestSelector};
use anyhow::Result;
use std::path::Path;

pub fn run_tests(repo: &Path, config: &RunTestsConfig, context: &JobContext) -> Result<i32> {
    let selector = TestSelector::parse(config.selector.as_deref());
    context.event(Event::Test(TestEvent::Started {
        selector: selector.clone(),
    }));

    let mut failed = 0;
    let mut passed = 0;
    let mut skipped = 0;

    if matches!(
        selector,
        TestSelector::Default | TestSelector::Full | TestSelector::KernelUnit
    ) {
        let report = kernel_unit::run(repo, config, context)?;
        if report.passed {
            passed += 1;
        } else {
            failed += 1;
        }
        context.report(Report::KernelUnit(report));
        if failed != 0 {
            context.event(Event::Test(TestEvent::Finished {
                passed,
                failed,
                skipped,
            }));
            return Ok(1);
        }
    }

    if matches!(
        selector,
        TestSelector::Default | TestSelector::Full | TestSelector::Ltp
    ) {
        let report = ltp::run(repo, config, context)?;
        failed += report.failed;
        passed += report.passed;
        skipped += report.skipped;
        context.report(Report::Ltp(report));
    }

    context.event(Event::Test(TestEvent::Finished {
        passed,
        failed,
        skipped,
    }));
    Ok(if failed == 0 { 0 } else { 1 })
}
