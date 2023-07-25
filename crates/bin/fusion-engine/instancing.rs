#[cfg(target_os = "windows")]
use windows;

use std::error::Error;

#[derive(Debug)]
pub struct InstanceError(String);
impl Error for InstanceError {}
impl std::fmt::Display for InstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "INSTANCING: {}", self.0)
    }
}
pub type Result<T> = std::result::Result<T, InstanceError>;

pub use self::instance::*;

#[cfg(target_os = "windows")]
mod instance {
    use super::InstanceError;
    use super::Result;

    use windows::Win32::{
        Foundation,
        Foundation::HANDLE,

        System::Threading,
    };

    use std::collections::HashMap;

    pub struct InstanceMap {
        instances: HashMap<String, Instance>
    }

    impl InstanceMap {
        pub fn new(name: &str) -> Result<InstanceMap> {

            let mut instance_map = InstanceMap {
                instances: HashMap::new(),
            };

            let instance = Instance::new(name)?;
            
            instance_map.push(name, instance)?;

            Ok(instance_map)
        }

        pub fn try_push(&mut self, name: &str) -> Result<&mut Self> {
            if self.is_mapped(name) {
                Err(InstanceError("Instance exists!".to_string()))
            } else {
                let instance = Instance::new(name)?;
                Ok(self.push(name, instance)?)
            }
        }

        fn push(&mut self, name: &str, instance: Instance) -> Result<&mut Self> {
            let i = self.instances.insert(name.to_string(), instance);
            match i {
                None => Ok(self),
                Some(_) => {
                    panic!("Tried to instance something already instanced! This should be unreachable.")
                }
            }
        }

        pub fn is_mapped(&self, name: &str) -> bool {
            if !self.instances.contains_key(name) {
                false
            } else {
                true
            }
        }

        pub fn is_active(&self, name: &str) -> bool {
            if !self.instances.contains_key(name) {
                false
            } else {
                self.instances[name].exists()
            }
        }

        pub fn release_instance(&mut self, name: &str) -> Result<&mut Self> {
            if self.is_mapped(name) && self.is_active(name) {
                self.instances.get_mut(name).unwrap().release();
                Ok(self)
            } else {
                Err(InstanceError("Instance doesn't exist!".to_string()))
            }            
        }

        pub fn restart_instance(&mut self, name: &str) -> Result<&mut Self> {
            if self.is_mapped(name) && !self.is_active(name) {
                self.instances.get_mut(name).unwrap().create_handle(name)?;
                Ok(self)
            } else {
                Err(InstanceError("Could not reinstate instance mutex!".to_string()))
            }
        }
    }

    struct Instance {
        handle: Option<HANDLE>,
    }

    unsafe impl Send for Instance {}
    unsafe impl Sync for Instance {}

    impl Instance {
        fn new(name: &str) -> Result<Self> {
            let mut instance = Instance { handle: None };

            instance.create_handle(name)?;
            Ok(instance)
        }

        fn exists(&self) -> bool {
            self.handle.is_some()
        }

        fn release(&mut self) {
            if let Some(h) = self.handle.take() {
                unsafe {
                    Foundation::CloseHandle(h);
                }
            }
        }

        fn create_handle(&mut self, name: &str) -> Result<&Self> {
            unsafe {
                let handle = Threading::CreateMutexW(std::ptr::null_mut(), Foundation::BOOL(0), Foundation::PWSTR(name.encode_utf16().chain(Some(0)).collect::<Vec<_>>().as_mut_ptr()));
                let lerr = Foundation::GetLastError();

                if handle.is_invalid() || handle.0 as u32 == Foundation::ERROR_INVALID_HANDLE {
                    Err(InstanceError("Windows handle invalid!".to_string()))
                } else if lerr == Foundation::ERROR_ALREADY_EXISTS {
                    /*
                    Foundation::CloseHandle(handle);
                    Ok(Instance{ handle: None })
                    */
                    Err(InstanceError("Handle exists! Is process already running?".to_string()))
                } else {
                    self.handle = Some(handle);
                    Ok(self)
                }
            }
        }
    }

    impl Drop for Instance {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                unsafe {
                    Foundation::CloseHandle(handle);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod instance {

}

#[cfg(target_os = "macos")]
mod instance {

}
