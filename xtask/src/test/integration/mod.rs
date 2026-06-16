mod kernel_images;
mod panic_handler_smoke;
mod userspace_boot;

use anyhow::Result;

trait IntegrationTest {
    fn test_count(&self) -> usize;
    fn run(&self) -> Result<Vec<IntegrationTestResult>>;
}

pub struct IntegrationTestResult {
    pub name: String,
    pub exit_code: i32,
    pub failure: Option<String>,
    pub output: String,
}

pub fn run() -> Result<i32> {
    let tests = integration_tests();
    eprintln!();
    let test_count = tests.iter().map(|test| test.test_count()).sum::<usize>();
    eprintln!("running {test_count} integration tests");

    for test in tests {
        let results = test.run()?;
        for result in results {
            eprint!("test {} ... ", result.name);
            if result.exit_code == 0 {
                eprintln!("ok");
            } else {
                eprintln!("FAILED");
                report_failure(&result);
                return Ok(result.exit_code);
            }
        }
    }

    Ok(0)
}

fn report_failure(result: &IntegrationTestResult) {
    eprintln!();
    eprintln!("failures:");
    eprintln!();
    eprintln!("---- {} stdout ----", result.name);
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
