use std::{ffi::CStr, os::raw::c_char};

// TODO Return meaningful error codes.
#[cfg(not(target_vendor = "uwp"))]
#[no_mangle]
pub extern "C" fn run_leaf(config_path: *const c_char) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    if let Ok(config) = unsafe { CStr::from_ptr(config_path).to_str() }
        .map_err(Into::into)
        .and_then(leaf::config::from_file)
    {
        let runners = match leaf::util::prepare(config) {
            Ok(v) => v,
            Err(e) => {
                println!("prepare failed: {}", e);
                return;
            }
        };
        rt.block_on(futures::future::join_all(runners));
    } else {
        println!("invalid config path or config file");
        return;
    }
}

#[cfg(target_vendor = "uwp")]
#[no_mangle]
pub extern "system" fn run_leaf(
    config_path: *const c_char,
    bind_host: *const c_char,
    on_dns: Option<extern "system" fn(dns: *const c_char)>,
) -> *mut tokio::runtime::Runtime {
    use std::ptr::null_mut;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut config = match unsafe { CStr::from_ptr(config_path).to_str() }
        .map_err(Into::into)
        .and_then(leaf::config::from_file)
    {
        Ok(config) => config,
        Err(_) => {
            println!("invalid config path or config file");
            return null_mut();
        }
    };
    if !bind_host.is_null() {
        let bind_host = unsafe { CStr::from_ptr(bind_host).to_str().unwrap().to_string() };
        for dns in config.dns.mut_iter() {
            if let Some(on_dns) = on_dns {
                dns.servers
                    .iter()
                    .map(|s| std::ffi::CString::new(&**s).unwrap())
                    .for_each(|cs| on_dns(cs.as_ptr()));
            }
            dns.bind = bind_host.clone();
        }
        for outbound in config.outbounds.iter_mut() {
            outbound.bind = bind_host.clone();
        }
    }
    for runner in match leaf::util::prepare(config) {
        Ok(v) => v,
        Err(e) => {
            println!("prepare failed: {}", e);
            return null_mut();
        }
    } {
        rt.spawn(runner);
    }

    Box::into_raw(Box::new(rt))
}

#[cfg(target_vendor = "uwp")]
#[no_mangle]
pub extern "system" fn stop_leaf(runtime: *mut tokio::runtime::Runtime) {
    if runtime.is_null() {
        return;
    }
    let rt = unsafe { Box::from_raw(runtime) };
    rt.shutdown_background();
}
