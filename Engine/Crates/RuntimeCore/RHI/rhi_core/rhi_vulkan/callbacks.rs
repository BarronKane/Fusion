use ash::vk;

pub unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut core::ffi::c_void
) -> vk::Bool32 {
    let callback_data = unsafe { *p_callback_data };
    let message_id_number = callback_data.message_id_number;
    let empty = "";

    let severity = match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => "ERROR",
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => "WARNING",
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => "INFO",
        vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => "VERBOSE",

        _ => "UNKNOWN"
    };

    let message_type = match message_type {
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL => "GENERAL",
        vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION => "VALIDATION",
        vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE => "PERFORMANCE",

        _ => "UNKNOWN"
    };

    let message_id_name_result = if callback_data.p_message_id_name.is_null() {
        unsafe { core::ffi::CStr::from_ptr(empty.as_ptr() as *const core::ffi::c_char).to_str() }
    } else {
        unsafe { core::ffi::CStr::from_ptr(callback_data.p_message_id_name).to_str() }
    };

    let message_result = if callback_data.p_message.is_null() {
        unsafe { core::ffi::CStr::from_ptr(empty.as_ptr() as *const core::ffi::c_char).to_str() }
    } else {
        unsafe { core::ffi::CStr::from_ptr(callback_data.p_message).to_str() }
    };

    let message_id_name = message_id_name_result.unwrap_or_else(|_| "UNKNOWN ID");
    let message = message_result.unwrap_or_else(|_| "UNKNOWN MESSAGE");

    // TODO: Strip out println for our own logging later.
    println!(
        "{severity:?}:\n{message_type:?} [{message_id_name} ({message_id_number})] : {message}\n",
    );

    vk::FALSE
}
