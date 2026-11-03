mod error;

pub use error::CoreError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_error_displays_message() {
        assert!(CoreError::Invalid("x".into()).to_string().contains("x"));
    }
}
