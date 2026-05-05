use crate::{
    misc::debug_exit::{QemuExitCode, debug_exit},
    misc::hlt_loop,
    s_print, s_println,
};
use owo_colors::OwoColorize;

pub fn run_tests(tests: &[&dyn Fn()]) -> ! {
    s_println!("\nRunning {} tests", tests.len().bold());
    for test in tests {
        test();
    }

    debug_exit(QemuExitCode::Success);
    hlt_loop();
}

pub struct Test {
    name: &'static str,
    test: fn(),
}

impl Test {
    pub fn new(name: &'static str, test: fn()) -> Self {
        Self { name, test }
    }

    pub fn run_test(&self) {
        s_print!("{} ", self.name);

        ((self.test)());

        s_println!("{}", "OK".green().bold());
    }
}

#[macro_export]
macro_rules! test {
    ($name:literal, $test_fn: expr) => {
        #[test_case]
        #[allow(unused_imports)]
        fn __test() {
            $crate::misc::testing::Test::new($name, $test_fn).run_test();
        }
    };
}
