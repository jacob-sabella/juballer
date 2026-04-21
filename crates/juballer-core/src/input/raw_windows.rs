//! Windows raw-input via RegisterRawInputDevices + WM_INPUT. Spawns a hidden message-only
//! window on a dedicated thread, decodes RAWINPUT structs, pushes Events into the EventRing.
#![cfg(all(target_os = "windows", feature = "raw-input"))]

use super::{Event, EventRing, KeyCode, Keymap};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

pub struct RawInputWindows {
    pub join: thread::JoinHandle<()>,
}

impl RawInputWindows {
    pub fn spawn(
        _vid: u16,
        _pid: u16,
        keymap: Keymap,
        ring: Arc<EventRing>,
    ) -> std::io::Result<Self> {
        let join = thread::Builder::new()
            .name("juballer-raw-input-win".into())
            .spawn(move || {
                run_loop(keymap, ring);
            })?;
        Ok(Self { join })
    }
}

fn run_loop(keymap: Keymap, ring: Arc<EventRing>) {
    // SAFETY: All Win32 calls are FFI. Window creation, class registration, and message pump
    // run on this dedicated thread only. STATE is thread-local.
    unsafe {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::Input::*;
        use windows::Win32::UI::WindowsAndMessaging::*;

        STATE.with(|s| {
            *s.borrow_mut() = Some(State { keymap, ring });
        });

        let h_instance = HINSTANCE::default();
        let class_name = windows::core::w!("juballer-raw-input");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: h_instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            windows::core::w!("juballer"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(h_instance),
            None,
        )
        .unwrap_or_default();

        let rid = RAWINPUTDEVICE {
            usUsagePage: 0x01, // Generic Desktop
            usUsage: 0x06,     // Keyboard
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        let _ = RegisterRawInputDevices(&[rid], std::mem::size_of::<RAWINPUTDEVICE>() as u32);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct State {
    keymap: Keymap,
    ring: Arc<EventRing>,
}

thread_local! {
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
}

// SAFETY: Win32 WNDPROC callback. Called by DispatchMessageW on the same thread that
// created the window. All pointers come from the OS and are valid for the duration of the call.
unsafe extern "system" fn wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    w: windows::Win32::Foundation::WPARAM,
    l: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Input::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    if msg == WM_INPUT {
        let mut size: u32 = 0;
        let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        // SAFETY: OS-provided lparam is a valid HRAWINPUT handle; size/header_size are valid.
        // This call queries the buffer size needed.
        unsafe {
            let _ = GetRawInputData(HRAWINPUT(l.0 as _), RID_INPUT, None, &mut size, header_size);
        }
        let mut buf = vec![0u8; size as usize];
        // SAFETY: buf is sized per previous call; pointer valid for size bytes.
        // This call populates buf with the RAWINPUT struct.
        unsafe {
            let _ = GetRawInputData(
                HRAWINPUT(l.0 as _),
                RID_INPUT,
                Some(buf.as_mut_ptr() as _),
                &mut size,
                header_size,
            );
        }
        // SAFETY: buf contains a valid RAWINPUT struct populated by the OS above.
        let raw = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
        if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
            // SAFETY: Union field access is safe because we've verified dwType == RIM_TYPEKEYBOARD.
            let kb = unsafe { raw.data.keyboard };
            let vk = kb.VKey;
            let pressed = (kb.Flags as u32 & RI_KEY_BREAK) == 0;
            let code_str = format!("VK_{}", vk);
            let ts = Instant::now();
            STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let event = if pressed {
                        match state.keymap.lookup(&code_str) {
                            Some((row, col)) => Event::KeyDown {
                                row,
                                col,
                                key: KeyCode(code_str),
                                ts,
                            },
                            None => Event::Unmapped {
                                key: KeyCode(code_str),
                                ts,
                            },
                        }
                    } else {
                        match state.keymap.lookup(&code_str) {
                            Some((row, col)) => Event::KeyUp {
                                row,
                                col,
                                key: KeyCode(code_str),
                                ts,
                            },
                            None => return,
                        }
                    };
                    state.ring.try_send(event);
                }
            });
        }
    }
    // SAFETY: Default handler for unhandled messages. hwnd/msg/w/l are OS-provided and valid.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
