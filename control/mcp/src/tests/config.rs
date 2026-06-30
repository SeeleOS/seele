#[derive(Debug, Clone, Default)]
pub struct RunTestsConfig {
    pub selector: Option<String>,
    pub ltp_suite: Option<String>,
    pub ltp_pattern: Option<String>,
    pub enable_profiling: bool,
}
