use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy)]
struct ModuleSpec {
    crate_name: &'static str,
    feature_env: &'static str,
    driver_count: usize,
    selected_by_soc_rp2350: bool,
}

const MODULE_SPECS: &[ModuleSpec] = &[
    ModuleSpec {
        crate_name: "fd-bus-gpio",
        feature_env: "CARGO_FEATURE_FD_BUS_GPIO",
        driver_count: 1,
        selected_by_soc_rp2350: true,
    },
    ModuleSpec {
        crate_name: "fd-bus-usb",
        feature_env: "CARGO_FEATURE_FD_BUS_USB",
        driver_count: 1,
        selected_by_soc_rp2350: false,
    },
    ModuleSpec {
        crate_name: "fd-net-chipset-infineon-cyw43439",
        feature_env: "CARGO_FEATURE_FD_NET_CHIPSET_INFINEON_CYW43439",
        driver_count: 2,
        selected_by_soc_rp2350: true,
    },
];

fn module_spec(crate_name: &str) -> Option<ModuleSpec> {
    MODULE_SPECS
        .iter()
        .copied()
        .find(|spec| spec.crate_name == crate_name)
}

fn module_enabled(spec: ModuleSpec) -> bool {
    env::var_os(spec.feature_env).is_some()
        || (spec.selected_by_soc_rp2350 && env::var_os("CARGO_FEATURE_SOC_RP2350").is_some())
}

fn available_fdxe_modules() -> Vec<&'static str> {
    MODULE_SPECS
        .iter()
        .copied()
        .filter(|spec| module_enabled(*spec))
        .map(|spec| spec.crate_name)
        .collect()
}

fn requested_fdxe_modules() -> Vec<String> {
    let mut modules = Vec::new();

    if env::var_os("CARGO_FEATURE_SOC_RP2350").is_some() {
        modules.push("fd-bus-gpio".to_owned());
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

fn requested_driver_capacity(requests: &[String]) -> usize {
    requests
        .iter()
        .filter_map(|module| module_spec(module))
        .filter(|spec| module_enabled(*spec))
        .map(|spec| spec.driver_count)
        .sum()
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
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_FD_BUS_GPIO");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_FD_BUS_USB");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_FD_NET_CHIPSET_INFINEON_CYW43439");

    let body = fs::read_to_string(&shared).expect("read shared FDXE ABI");
    fs::write(&shared_out, body).expect("write staged FDXE ABI");

    let available = available_fdxe_modules();
    let requests = requested_fdxe_modules();
    let module_capacity = requests
        .iter()
        .filter_map(|module| module_spec(module))
        .filter(|spec| module_enabled(*spec))
        .count();
    let driver_capacity = requested_driver_capacity(&requests);

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
