use crate::{s_print, s_println};

pub fn run_tests(tests: &[&dyn Fn()]) {
    use crate::{
        misc::debug_exit::{QemuExitCode, debug_exit},
        s_println,
    };

    s_println!("\nRunning {} tests", tests.len());
    for test in tests {
        test();
    }

    s_println!("\nTest success!");
    debug_exit(QemuExitCode::Success);
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

        s_println!("[OK]");
    }
}

#[macro_export]
macro_rules! test {
    ($name:literal, $test_fn: expr) => {
        #[test_case]
        #[allow(unused_imports)]
        fn __test() {
            $crate::testing::Test::new($name, $test_fn).run_test();
        }
    };
}
