use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "build_support/fdxe_select.rs"]
mod fdxe_select;

use fdxe_select::{
    MODULE_SPECS,
    ModuleSpec,
    module_enabled_with,
    module_spec,
    validate_selected_modules,
};

fn soc_rp2350_enabled() -> bool {
    env::var_os("CARGO_FEATURE_SOC_RP2350").is_some()
}

fn module_enabled(spec: ModuleSpec) -> bool {
    module_enabled_with(
        spec,
        |name| env::var_os(name).is_some(),
        soc_rp2350_enabled(),
    )
}

fn available_fdxe_modules() -> Vec<&'static str> {
    MODULE_SPECS
        .iter()
        .copied()
        .filter(|spec| module_enabled(*spec))
        .map(|spec| spec.crate_name)
        .collect()
}

fn selected_module_specs(requests: &[String]) -> Vec<ModuleSpec> {
    requests
        .iter()
        .filter_map(|module| module_spec(module))
        .filter(|spec| module_enabled(*spec))
        .collect()
}

fn requested_fdxe_modules() -> Vec<String> {
    let mut modules = Vec::new();

    if soc_rp2350_enabled() {
        modules.push("fd-bus-gpio".to_owned());
        modules.push("fd-bus-usb".to_owned());
        modules.push("fd-net-chipset-infineon-cyw43439".to_owned());
    }

    if let Some(requested) = env::var_os("FUSION_FDXE_REQUESTS") {
        let requested = requested.to_string_lossy();
        for module in requested
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !modules.iter().any(|existing| existing == module) {
                modules.push(module.to_owned());
            }
        }
    }

    for module in &modules {
        match module_spec(module) {
            Some(spec) if !module_enabled(spec) => {
                println!(
                    "cargo:warning=FDXE module request '{module}' is known but not enabled in the current firmware feature graph"
                );
            }
            Some(_) => {}
            None => {
                println!(
                    "cargo:warning=FDXE module request '{module}' is unknown to fusion-firmware build selection; verify the crate name and dependency wiring"
                );
            }
        }
    }

    modules
}

fn render_str_list(name: &str, values: &[impl AsRef<str>]) -> String {
    if values.is_empty() {
        return format!("pub const {name}: &[&str] = &[];\n");
    }

    let body = values
        .iter()
        .map(|value| format!("    {:?},\n", value.as_ref()))
        .collect::<String>();
    format!("pub const {name}: &[&str] = &[\n{body}];\n")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let shared = manifest_dir.join("../fusion-hal/fdxe/shared.rs");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("out dir"));
    let shared_out = out_dir.join("fdxe_shared.rs");
    let requests_out = out_dir.join("selected_fdxe_requests.rs");

    println!("cargo:rerun-if-changed={}", shared.display());
    println!("cargo:rerun-if-env-changed=FUSION_FDXE_REQUESTS");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SOC_RP2350");
    for spec in MODULE_SPECS {
        println!("cargo:rerun-if-env-changed={}", spec.feature_env);
    }

    let body = fs::read_to_string(&shared).expect("read shared FDXE ABI");
    fs::write(&shared_out, body).expect("write staged FDXE ABI");

    let available = available_fdxe_modules();
    let requests = requested_fdxe_modules();
    let selected_specs = selected_module_specs(&requests);
    if let Err(error) = validate_selected_modules(&selected_specs) {
        panic!("{error}");
    }
    let module_capacity = selected_specs.len();
    let driver_capacity: usize = selected_specs.iter().map(|spec| spec.drivers.len()).sum();

    let mut rendered = String::new();
    rendered.push_str(&render_str_list(
        "AVAILABLE_FDXE_MODULE_CRATE_NAMES",
        &available,
    ));
    rendered.push_str(&render_str_list(
        "REQUESTED_FDXE_MODULE_CRATE_NAMES",
        &requests,
    ));
    rendered.push_str(&format!(
        "pub const REQUESTED_FDXE_MODULE_CAPACITY: usize = {module_capacity};\n"
    ));
    rendered.push_str(&format!(
        "pub const REQUESTED_FDXE_DRIVER_CAPACITY: usize = {driver_capacity};\n"
    ));
    fs::write(&requests_out, rendered).expect("write selected FDXE request list");
}
