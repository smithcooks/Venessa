use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenConfig,
    Quit,
}

static TRAY_SENDER: OnceLock<crossbeam_channel::Sender<TrayCommand>> = OnceLock::new();

#[cfg(windows)]
pub struct TrayIcon {
    is_running: Arc<AtomicBool>,
}

#[cfg(windows)]
impl TrayIcon {
    pub fn new(cmd_sender: crossbeam_channel::Sender<TrayCommand>) -> Self {
        let _ = TRAY_SENDER.set(cmd_sender);
        let is_running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&is_running);

        std::thread::spawn(move || {
            use std::ptr::null_mut;
            use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
            use windows_sys::Win32::UI::Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
                DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW,
                PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu,
                TranslateMessage, HICON, HMENU, IDI_APPLICATION, MF_DISABLED, MF_GRAYED,
                MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
                WM_COMMAND, WM_DESTROY, WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_OVERLAPPED,
            };

            const WM_TRAYICON: u32 = WM_USER + 100;
            const ID_TRAY_TITLE: usize = 1001;
            const ID_TRAY_CONFIG: usize = 1002;
            const ID_TRAY_EXIT: usize = 1003;

            unsafe extern "system" fn window_proc(
                hwnd: HWND,
                msg: u32,
                w_param: WPARAM,
                l_param: LPARAM,
            ) -> LRESULT {
                match msg {
                    WM_TRAYICON => {
                        let event = (l_param & 0xFFFF) as u32;
                        if event == WM_RBUTTONUP {
                            let mut cursor = POINT { x: 0, y: 0 };
                            GetCursorPos(&mut cursor);

                            let menu: HMENU = CreatePopupMenu();

                            // Title header
                            let title: Vec<u16> = "Venessa Voice Dictation\0".encode_utf16().collect();
                            AppendMenuW(menu, MF_STRING | MF_DISABLED | MF_GRAYED, ID_TRAY_TITLE, title.as_ptr());

                            AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());

                            // Open config
                            let cfg_label: Vec<u16> = "Edit Config (config.toml)\0".encode_utf16().collect();
                            AppendMenuW(menu, MF_STRING, ID_TRAY_CONFIG, cfg_label.as_ptr());

                            AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());

                            // Exit
                            let exit_label: Vec<u16> = "Exit Venessa\0".encode_utf16().collect();
                            AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, exit_label.as_ptr());

                            SetForegroundWindow(hwnd);
                            TrackPopupMenu(
                                menu,
                                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
                                cursor.x,
                                cursor.y,
                                0,
                                hwnd,
                                null_mut(),
                            );
                            DestroyMenu(menu);
                        }
                        0
                    }
                    WM_COMMAND => {
                        let id = (w_param & 0xFFFF) as usize;
                        if id == ID_TRAY_CONFIG {
                            if let Some(s) = TRAY_SENDER.get() {
                                let _ = s.send(TrayCommand::OpenConfig);
                            }
                        } else if id == ID_TRAY_EXIT {
                            if let Some(s) = TRAY_SENDER.get() {
                                let _ = s.send(TrayCommand::Quit);
                            }
                            PostQuitMessage(0);
                        }
                        0
                    }
                    WM_DESTROY => {
                        PostQuitMessage(0);
                        0
                    }
                    _ => DefWindowProcW(hwnd, msg, w_param, l_param),
                }
            }

            unsafe {
                let class_name: Vec<u16> = "VenessaTrayClass\0".encode_utf16().collect();
                let window_title: Vec<u16> = "VenessaTray\0".encode_utf16().collect();

                let wnd_class = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(window_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: 0 as HINSTANCE,
                    hIcon: 0 as HICON,
                    hCursor: 0 as _,
                    hbrBackground: 0 as _,
                    lpszMenuName: null_mut(),
                    lpszClassName: class_name.as_ptr(),
                };

                RegisterClassW(&wnd_class);

                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    window_title.as_ptr(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    0 as HINSTANCE,
                    null_mut(),
                );

                let icon: HICON = LoadIconW(0 as HINSTANCE, IDI_APPLICATION);

                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = 1;
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                nid.uCallbackMessage = WM_TRAYICON;
                nid.hIcon = icon;

                let tip_text = "Venessa: Voice Dictation (Hold Ctrl+Space to speak)";
                let tip_utf16: Vec<u16> = tip_text.encode_utf16().take(127).chain(std::iter::once(0)).collect();
                for (i, &c) in tip_utf16.iter().enumerate() {
                    if i < nid.szTip.len() {
                        nid.szTip[i] = c;
                    }
                }

                Shell_NotifyIconW(NIM_ADD, &nid);

                let mut msg = std::mem::zeroed();
                while running_clone.load(Ordering::SeqCst) && GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                Shell_NotifyIconW(NIM_DELETE, &nid);
                DestroyWindow(hwnd);
            }
        });

        Self { is_running }
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}
