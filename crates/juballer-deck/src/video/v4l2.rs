//! v4l2 capture backend. Opens a device by path, negotiates MJPEG (preferred) or
//! YUYV, and spawns a blocking capture thread that pushes decoded RGBA frames
//! into a bounded channel.

use super::{VideoBackend, VideoFrame};
use std::sync::mpsc::{sync_channel, Receiver, TrySendError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

const DEFAULT_W: u32 = 640;
const DEFAULT_H: u32 = 480;

pub struct V4l2Backend {
    rx: Receiver<VideoFrame>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl V4l2Backend {
    pub fn open(path: &str) -> std::io::Result<Self> {
        use v4l::io::traits::CaptureStream;
        use v4l::prelude::*;
        use v4l::video::Capture;
        use v4l::{Format, FourCC};

        let dev = Device::with_path(path)?;
        let mut use_mjpg = true;
        let mut fmt = Format::new(DEFAULT_W, DEFAULT_H, FourCC::new(b"MJPG"));
        let active = match dev.set_format(&fmt) {
            Ok(f) if f.fourcc == FourCC::new(b"MJPG") => f,
            _ => {
                use_mjpg = false;
                fmt = Format::new(DEFAULT_W, DEFAULT_H, FourCC::new(b"YUYV"));
                dev.set_format(&fmt)?
            }
        };

        let w = active.width;
        let h = active.height;
        let fourcc = active.fourcc;
        let pixfmt = if fourcc == FourCC::new(b"MJPG") {
            PixFmt::Mjpeg
        } else if fourcc == FourCC::new(b"YUYV") {
            PixFmt::Yuyv
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("unsupported fourcc {fourcc}"),
            ));
        };

        let (tx, rx) = sync_channel::<VideoFrame>(2);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let path_owned = path.to_string();
        let handle = std::thread::Builder::new()
            .name(format!("v4l2:{path_owned}"))
            .spawn(move || {
                let _ = use_mjpg;
                let dev = match Device::with_path(&path_owned) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!(target: "juballer::v4l2", "reopen {path_owned}: {e}");
                        return;
                    }
                };
                if let Err(e) = dev.set_format(&Format::new(w, h, fourcc)) {
                    tracing::warn!(target: "juballer::v4l2", "set_format {path_owned}: {e}");
                    return;
                }
                let mut stream =
                    match MmapStream::with_buffers(&dev, v4l::buffer::Type::VideoCapture, 4) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(target: "juballer::v4l2", "stream {path_owned}: {e}");
                            return;
                        }
                    };
                while !stop_thread.load(Ordering::Relaxed) {
                    let (buf, _meta) = match stream.next() {
                        Ok(f) => f,
                        Err(e) => {
                            tracing::warn!(target: "juballer::v4l2", "capture {path_owned}: {e}");
                            break;
                        }
                    };
                    let decoded = match pixfmt {
                        PixFmt::Mjpeg => decode_mjpeg(buf, w, h),
                        PixFmt::Yuyv => Some(super::yuyv_to_rgba(buf, w, h)),
                    };
                    let Some(rgba) = decoded else {
                        continue;
                    };
                    let frame = VideoFrame {
                        width: w,
                        height: h,
                        data: rgba,
                        captured_at: std::time::Instant::now(),
                    };
                    match tx.try_send(frame) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            // Consumer behind; drop.
                        }
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
            })?;

        Ok(Self {
            rx,
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for V4l2Backend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl VideoBackend for V4l2Backend {
    fn try_recv_frame(&mut self) -> Option<VideoFrame> {
        self.rx.try_recv().ok()
    }
}

#[derive(Copy, Clone)]
enum PixFmt {
    Mjpeg,
    Yuyv,
}

fn decode_mjpeg(buf: &[u8], expected_w: u32, expected_h: u32) -> Option<Vec<u8>> {
    let mut decoder = jpeg_decoder::Decoder::new(buf);
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    if info.width as u32 != expected_w || info.height as u32 != expected_h {
        // Size drift; we still promote to RGBA but keep the first frame's expected dims.
    }
    let w = info.width as u32;
    let h = info.height as u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            for (i, px) in pixels.chunks(3).enumerate() {
                rgba[i * 4] = px[0];
                rgba[i * 4 + 1] = px[1];
                rgba[i * 4 + 2] = px[2];
                rgba[i * 4 + 3] = 0xff;
            }
        }
        jpeg_decoder::PixelFormat::L8 => {
            for (i, p) in pixels.iter().enumerate() {
                rgba[i * 4] = *p;
                rgba[i * 4 + 1] = *p;
                rgba[i * 4 + 2] = *p;
                rgba[i * 4 + 3] = 0xff;
            }
        }
        _ => return None,
    }
    // If dimensions differ from expected, truncate / drop.
    if w != expected_w || h != expected_h {
        return None;
    }
    Some(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a v4l2 device at /dev/video0"]
    fn opens_video0_if_present() {
        let backend = V4l2Backend::open("/dev/video0");
        assert!(backend.is_ok(), "open /dev/video0: {:?}", backend.err());
        let mut b = backend.unwrap();
        // Give the capture thread a moment to warm up.
        std::thread::sleep(std::time::Duration::from_millis(500));
        let f = b.try_recv_frame();
        assert!(f.is_some(), "expected at least one frame within 500ms");
    }
}
