use std::{
    ffi::c_void,
    ptr::null,
    sync::{
        atomic::{AtomicPtr, Ordering},
        Arc, Mutex,
    },
};

use anyhow::{anyhow, Result};
use log::*;
use protobuf::Message;
use std::sync::Once;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex as TokioMutex;

use crate::{
    app::dispatcher::Dispatcher,
    app::fake_dns::{FakeDns, FakeDnsMode},
    app::nat_manager::NatManager,
    config::{Inbound, TunInboundSettings},
    Runner,
};

use super::netstack::NetStack;

const MTU: usize = 1500;

enum ReceiverInfo {
    Registered {
        on_receive: extern "C" fn(*const u8, usize, *const c_void),
        context: AtomicPtr<c_void>,
        tun_rx: UnboundedReceiver<Vec<u8>>,
    },
    ReceiverTaken {
        on_receive: extern "C" fn(*const u8, usize, *const c_void),
        context: AtomicPtr<c_void>,
    },
    Stopped,
}

impl ReceiverInfo {
    fn take_tun_rx(&mut self) -> Option<UnboundedReceiver<Vec<u8>>> {
        let new_receiver_info = if let ReceiverInfo::Registered {
            on_receive,
            context,
            ..
        } = self
        {
            ReceiverInfo::ReceiverTaken {
                on_receive: *on_receive,
                context: AtomicPtr::new(context.load(Ordering::SeqCst)),
            }
        } else {
            return None;
        };
        if let ReceiverInfo::Registered { tun_rx, .. } = std::mem::replace(self, new_receiver_info)
        {
            Some(tun_rx)
        } else {
            unreachable!()
        }
    }
}

static mut RECEIVER_INFO: Option<Mutex<ReceiverInfo>> = None;
static RECEIVER_INFO_ONCE: Once = Once::new();
fn get_receiver_info() -> &'static Mutex<ReceiverInfo> {
    RECEIVER_INFO_ONCE
        .call_once(|| unsafe { RECEIVER_INFO = Some(Mutex::new(ReceiverInfo::Stopped)) });
    unsafe { RECEIVER_INFO.as_ref().unwrap() }
}

#[no_mangle]
pub extern "C" fn netstack_register(
    on_receive: extern "C" fn(*const u8, usize, *const c_void),
    context: *const c_void,
) -> *mut UnboundedSender<Vec<u8>> {
    let mut receiver_info = get_receiver_info().lock().unwrap();
    let (tx, rx) = unbounded_channel();
    *receiver_info = ReceiverInfo::Registered {
        on_receive,
        context: AtomicPtr::new(context as *mut _),
        tun_rx: rx,
    };
    Box::into_raw(Box::new(tx))
}

#[no_mangle]
pub extern "C" fn netstack_send(
    handle: *const UnboundedSender<Vec<u8>>,
    data: *mut u8,
    size: usize,
) -> i32 {
    // We cannot guarantee our sender is unique in caller's environment.
    // Simply casting the pointer to a mutable reference may cause UB.
    let handle = match unsafe { handle.as_ref() } {
        Some(h) => h,
        None => return -1,
    };
    let data = unsafe { std::slice::from_raw_parts(data, size) };
    // Unbounded channel only requires a shared reference to send data,
    // while bounded channel needs a exclusive reference.
    // Therefore, we cannot use a bounded channel here.
    match handle.send(data.to_vec()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

#[no_mangle]
pub extern "C" fn netstack_release(handle: *mut UnboundedSender<Vec<u8>>) -> *const c_void {
    unsafe { Box::from_raw(handle) };
    let mut receiver_info = get_receiver_info().lock().unwrap();
    if let ReceiverInfo::Registered { context, .. } | ReceiverInfo::ReceiverTaken { context, .. } =
        std::mem::replace(&mut *receiver_info, ReceiverInfo::Stopped)
    {
        context.load(Ordering::SeqCst)
    } else {
        null()
    }
}

pub fn new(
    inbound: Inbound,
    dispatcher: Arc<Dispatcher>,
    nat_manager: Arc<NatManager>,
) -> Result<Runner> {
    let (on_receive, context) = {
        let receiver_info = get_receiver_info().lock().unwrap();
        match &*receiver_info {
            ReceiverInfo::Registered {
                on_receive,
                context,
                ..
            }
            | ReceiverInfo::ReceiverTaken {
                on_receive,
                context,
            } => Ok((*on_receive, AtomicPtr::new(context.load(Ordering::SeqCst)))),
            _ => Err(anyhow!(
                "Must call netstack_register before initializing netstack"
            )),
        }
    }?;
    let settings = TunInboundSettings::parse_from_bytes(&inbound.settings).unwrap();

    // FIXME it's a bad design to have 2 lists in config while we need only one
    let (fake_dns_mode, fake_dns_filters) = match (
        settings.fake_dns_exclude.len(),
        settings.fake_dns_include.len(),
    ) {
        (_, 0) => (FakeDnsMode::Exclude, settings.fake_dns_exclude),
        (0, _) => (FakeDnsMode::Include, settings.fake_dns_include),
        _ => Err(anyhow!(
            "fake DNS run in either include mode or exclude mode"
        ))?,
    };

    Ok(Box::pin(async move {
        let fakedns = Arc::new(TokioMutex::new(FakeDns::new(fake_dns_mode)));
        {
            let mut fakedns = fakedns.lock().await;

            for filter in fake_dns_filters.into_iter() {
                fakedns.add_filter(filter);
            }
        }

        let stack = NetStack::new(inbound.tag.clone(), dispatcher, nat_manager, fakedns);

        let mtu = MTU as i32;
        let (mut stack_reader, mut stack_writer) = io::split(stack);

        let s2t = async move {
            let mut buf = vec![0; mtu as usize];

            loop {
                match stack_reader.read(&mut buf).await {
                    Ok(0) => {
                        debug!("read stack eof");
                        return;
                    }
                    Ok(n) => {
                        on_receive(buf.as_ptr(), n, context.load(Ordering::Relaxed));
                    }
                    Err(err) => {
                        warn!("read stack failed {:?}", err);
                        return;
                    }
                }
            }
        };

        let t2s = async move {
            let mut tun_rx = {
                let mut receiver_info = get_receiver_info().lock().unwrap();
                receiver_info.take_tun_rx().unwrap()
            };
            while let Some(packet) = tun_rx.recv().await {
                match stack_writer.write(&packet).await {
                    Ok(_) => (),
                    Err(e) => {
                        warn!("write pkt to stack failed: {}", e);
                        return;
                    }
                }
            }
        };

        info!("tun inbound started");

        tokio::select! {
            r1 = t2s => debug!("s2t ended {:?}", r1),
            r2 = s2t => debug!("s2t ended {:?}", r2)
        }
    }))
}
