mod network_listener;

#[cfg(all(
    feature = "inbound-tun",
    any(
        target_os = "ios",
        target_os = "android",
        target_os = "macos",
        target_os = "linux",
        target_vendor = "uwp"
    )
))]
mod tun_listener;

pub mod manager;
