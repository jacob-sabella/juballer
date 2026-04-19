use crate::Result;
use std::sync::Arc;

/// Open a borderless fullscreen window. If `desc_contains` is provided, target the first
/// monitor whose name contains that substring (case-insensitive). Otherwise use primary.
pub fn open_fullscreen(
    event_loop: &winit::event_loop::ActiveEventLoop,
    title: &str,
    desc_contains: Option<&str>,
) -> Result<Arc<winit::window::Window>> {
    let monitor = if let Some(needle) = desc_contains {
        let lower = needle.to_lowercase();
        event_loop
            .available_monitors()
            .find(|m| {
                m.name()
                    .map(|n| n.to_lowercase().contains(&lower))
                    .unwrap_or(false)
            })
            .or_else(|| event_loop.primary_monitor())
            .or_else(|| event_loop.available_monitors().next())
    } else {
        event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next())
    }
    .ok_or_else(|| crate::Error::MonitorNotFound("no monitors available".into()))?;

    log::info!("opening fullscreen on monitor: {:?}", monitor.name());

    let attrs = winit::window::WindowAttributes::default()
        .with_title(title)
        .with_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(monitor))));
    let window = event_loop.create_window(attrs)?;
    Ok(Arc::new(window))
}
