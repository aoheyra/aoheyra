//! Pinned UniFFI CLI wrapper used by the Android Gradle binding task.
//!
//! The wrapper exists only with the `bindgen` feature so the shipped cdylib
//! does not link the generator. The Gradle task calls this exact version,
//! keeping generated bindings reproducible.

fn main() {
    uniffi::uniffi_bindgen_main();
}
