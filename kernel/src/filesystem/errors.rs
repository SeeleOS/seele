#[derive(Clone, Copy, Debug)]
pub enum FSError {
    NotFound,
    NotADirectory,
    NotAFile,
}
