use libloading as ll;

#[repr(C)]
pub struct App_State {
    _reload: [u8; 0],
    _terminate: [u8; 0]
}

pub struct AppApi<'lib> {
    /// On start.
    pub init: ll::Symbol<'lib, fn() -> *mut App_State>,

    /// Called each loop, check if the app should reload.
    pub update: ll::Symbol<'lib, fn(*mut App_State) -> bool>,

    /// Called each loop, check if the app should terminate.
    pub terminate: ll::Symbol<'lib, fn(*mut App_State) -> bool>,
    
    /// Called upon exit.
    pub shutdown: ll::Symbol<'lib, fn(*mut App_State)>,

    /// Called upun unload.
    pub unload: ll::Symbol<'lib, fn(*mut App_State)>,

    /// Called upon reload.
    pub reload: ll::Symbol<'lib, fn(*mut App_State)>,
}

impl<'lib> AppApi<'lib> {
    pub unsafe fn new(lib: &'lib ll::Library) -> Self {
        let func_init: ll::Symbol<'lib, fn() -> *mut App_State> = lib.get(b"init").unwrap();

        let func_update: ll::Symbol<'lib, fn(*mut App_State) -> bool> = lib.get(b"update").unwrap();

        let func_terminate: ll::Symbol<'lib, fn(*mut App_State) -> bool> = lib.get(b"terminate").unwrap();

        let func_shutdown: ll::Symbol<'lib, fn(*mut App_State)> = lib.get(b"app_shutdown").unwrap();

        let func_unload: ll::Symbol<'lib, fn(*mut App_State)> = lib.get(b"unload").unwrap();

        let func_reload: ll::Symbol<'lib, fn(*mut App_State)> = lib.get(b"reload").unwrap();

        AppApi {
            init: func_init,
            update: func_update,
            terminate: func_terminate,
            shutdown: func_shutdown,
            unload: func_unload,
            reload: func_reload,            
        }
    }
}

