#[no_mangle]
pub extern "C" fn sample_version() -> usize {
    sample_core::version().len() + sample_core::FLAVOR.len()
}
