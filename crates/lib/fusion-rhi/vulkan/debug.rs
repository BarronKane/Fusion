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
    Entry,
    Instance, extensions::ext::DebugUtils
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
        Flag::ERROR => error!("{:?} - {:?}", typ, message),
        _ => error!("UKNOWN FLAG: {:?} - {:?}", typ, message)
    }
    
    vk::FALSE
}

impl VulkanApp {
    pub fn setup_debug_messenger(
        enable_validation: bool,
        entry: &Entry,
        instance: &Instance,
    ) -> Option<(DebugUtils, vk::DebugUtilsMessengerEXT)> {
        if enable_validation {
            return None;
        }

        use vk::DebugUtilsMessageSeverityFlagsEXT as Flag;
        use vk::DebugUtilsMessageTypeFlagsEXT as Type;

        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT {
            s_type:     vk::StructureType::DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT,
            p_next:     std::ptr::null(),
            flags:      vk::DebugUtilsMessengerCreateFlagsEXT::empty(),

            message_severity:
                Flag::VERBOSE       |
                Flag::INFO          |
                Flag::WARNING       |
                Flag::ERROR,

            message_type:
                Type::GENERAL       |
                Type::PERFORMANCE   |
                Type::VALIDATION    |
                Type::DEVICE_ADDRESS_BINDING,

            pfn_user_callback: Some(vulkan_debug_callback),
            p_user_data:    std::ptr::null_mut()
        };

        let debug_utils = DebugUtils::new(entry, instance);
        let debug_utils_messenger = unsafe {
            debug_utils
                .create_debug_utils_messenger(&debug_info, None)
                .unwrap()
        };

        Some((debug_utils, debug_utils_messenger))
    }

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
