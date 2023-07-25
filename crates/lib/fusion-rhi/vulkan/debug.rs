use super::VulkanApp;

use tracing::{
    error,
    warn,
    info,
    debug,
};

use std::ffi::{
    CStr,
    CString,
    c_char,
    c_void
};

use ash::{
    vk,
    Entry
};

unsafe extern "system" fn vulkan_debug_callback(
    flag: vk::DebugUtilsMessageSeverityFlagsEXT,
    typ: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _: *mut c_void,
) -> vk::Bool32 {
    use vk::DebugUtilsMessageSeverityFlagsEXT as Flag;

    let message = CStr::from_ptr((*p_callback_data).p_message);
    
    match flag {
        Flag::VERBOSE => debug!("{:?} - {:?}", typ, message),
        Flag::INFO => info!("{:?} - {:?}", typ, message),
        Flag::WARNING => warn!("{:?} - {:?}", typ, message),
        _ => error!("{:?} - {:?}", typ, message),
    }
    
    vk::FALSE
}

impl VulkanApp {
    pub fn check_validation_layer_support(entry: &Entry) {
            for in_layer in Self::get_layer_names().iter() {
                let ilayer = in_layer.clone();
                let ilayer = ilayer.into_string().unwrap();
                let layer = ilayer.as_str();
                let found = entry
                    .enumerate_instance_layer_properties()
                    .unwrap()
                    .iter()
                    .any(|lp| {
                        let name = unsafe { CStr::from_ptr(lp.layer_name.as_ptr()) };
                        let name = name.to_str().expect("Failed to get layer name pointer");
                        &layer == &name
                    });
        
                if !found {
                    panic!("Validation layer not supported: {}", &ilayer.clone());
                }
            }
        }

        pub fn get_layer_names() -> Vec<CString> {
            let layer_names = vec![
                CString::new("VK_LAYER_KHRONOS_validation").unwrap()
            ];

            layer_names
        }

        pub fn get_layer_names_ptrs() -> Vec<*const c_char> {
            let layer_names_ptrs = Self::get_layer_names()
                .iter()
                .map(|name| name.as_ptr())
                .collect::<Vec<_>>();

            layer_names_ptrs
        }
}
