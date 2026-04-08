use fusion_firmware::FirmwareBootstrapContext;
use fusion_sys::{
    context,
    courier,
};

#[fusion_firmware::fusion_firmware_main]
fn main(_bootstrap: &FirmwareBootstrapContext) -> ! {
    let Ok(_context_id) = context::local::id() else {
        std::process::abort();
    };
    let Ok(context_courier_id) = context::local::courier_id() else {
        std::process::abort();
    };
    let Ok(courier_id) = courier::local::id() else {
        std::process::abort();
    };
    if context_courier_id != courier_id {
        std::process::abort();
    }
    std::process::exit(0)
}
