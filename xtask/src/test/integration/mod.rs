mod kernel_images;
mod panic_handler_smoke;
mod userspace_boot;

use anyhow::Result;
use owo_colors::OwoColorize;

trait IntegrationTest {
    fn name(&self) -> &'static str;
    fn run(&self) -> Result<IntegrationTestResult>;
}

pub struct IntegrationTestResult {
    pub exit_code: i32,
    pub failure: Option<String>,
    pub output: String,
}

pub fn run() -> Result<i32> {
    let tests = integration_tests();
    eprintln!();
    eprintln!("running {} integration tests", tests.len());

    for test in tests {
        let result = test.run()?;
        eprint!("test {} ... ", test.name());
        if result.exit_code == 0 {
            eprintln!("{}", "ok".green().bold());
        } else {
            eprintln!("{}", "FAILED".red().bold());
            report_failure(test.name(), &result);
            return Ok(result.exit_code);
        }
    }

    Ok(0)
}

fn report_failure(name: &str, result: &IntegrationTestResult) {
    eprintln!();
    eprintln!("{}", "failures:".red().bold());
    eprintln!();
    eprintln!("---- {name} stdout ----");
    if let Some(failure) = &result.failure {
        eprintln!("{failure}");
    }
    if !result.output.is_empty() {
        eprint!("{}", result.output);
        if !result.output.ends_with('\n') {
            eprintln!();
        }
    }
    eprintln!();
}

fn integration_tests() -> [&'static dyn IntegrationTest; 3] {
    [
        &kernel_images::KERNEL_IMAGES,
        &userspace_boot::USERSPACE_BOOT,
        &panic_handler_smoke::PANIC_HANDLER_SMOKE,
    ]
}
