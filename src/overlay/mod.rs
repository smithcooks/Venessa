use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayState {
    Hidden,
    Listening,
    Transcribing,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum OverlayCommand {
    ShowListening,
    ShowTranscribing,
    ShowError(String),
    Hide,
}

static OVERLAY_TX: OnceLock<Sender<OverlayCommand>> = OnceLock::new();
static OVERLAY_HWND: AtomicUsize = AtomicUsize::new(0);
static OVERLAY_STATE: OnceLock<RwLock<OverlayState>> = OnceLock::new();

#[cfg(windows)]
pub struct StatusOverlay {
    _private: (),
}

#[cfg(windows)]
impl StatusOverlay {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<OverlayCommand>();
        let _ = OVERLAY_TX.set(tx);
        let _ = OVERLAY_STATE.set(RwLock::new(OverlayState::Hidden));

        let (init_tx, init_rx) = crossbeam_channel::bounded::<()>(1);

        // 1. Dedicated Win32 Window Thread
        std::thread::spawn(move || {
            use std::ptr::null_mut;
            use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
            use windows_sys::Win32::Graphics::Gdi::{
                BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
                DrawTextW, EndPaint, FillRect, FrameRgn, SetBkMode, SetTextColor,
                SetWindowRgn, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBRUSH, HFONT, HRGN,
                PAINTSTRUCT, TRANSPARENT,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                GetSystemMetrics, KillTimer, PostQuitMessage, RegisterClassW,
                ShowWindow, TranslateMessage, MSG, SM_CXSCREEN, SW_HIDE, WM_DESTROY,
                WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            };

            const TIMER_AUTO_HIDE: usize = 1;
            static mut FONT_HANDLE: HFONT = 0 as _;

            unsafe extern "system" fn overlay_wnd_proc(
                hwnd: HWND,
                msg: u32,
                w_param: WPARAM,
                l_param: LPARAM,
            ) -> LRESULT {
                match msg {
                    WM_PAINT => {
                        let mut ps: PAINTSTRUCT = std::mem::zeroed();
                        let hdc = BeginPaint(hwnd, &mut ps);

                        let mut rect = RECT {
                            left: 0,
                            top: 0,
                            right: 240,
                            bottom: 44,
                        };

                        let state = OVERLAY_STATE
                            .get()
                            .and_then(|l| l.read().ok())
                            .map(|s| (*s).clone())
                            .unwrap_or(OverlayState::Hidden);

                        let (bg_color, border_color, text_color, label) = match &state {
                            OverlayState::Listening => (
                                0x001B1B1E, // Dark background
                                0x007EEB10, // Emerald Green RGB(16, 235, 126)
                                0x00FFFFFF, // White
                                "🎙️  Listening...",
                            ),
                            OverlayState::Transcribing => (
                                0x001E1B1B,
                                0x00F6823B, // Blue RGB(59, 130, 246)
                                0x00E0E0E0,
                                "⏳  Transcribing...",
                            ),
                            OverlayState::Error(err) => (
                                0x001A1828,
                                0x004444EF, // Red RGB(239, 68, 68)
                                0x008080FF,
                                if err.is_empty() { "⚠️  Error" } else { err.as_str() },
                            ),
                            OverlayState::Hidden => (0, 0, 0, ""),
                        };

                        // 1. Fill background
                        let bg_brush: HBRUSH = CreateSolidBrush(bg_color as COLORREF);
                        FillRect(hdc, &rect, bg_brush);
                        DeleteObject(bg_brush as _);

                        // 2. Frame border
                        let rgn: HRGN = CreateRoundRectRgn(0, 0, 240, 44, 22, 22);
                        let border_brush: HBRUSH = CreateSolidBrush(border_color as COLORREF);
                        FrameRgn(hdc, rgn, border_brush, 2, 2);
                        DeleteObject(border_brush as _);
                        DeleteObject(rgn as _);

                        // 3. Draw text
                        windows_sys::Win32::Graphics::Gdi::SelectObject(hdc, FONT_HANDLE as _);
                        SetBkMode(hdc, TRANSPARENT as i32);
                        SetTextColor(hdc, text_color as COLORREF);

                        let text_u16: Vec<u16> = label.encode_utf16().collect();
                        DrawTextW(
                            hdc,
                            text_u16.as_ptr(),
                            text_u16.len() as i32,
                            &mut rect,
                            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                        );

                        EndPaint(hwnd, &ps);
                        0
                    }
                    WM_TIMER => {
                        if w_param == TIMER_AUTO_HIDE {
                            KillTimer(hwnd, TIMER_AUTO_HIDE);
                            if let Some(l) = OVERLAY_STATE.get() {
                                if let Ok(mut s) = l.write() {
                                    *s = OverlayState::Hidden;
                                }
                            }
                            ShowWindow(hwnd, SW_HIDE);
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
                let class_name: Vec<u16> = "VenessaOverlayClass\0".encode_utf16().collect();
                let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();

                FONT_HANDLE = CreateFontW(
                    -14, 0, 0, 0, 600, 0, 0, 0, 1, 0, 0, 0, 0, font_name.as_ptr()
                );

                let wnd_class = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(overlay_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: 0 as HINSTANCE,
                    hIcon: 0 as _,
                    hCursor: 0 as _,
                    hbrBackground: 0 as _,
                    lpszMenuName: null_mut(),
                    lpszClassName: class_name.as_ptr(),
                };

                RegisterClassW(&wnd_class);

                let width = 240;
                let height = 44;
                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                let x = (screen_w - width) / 2;
                let y = 35; // Top center of display

                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                    class_name.as_ptr(),
                    class_name.as_ptr(),
                    WS_POPUP,
                    x,
                    y,
                    width,
                    height,
                    0 as _,
                    0 as _,
                    0 as HINSTANCE,
                    null_mut(),
                );

                // Rounded window clipping region
                let rgn = CreateRoundRectRgn(0, 0, width, height, 22, 22);
                SetWindowRgn(hwnd, rgn, 1);

                OVERLAY_HWND.store(hwnd as usize, Ordering::SeqCst);
                let _ = init_tx.send(());

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                if FONT_HANDLE != 0 as _ {
                    DeleteObject(FONT_HANDLE as _);
                }
                DestroyWindow(hwnd);
            }
        });

        let _ = init_rx.recv();

        // 2. Command dispatcher thread
        std::thread::spawn(move || {
            use windows_sys::Win32::Foundation::HWND;
            use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                KillTimer, SetTimer, SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE,
                SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE,
            };

            const TIMER_AUTO_HIDE: usize = 1;

            let hwnd = OVERLAY_HWND.load(Ordering::SeqCst) as HWND;
            while let Ok(cmd) = rx.recv() {
                unsafe {
                    match cmd {
                        OverlayCommand::ShowListening => {
                            KillTimer(hwnd, TIMER_AUTO_HIDE);
                            if let Some(l) = OVERLAY_STATE.get() {
                                if let Ok(mut s) = l.write() {
                                    *s = OverlayState::Listening;
                                }
                            }
                            InvalidateRect(hwnd, std::ptr::null(), 1);
                            SetWindowPos(
                                hwnd,
                                HWND_TOPMOST,
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        }
                        OverlayCommand::ShowTranscribing => {
                            KillTimer(hwnd, TIMER_AUTO_HIDE);
                            if let Some(l) = OVERLAY_STATE.get() {
                                if let Ok(mut s) = l.write() {
                                    *s = OverlayState::Transcribing;
                                }
                            }
                            InvalidateRect(hwnd, std::ptr::null(), 1);
                            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        }
                        OverlayCommand::ShowError(msg) => {
                            if let Some(l) = OVERLAY_STATE.get() {
                                if let Ok(mut s) = l.write() {
                                    *s = OverlayState::Error(msg);
                                }
                            }
                            InvalidateRect(hwnd, std::ptr::null(), 1);
                            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            SetTimer(hwnd, TIMER_AUTO_HIDE, 3500, None);
                        }
                        OverlayCommand::Hide => {
                            KillTimer(hwnd, TIMER_AUTO_HIDE);
                            if let Some(l) = OVERLAY_STATE.get() {
                                if let Ok(mut s) = l.write() {
                                    *s = OverlayState::Hidden;
                                }
                            }
                            ShowWindow(hwnd, SW_HIDE);
                        }
                    }
                }
            }
        });

        Self { _private: () }
    }

    pub fn show_listening(&self) {
        if let Some(tx) = OVERLAY_TX.get() {
            let _ = tx.send(OverlayCommand::ShowListening);
        }
    }

    pub fn show_transcribing(&self) {
        if let Some(tx) = OVERLAY_TX.get() {
            let _ = tx.send(OverlayCommand::ShowTranscribing);
        }
    }

    pub fn show_error(&self, msg: &str) {
        if let Some(tx) = OVERLAY_TX.get() {
            let _ = tx.send(OverlayCommand::ShowError(msg.to_string()));
        }
    }

    pub fn hide(&self) {
        if let Some(tx) = OVERLAY_TX.get() {
            let _ = tx.send(OverlayCommand::Hide);
        }
    }
}
