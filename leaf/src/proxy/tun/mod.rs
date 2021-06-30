#[cfg(all(
    feature = "inbound-tun",
    any(target_os = "ios", target_os = "macos", target_os = "linux")
))]
#[path = "inbound_tun.rs"]
pub mod inbound;
#[cfg(all(feature = "inbound-tun", target_os = "windows"))]
#[path = "inbound_winrt.rs"]
pub mod inbound;

pub mod netstack;
