mod kernel_images;
mod panic_handler_smoke;
mod userspace_boot;

use anyhow::Result;

trait IntegrationTest {
    fn name(&self) -> &'static str;
    fn run(&self) -> Result<i32>;
}

pub fn run() -> Result<i32> {
    for test in integration_tests() {
        eprintln!("running integration test: {}", test.name());
        let exit_code = test.run()?;
        if exit_code != 0 {
            return Ok(exit_code);
        }
    }

    Ok(0)
}

fn integration_tests() -> [&'static dyn IntegrationTest; 3] {
    [
        &kernel_images::KERNEL_IMAGES,
        &userspace_boot::USERSPACE_BOOT,
        &panic_handler_smoke::PANIC_HANDLER_SMOKE,
    ]
}
