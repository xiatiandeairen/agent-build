include!(concat!(env!("OUT_DIR"), "/flavor.rs"));

pub fn version() -> &'static str {
    "v1"
}
#[cfg(test)]
mod tests {
    // outside the table: the sample project's own test, which A21 makes fail on purpose
    #[test]
    fn version_is_set() {
        assert!(std::env::var("SAMPLE_FAIL").is_err(), "SAMPLE_FAIL is set");
        assert_eq!(super::version().len(), 2);
    }
}
