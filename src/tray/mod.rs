use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    OpenSettings,
    OpenConfig,
    Quit,
}

#[derive(Debug, Clone)]
pub enum TrayUiAction {
    ShowNotification {
        title: String,
        message: String,
        is_error: bool,
    },
    UpdateTooltip(String),
}

static TRAY_SENDER: OnceLock<crossbeam_channel::Sender<TrayCommand>> = OnceLock::new();
static TRAY_UI_TX: OnceLock<crossbeam_channel::Sender<TrayUiAction>> = OnceLock::new();
static TRAY_HWND: AtomicUsize = AtomicUsize::new(0);

#[cfg(windows)]
pub struct TrayIcon {
    is_running: Arc<AtomicBool>,
}

#[cfg(windows)]
impl TrayIcon {
    pub fn new(cmd_sender: crossbeam_channel::Sender<TrayCommand>, initial_tooltip: &str) -> Self {
        let _ = TRAY_SENDER.set(cmd_sender);
        let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<TrayUiAction>();
        let _ = TRAY_UI_TX.set(ui_tx);

        let is_running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&is_running);
        let init_tip = initial_tooltip.to_string();

        std::thread::spawn(move || {
            use std::ptr::null_mut;
            use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
            use windows_sys::Win32::UI::Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR,
                NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
                DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW,
                PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu,
                TranslateMessage, HICON, HMENU, IDI_APPLICATION, MF_DISABLED, MF_GRAYED,
                MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
                WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK, WM_RBUTTONUP, WM_USER, WNDCLASSW,
                WS_OVERLAPPED,
            };

            const WM_TRAYICON: u32 = WM_USER + 100;
            const ID_TRAY_TITLE: usize = 1001;
            const ID_TRAY_SETTINGS: usize = 1002;
            const ID_TRAY_CONFIG: usize = 1003;
            const ID_TRAY_EXIT: usize = 1004;

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

                            // Settings GUI
                            let settings_label: Vec<u16> = "⚙️ Settings...\0".encode_utf16().collect();
                            AppendMenuW(menu, MF_STRING, ID_TRAY_SETTINGS, settings_label.as_ptr());

                            // Open raw config
                            let cfg_label: Vec<u16> = "Open Config File (config.toml)\0".encode_utf16().collect();
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
                        } else if event == WM_LBUTTONDBLCLK {
                            if let Some(s) = TRAY_SENDER.get() {
                                let _ = s.send(TrayCommand::OpenSettings);
                            }
                        }
                        0
                    }
                    WM_COMMAND => {
                        let id = (w_param & 0xFFFF) as usize;
                        if id == ID_TRAY_SETTINGS {
                            if let Some(s) = TRAY_SENDER.get() {
                                let _ = s.send(TrayCommand::OpenSettings);
                            }
                        } else if id == ID_TRAY_CONFIG {
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

                TRAY_HWND.store(hwnd as usize, Ordering::SeqCst);

                let icon: HICON = LoadIconW(0 as HINSTANCE, IDI_APPLICATION);

                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = 1;
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                nid.uCallbackMessage = WM_TRAYICON;
                nid.hIcon = icon;

                let tip_utf16: Vec<u16> = init_tip.encode_utf16().take(127).chain(std::iter::once(0)).collect();
                for (i, &c) in tip_utf16.iter().enumerate() {
                    if i < nid.szTip.len() {
                        nid.szTip[i] = c;
                    }
                }

                Shell_NotifyIconW(NIM_ADD, &nid);

                // Dedicated thread for handling UI updates (notifications, tooltip changes)
                let hwnd_val = hwnd as usize;
                std::thread::spawn(move || {
                    let hwnd = hwnd_val as HWND;
                    let icon = LoadIconW(0 as HINSTANCE, IDI_APPLICATION);
                    while let Ok(action) = ui_rx.recv() {
                        match action {
                            TrayUiAction::ShowNotification { title, message, is_error } => {
                                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                                nid.hWnd = hwnd;
                                nid.uID = 1;
                                nid.uFlags = NIF_INFO;
                                nid.hIcon = icon;
                                nid.dwInfoFlags = if is_error { NIIF_ERROR } else { NIIF_INFO };

                                let title_u16: Vec<u16> = title.encode_utf16().take(63).chain(std::iter::once(0)).collect();
                                for (i, &c) in title_u16.iter().enumerate() {
                                    if i < nid.szInfoTitle.len() {
                                        nid.szInfoTitle[i] = c;
                                    }
                                }

                                let msg_u16: Vec<u16> = message.encode_utf16().take(255).chain(std::iter::once(0)).collect();
                                for (i, &c) in msg_u16.iter().enumerate() {
                                    if i < nid.szInfo.len() {
                                        nid.szInfo[i] = c;
                                    }
                                }

                                Shell_NotifyIconW(NIM_MODIFY, &nid);
                            }
                            TrayUiAction::UpdateTooltip(text) => {
                                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                                nid.hWnd = hwnd;
                                nid.uID = 1;
                                nid.uFlags = NIF_TIP;

                                let tip_u16: Vec<u16> = text.encode_utf16().take(127).chain(std::iter::once(0)).collect();
                                for (i, &c) in tip_u16.iter().enumerate() {
                                    if i < nid.szTip.len() {
                                        nid.szTip[i] = c;
                                    }
                                }

                                Shell_NotifyIconW(NIM_MODIFY, &nid);
                            }
                        }
                    }
                });

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

    pub fn notify(title: &str, message: &str, is_error: bool) {
        if let Some(tx) = TRAY_UI_TX.get() {
            let _ = tx.send(TrayUiAction::ShowNotification {
                title: title.to_string(),
                message: message.to_string(),
                is_error,
            });
        }
    }

    pub fn update_tip(text: &str) {
        if let Some(tx) = TRAY_UI_TX.get() {
            let _ = tx.send(TrayUiAction::UpdateTooltip(text.to_string()));
        }
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}
