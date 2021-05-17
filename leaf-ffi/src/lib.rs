use std::{ffi::CStr, os::raw::c_char};

use leaf::{Config, RuntimeOption, StartOptions};

/// No error.
pub const ERR_OK: i32 = 0;
/// Config path error.
pub const ERR_CONFIG_PATH: i32 = 1;
/// Config parsing error.
pub const ERR_CONFIG: i32 = 2;
/// IO error.
pub const ERR_IO: i32 = 3;
/// Config file watcher error.
pub const ERR_WATCHER: i32 = 4;
/// Async channel send error.
pub const ERR_ASYNC_CHANNEL_SEND: i32 = 5;
/// Sync channel receive error.
pub const ERR_SYNC_CHANNEL_RECV: i32 = 6;
/// Runtime manager error.
pub const ERR_RUNTIME_MANAGER: i32 = 7;
/// No associated config file.
pub const ERR_NO_CONFIG_FILE: i32 = 8;

fn to_errno(e: leaf::Error) -> i32 {
    match e {
        leaf::Error::Config(..) => ERR_CONFIG,
        leaf::Error::NoConfigFile => ERR_NO_CONFIG_FILE,
        leaf::Error::Io(..) => ERR_IO,
        #[cfg(feature = "auto-reload")]
        leaf::Error::Watcher(..) => ERR_WATCHER,
        leaf::Error::AsyncChannelSend(..) => ERR_ASYNC_CHANNEL_SEND,
        leaf::Error::SyncChannelRecv(..) => ERR_SYNC_CHANNEL_RECV,
        leaf::Error::RuntimeManager => ERR_RUNTIME_MANAGER,
    }
}

/// Starts leaf with options, on a successful start this function blocks the current
/// thread.
///
/// @note This is not a stable API, parameters will change from time to time.
///
/// @param rt_id A unique ID to associate this leaf instance, this is required when
///              calling subsequent FFI functions, e.g. reload, shutdown.
/// @param config_path The path of the config file, must be a file with suffix .conf
///                    or .json, according to the enabled features.
/// @param auto_reload Enabls auto reloading when config file changes are detected,
///                    takes effect only when the "auto-reload" feature is enabled.
/// @param multi_thread Whether to use a multi-threaded runtime.
/// @param auto_threads Sets the number of runtime worker threads automatically,
///                     takes effect only when multi_thread is true.
/// @param threads Sets the number of runtime worker threads, takes effect when
///                     multi_thread is true, but can be overridden by auto_threads.
/// @param stack_size Sets stack size of the runtime worker threads, takes effect when
///                   multi_thread is true.
/// @return ERR_OK on finish running, any other errors means a startup failure.
#[no_mangle]
pub extern "C" fn leaf_run_with_options(
    rt_id: u16,
    config_path: *const c_char,
    auto_reload: bool, // requires this parameter anyway
    multi_thread: bool,
    auto_threads: bool,
    threads: i32,
    stack_size: i32,
) -> i32 {
    if let Ok(config_path) = unsafe { CStr::from_ptr(config_path).to_str() } {
        if let Err(e) = leaf::util::run_with_options(
            rt_id,
            config_path.to_string(),
            #[cfg(feature = "auto-reload")]
            auto_reload,
            multi_thread,
            auto_threads,
            threads as usize,
            stack_size as usize,
        ) {
            return to_errno(e);
        }
        ERR_OK
    } else {
        ERR_CONFIG_PATH
    }
}

/// Starts leaf with a single-threaded runtime, on a successful start this function
/// blocks the current thread.
///
/// @param rt_id A unique ID to associate this leaf instance, this is required when
///              calling subsequent FFI functions, e.g. reload, shutdown.
/// @param config_path The path of the config file, must be a file with suffix .conf
///                    or .json, according to the enabled features.
/// @return ERR_OK on finish running, any other errors means a startup failure.
#[no_mangle]
pub extern "C" fn leaf_run(rt_id: u16, config_path: *const c_char) -> i32 {
    if let Ok(config_path) = unsafe { CStr::from_ptr(config_path).to_str() } {
        let opts = leaf::StartOptions {
            config: leaf::Config::File(config_path.to_string()),
            #[cfg(feature = "auto-reload")]
            auto_reload: false,
            runtime_opt: leaf::RuntimeOption::SingleThread,
        };
        if let Err(e) = leaf::start(rt_id, opts, Box::new(|_| {})) {
            return to_errno(e);
        }
        ERR_OK
    } else {
        ERR_CONFIG_PATH
    }
}

/// Reloads DNS servers, outbounds and routing rules from the config file.
///
/// @param rt_id The ID of the leaf instance to reload.
///
/// @return Returns ERR_OK on success.
#[no_mangle]
pub extern "C" fn leaf_reload(rt_id: u16) -> i32 {
    if let Err(e) = leaf::reload(rt_id) {
        return to_errno(e);
    }
    ERR_OK
}

/// Shuts down leaf.
///
/// @param rt_id The ID of the leaf instance to reload.
///
/// @return Returns true on success, false otherwise.
#[no_mangle]
pub extern "C" fn leaf_shutdown(rt_id: u16) -> bool {
    leaf::shutdown(rt_id)
}

/// Tests the configuration.
///
/// @param config_path The path of the config file, must be a file with suffix .conf
///                    or .json, according to the enabled features.
/// @return Returns ERR_OK on success, i.e no syntax error.
#[no_mangle]
pub extern "C" fn leaf_test_config(config_path: *const c_char) -> i32 {
    if let Ok(config_path) = unsafe { CStr::from_ptr(config_path).to_str() } {
        if let Err(e) = leaf::test_config(&config_path) {
            return to_errno(e);
        }
        ERR_OK
    } else {
        ERR_CONFIG_PATH
    }
}

#[cfg(target_vendor = "uwp")]
#[no_mangle]
pub extern "C" fn run_leaf(
    config_path: *const c_char,
    bind_host: *const c_char,
    on_dns: Option<extern "system" fn(dns: *const c_char)>,
) -> *mut tokio::runtime::Runtime {
    use std::ptr::null_mut;
    let config_path = match unsafe { CStr::from_ptr(config_path).to_str() } {
        Ok(s) => s.to_string(),
        Err(_) => {
            println!("invalid config path");
            return null_mut();
        }
    };
    let config = match leaf::config::from_file(&config_path) {
        Ok(config) => config,
        Err(_) => {
            println!("invalid config file");
            return null_mut();
        }
    };
    if let Some(on_dns) = on_dns {
        for dns in config.dns.iter() {
            dns.servers
                .iter()
                .map(|s| std::ffi::CString::new(&**s).unwrap())
                .for_each(|cs| on_dns(cs.as_ptr()));
        }
    }
    let config_transformer = if bind_host.is_null() {
        Box::new(|_: &mut leaf::config::Config| {})
            as Box<dyn Fn(&mut leaf::config::Config) + Send + Sync>
    } else {
        let bind_host = unsafe { CStr::from_ptr(bind_host).to_str().unwrap().to_string() };
        Box::new(move |config: &mut leaf::config::Config| {
            let leaf::config::Config { dns, outbounds, .. } = config;
            dns.mut_iter().for_each(|d| d.bind = bind_host.clone());
            outbounds
                .iter_mut()
                .for_each(|o| o.bind = bind_host.clone());
        }) as Box<dyn Fn(&mut leaf::config::Config) + Send + Sync>
    };
    match leaf::start(
        0,
        StartOptions {
            config: Config::File(config_path),
            auto_reload: true,
            runtime_opt: RuntimeOption::MultiThreadAuto(2 * 1024 * 1024),
        },
        config_transformer,
    ) {
        Ok(rt) => Box::into_raw(Box::new(rt)),
        Err(_) => null_mut(),
    }
}

#[cfg(target_vendor = "uwp")]
#[no_mangle]
pub extern "C" fn stop_leaf(runtime: *mut tokio::runtime::Runtime) {
    if runtime.is_null() {
        return;
    }
    leaf::RUNTIME_MANAGER.lock().unwrap().clear();
    let rt = unsafe { Box::from_raw(runtime) };
    rt.shutdown_background();
}
