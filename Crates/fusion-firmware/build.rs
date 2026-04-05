use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let shared = manifest_dir.join("../fusion-hal/fdxe/shared.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("fdxe_shared.rs");

    println!("cargo:rerun-if-changed={}", shared.display());

    let body = fs::read_to_string(&shared).expect("read shared FDXE ABI");
    fs::write(&out, body).expect("write staged FDXE ABI");
}
