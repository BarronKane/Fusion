pub mod engine_api;
pub mod instancing;

use libloading as ll;
use notify;
use std::{path::{Path, PathBuf}, time};

use fusion_util as util;

#[cfg(feature = "monolithic")]
use fusion_editor;

pub struct LiveLib {
    lib_path: PathBuf,
    current_lib: PathBuf,
    lib: Option<ll::Library>,
}

impl LiveLib {
    pub fn new(lib_name: &str) -> Self {

        // Windows
        let extension = "dll";
        let rpath = util::get_cwd().unwrap();
        let mut lib_path = rpath.join(lib_name);
        lib_path.set_extension(extension);
        println!("lib_path: {}", lib_path.display());

        let (library, new_lib) = reload_library(&lib_path);
        println!("reloaded_lib_name: {}", new_lib.display());

        LiveLib {
            lib_path,
            current_lib: new_lib,
            lib: Some(library),
        }
    }
    
    pub fn load_symbol<S>(&self, symbol_name: &str) -> ll::Symbol<S> {
        let lib = match &self.lib {
            Some(p) => p,
            None => panic!("Library not loaded!"),
        };
        unsafe { 
            lib.get(symbol_name.as_bytes())
                .expect(format!("Failed to find symbol '{:?}'", symbol_name).as_str())
        }
    }

    pub fn update(&mut self) -> &Self {
        self.unload();
        let(library, new_lib) = reload_library(&self.lib_path);
        self.lib = Some(library);
        self.current_lib = new_lib;
        self
    }

    pub fn unload(&mut self) -> &Self {
        println!("Removing: {}, {}", &self.current_lib.is_file(), &self.current_lib.display());
        self.lib = None;
        std::fs::remove_file(&self.current_lib).expect("Could not remove library.");
        self
    }
}

impl Drop for LiveLib {
    fn drop(&mut self) {
        self.unload();
    }
}

fn reload_library(lib_path: &PathBuf) -> (ll::Library, PathBuf) {
    let unique_name = {
        let timestamp = time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut new_lib = lib_path.clone();
        new_lib.set_file_name(format!("{}-{}", lib_path.file_stem().unwrap().to_str().unwrap(), timestamp));
        new_lib.set_extension("dll");
        new_lib
    };
    println!("{}", unique_name.display());
    std::fs::copy(lib_path, &unique_name).expect("Failed to copy new lib!");
    let unique_lib = Path::new(&unique_name);

    unsafe { (ll::Library::new(unique_lib.as_os_str()).expect(format!("Failed to load lib: '{:?}'", unique_lib).as_str()), unique_lib.to_path_buf()) }
}

fn main() {
    let _instances = instancing::InstanceMap::new("fusion-engine");
        let _ = match _instances {
            Ok(_instances) => _instances,
            Err(e) => panic!("Process already running: {}", e)
        };

    scoped_main();        
}

fn scoped_main() {
    #[cfg(feature = "monolithic")]
    monolithic_main();
    #[cfg(not(feature = "monolithic"))]
    shared_main();

}

#[cfg(feature = "monolithic")]
fn monolithic_main() {
    let state = fusion_editor::init();
    unsafe {
        let check = fusion_editor::update(state);
    }
}

#[cfg(not(feature = "monolithic"))]
fn shared_main() {
    let mut live_lib = LiveLib::new("fusion_editor");
    let lib = match &live_lib.lib {
        Some(p) => p.clone(),
        None => panic!("Library not loaded!"),        
    };
    let mut api = unsafe { 
        engine_api::AppApi::new(lib)
    };
    let mut app_state = (api.init)();

    let mut terminate: bool = false;
    while terminate == false {
        if (api.update)(app_state) {
            (api.unload)(app_state);
            live_lib.update();
            let _lib = match &live_lib.lib {
                Some(p) => p.clone(),
                None => panic!("Library not loaded!"),        
            };
            api = unsafe {
                engine_api::AppApi::new(_lib)
            };
            app_state = (api.init)();
            (api.reload)(app_state);
        }
        if (api.terminate)(app_state) {
            unsafe {
                terminate = true;
                (api.shutdown)(app_state);
            }
        }
    }
}
