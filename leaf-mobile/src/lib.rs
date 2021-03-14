use std::ffi::CString;
use std::{ffi::CStr, os::raw::c_char, ptr::null_mut, sync::Once};

use bytes::BytesMut;
use futures::future::select_all;
use log::*;
use tokio::runtime::Runtime;

use leaf::config;

#[cfg(target_os = "ios")]
pub mod ios;

mod logger;
use logger::ConsoleWriter;

static INIT_LOG: Once = Once::new();

// this function is available on iOS 13.0+
// use ios::os_proc_available_memory;

#[no_mangle]
pub extern "system" fn run_leaf(
    path: *const c_char,
    bind_host: *const c_char,
    on_dns: Option<extern "system" fn(dns: *const c_char)>,
) -> *mut Runtime {
    if let Ok(mut config) = unsafe { CStr::from_ptr(path).to_str() }
        .map_err(Into::into)
        .and_then(leaf::config::from_file)
    {
        if !bind_host.is_null() {
            let bind_host = unsafe { CStr::from_ptr(bind_host).to_str().unwrap().to_string() };
            for dns in config.dns.mut_iter() {
                if let Some(on_dns) = on_dns {
                    dns.servers
                        .iter()
                        .map(|s| CString::new(&**s).unwrap())
                        .for_each(|cs| on_dns(cs.as_ptr()));
                }
                dns.bind = bind_host.clone();
            }
            for outbound in config.outbounds.iter_mut() {
                outbound.bind = bind_host.clone();
            }
        }

        // fern panics when logging is initialized twice.
        // TODO: re-init logs?
        INIT_LOG.call_once(|| {
            let loglevel = if let Some(log) = config.log.as_ref() {
                match log.level {
                    config::Log_Level::TRACE => log::LevelFilter::Trace,
                    config::Log_Level::DEBUG => log::LevelFilter::Debug,
                    config::Log_Level::INFO => log::LevelFilter::Info,
                    config::Log_Level::WARN => log::LevelFilter::Warn,
                    config::Log_Level::ERROR => log::LevelFilter::Error,
                }
            } else {
                log::LevelFilter::Info
            };
            let mut logger = leaf::common::log::setup_logger(loglevel);
            let console_output =
                fern::Output::writer(Box::new(ConsoleWriter(BytesMut::new())), "\n");
            logger = logger.chain(console_output);
            if let Some(log) = config.log.as_ref() {
                match log.output {
                    config::Log_Output::CONSOLE => {
                        // console output already applied
                    }
                    config::Log_Output::FILE => {
                        let f = fern::log_file(&log.output_file).expect("open log file failed");
                        let file_output = fern::Output::file(f, "\n");
                        logger = logger.chain(file_output);
                    }
                }
            }
            leaf::common::log::apply_logger(logger);
        });

        let mut rt = tokio::runtime::Builder::new();
        #[cfg(target_os = "ios")]
        let rt = rt.basic_scheduler().enable_all().build().unwrap();
        #[cfg(not(target_os = "ios"))]
        let rt = rt.threaded_scheduler().enable_all().build().unwrap();

        let runners = match leaf::util::create_runners(config) {
            Ok(v) => v,
            Err(e) => {
                error!("create runners failed: {}", e);
                return null_mut();
            }
        };

        // let monit_mem = Box::pin(async {
        //     loop {
        //         let n = unsafe { os_proc_available_memory() };
        //         debug!("{} bytes memory available", n);
        //         tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
        //     }
        // });

        rt.spawn(select_all(runners));
        // for fut in runners {
        //     rt.spawn(fut);
        // }
        #[cfg(target_os = "ios")]
        return null_mut();
        #[cfg(not(target_os = "ios"))]
        Box::into_raw(Box::new(rt))
    } else {
        error!("invalid config path or config file");
        null_mut()
    }
}

#[no_mangle]
pub extern "system" fn stop_leaf(runtime: *mut Runtime) {
    if runtime.is_null() {
        return;
    }
    let rt = unsafe { Box::from_raw(runtime) };
    rt.shutdown_background();
}
