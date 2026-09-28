//! Embed the committed bundle; Rust builds never invoke Node.
//!
//! Embedding is forced in every profile (memory-serve would otherwise read `web/dist` from disk
//! in debug builds), so `cargo test` exercises the same embedded assets a release binary serves.
fn main() {
    println!("cargo:rerun-if-changed=web/dist");
    memory_serve::load_directory_with_embed("web/dist", true);
}
