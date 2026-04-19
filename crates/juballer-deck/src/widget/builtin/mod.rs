//! Built-in widgets.

pub mod action_mini;
pub mod clock;
pub mod counter_widget;
pub mod dynamic;
pub mod homelab_status;
pub mod http_probe;
pub mod image_widget;
pub mod log_feed;
pub mod notification_toast;
pub mod now_playing;
pub mod plugin_proxy;
pub mod register;
pub mod sysinfo_widget;
pub mod text;

pub use register::register_builtins;
