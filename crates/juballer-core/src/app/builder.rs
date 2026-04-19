use crate::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentMode {
    /// Vsync on. Most compatible; no tearing. Default.
    Fifo,
    /// Low latency + tear-free, but wgpu 23 has semaphore-reuse validation warnings
    /// on Vulkan. Opt in explicitly if you know the tradeoff. Fixed in wgpu 24+.
    Mailbox,
    /// No vsync; lowest latency; may tear. Best for VRR displays + rhythm games.
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTarget {
    Monitor,
    Fixed(u32),
    Unlimited,
}

#[derive(Debug, Clone)]
pub struct AppBuilder {
    pub(crate) title: String,
    pub(crate) present_mode: PresentMode,
    pub(crate) swapchain_buffers: u8,
    pub(crate) target_refresh: RefreshTarget,
    pub(crate) bg_color: Color,
    pub(crate) controller_vid: u16,
    pub(crate) controller_pid: u16,
    pub(crate) monitor_desc: Option<String>,
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self {
            title: "juballer".into(),
            present_mode: PresentMode::Fifo,
            swapchain_buffers: 2,
            target_refresh: RefreshTarget::Monitor,
            bg_color: Color::BLACK,
            controller_vid: 0,
            controller_pid: 0,
            monitor_desc: None,
        }
    }
}

impl AppBuilder {
    pub fn title(mut self, s: impl Into<String>) -> Self {
        self.title = s.into();
        self
    }
    pub fn present_mode(mut self, m: PresentMode) -> Self {
        self.present_mode = m;
        self
    }
    pub fn swapchain_buffers(mut self, n: u8) -> Self {
        assert!(n == 2 || n == 3, "swapchain_buffers must be 2 or 3");
        self.swapchain_buffers = n;
        self
    }
    pub fn target_refresh(mut self, r: RefreshTarget) -> Self {
        self.target_refresh = r;
        self
    }
    pub fn bg_color(mut self, c: Color) -> Self {
        self.bg_color = c;
        self
    }
    pub fn controller_vid_pid(mut self, vid: u16, pid: u16) -> Self {
        self.controller_vid = vid;
        self.controller_pid = pid;
        self
    }
    /// Open fullscreen on the monitor whose `MonitorHandle::name()` contains this substring.
    /// Case-insensitive. If no match, falls back to primary monitor.
    /// On Wayland/Hyprland the name typically includes the make + model + serial.
    pub fn on_monitor(mut self, desc_contains: impl Into<String>) -> Self {
        self.monitor_desc = Some(desc_contains.into());
        self
    }
}
