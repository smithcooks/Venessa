use crate::config::VenessaConfig;
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateFontW, HBRUSH, HFONT, COLOR_BTNFACE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
    GetWindowTextLengthW, GetWindowTextW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, HMENU, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW,
    WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION,
    WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

// Control IDs
const ID_API_KEY_EDIT: usize = 101;
const ID_MODEL_COMBO: usize = 102;
const ID_HOTKEY_EDIT: usize = 103;
const ID_MODE_COMBO: usize = 104;
const ID_SOUND_CHECK: usize = 105;
const ID_DELAY_EDIT: usize = 106;
const ID_PROMPT_EDIT: usize = 107;
const ID_SAVE_BTN: usize = 201;
const ID_CANCEL_BTN: usize = 202;

// Win32 control styles
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_AUTOCHECKBOX: u32 = 0x0003;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const BST_UNCHECKED: usize = 0;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;

static GUI_CONFIG: OnceLock<Arc<Mutex<VenessaConfig>>> = OnceLock::new();
static GUI_UPDATED_TX: OnceLock<Sender<VenessaConfig>> = OnceLock::new();
static GUI_HWND: AtomicUsize = AtomicUsize::new(0);

pub struct SettingsGui {
    _private: (),
}

impl SettingsGui {
    pub fn new(
        config: Arc<Mutex<VenessaConfig>>,
        updated_tx: Sender<VenessaConfig>,
    ) -> Self {
        let _ = GUI_CONFIG.set(config);
        let _ = GUI_UPDATED_TX.set(updated_tx);

        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<()>(1);

        std::thread::spawn(move || {
            unsafe {
                let class_name: Vec<u16> = "VenessaSettingsClass\0".encode_utf16().collect();
                let window_title: Vec<u16> = "Venessa - Settings & Configuration\0".encode_utf16().collect();

                let wnd_class = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(settings_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: 0 as HINSTANCE,
                    hIcon: 0 as _,
                    hCursor: 0 as _,
                    hbrBackground: (COLOR_BTNFACE + 1) as HBRUSH,
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class_name.as_ptr(),
                };

                RegisterClassW(&wnd_class);

                let width = 500;
                let height = 580;

                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                let screen_h = GetSystemMetrics(SM_CYSCREEN);
                let x = (screen_w - width) / 2;
                let y = (screen_h - height) / 2;

                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    window_title.as_ptr(),
                    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                    x,
                    y,
                    width,
                    height,
                    0 as _,
                    0 as _,
                    0 as HINSTANCE,
                    std::ptr::null(),
                );

                GUI_HWND.store(hwnd as usize, Ordering::SeqCst);
                setup_controls(hwnd);

                let _ = ready_tx.send(());

                let mut msg = std::mem::zeroed();
                while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });

        let _ = ready_rx.recv();
        Self { _private: () }
    }

    pub fn show(&self) {
        show_gui();
    }
}

pub fn show_gui() {
    let hwnd_val = GUI_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = hwnd_val as HWND;
        unsafe {
            load_config_into_controls(hwnd);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

unsafe fn create_font() -> HFONT {
    let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    CreateFontW(
        -13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, font_name.as_ptr()
    )
}

unsafe fn setup_controls(hwnd: HWND) {
    let hfont = create_font();

    let static_class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let edit_class: Vec<u16> = "EDIT\0".encode_utf16().collect();
    let button_class: Vec<u16> = "BUTTON\0".encode_utf16().collect();
    let combo_class: Vec<u16> = "COMBOBOX\0".encode_utf16().collect();

    let add_control = |class: *const u16, title: &str, style: u32, x, y, w, h, id: usize| -> HWND {
        let t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let ctrl = CreateWindowExW(
            0,
            class,
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            hwnd,
            id as HMENU,
            0 as HINSTANCE,
            std::ptr::null(),
        );
        SendMessageW(ctrl, WM_SETFONT, hfont as WPARAM, 1);
        ctrl
    };

    // 1. Gemini API Key
    add_control(static_class.as_ptr(), "Google Gemini API Key:", 0, 20, 15, 440, 20, 0);
    add_control(edit_class.as_ptr(), "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, 20, 38, 445, 24, ID_API_KEY_EDIT);
    add_control(static_class.as_ptr(), "Get a free key from Google AI Studio: aistudio.google.com/app/apikey", 0, 20, 64, 440, 18, 0);

    // 2. Gemini Model
    add_control(static_class.as_ptr(), "Gemini Model:", 0, 20, 90, 210, 20, 0);
    let model_combo = add_control(combo_class.as_ptr(), "", WS_BORDER | WS_TABSTOP | CBS_DROPDOWNLIST, 20, 112, 210, 160, ID_MODEL_COMBO);
    for m in &["gemini-3.5-flash-lite", "gemini-3-flash-preview", "gemini-3.8-flash", "gemini-3.5-transcribe"] {
        let text_u16: Vec<u16> = m.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(model_combo, CB_ADDSTRING, 0, text_u16.as_ptr() as LPARAM);
    }

    // 3. Hotkey
    add_control(static_class.as_ptr(), "Activation Hotkey:", 0, 255, 90, 210, 20, 0);
    add_control(edit_class.as_ptr(), "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, 255, 112, 210, 24, ID_HOTKEY_EDIT);
    add_control(static_class.as_ptr(), "e.g. Alt, Ctrl+Space, Alt+Space, F8, CapsLock", 0, 255, 138, 220, 16, 0);

    // 4. Trigger Mode
    add_control(static_class.as_ptr(), "Trigger Mode:", 0, 20, 160, 210, 20, 0);
    let mode_combo = add_control(combo_class.as_ptr(), "", WS_BORDER | WS_TABSTOP | CBS_DROPDOWNLIST, 20, 182, 210, 100, ID_MODE_COMBO);
    for m in &["hold (Push-to-Talk)", "toggle (Press to start/stop)"] {
        let text_u16: Vec<u16> = m.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(mode_combo, CB_ADDSTRING, 0, text_u16.as_ptr() as LPARAM);
    }

    // 5. Typing Delay
    add_control(static_class.as_ptr(), "Typing Delay (ms / char):", 0, 255, 160, 210, 20, 0);
    add_control(edit_class.as_ptr(), "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, 255, 182, 210, 24, ID_DELAY_EDIT);

    // 6. Sound Feedback
    add_control(button_class.as_ptr(), "Play subtle audio beep when recording starts and ends", BS_AUTOCHECKBOX | WS_TABSTOP, 20, 220, 440, 22, ID_SOUND_CHECK);

    // 7. Prompt
    add_control(static_class.as_ptr(), "Transcription & Disfluency Cleanup Prompt:", 0, 20, 252, 440, 20, 0);
    add_control(edit_class.as_ptr(), "", WS_BORDER | WS_TABSTOP | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL, 20, 274, 445, 190, ID_PROMPT_EDIT);

    // 8. Buttons
    add_control(button_class.as_ptr(), "💾 Save Settings", BS_DEFPUSHBUTTON | WS_TABSTOP, 140, 480, 150, 36, ID_SAVE_BTN);
    add_control(button_class.as_ptr(), "Close", WS_TABSTOP, 305, 480, 110, 36, ID_CANCEL_BTN);

    load_config_into_controls(hwnd);
}

unsafe fn load_config_into_controls(hwnd: HWND) {
    if let Some(cfg_lock) = GUI_CONFIG.get() {
        if let Ok(cfg) = cfg_lock.lock() {
            set_control_text(hwnd, ID_API_KEY_EDIT, &cfg.gemini_api_key);
            set_control_text(hwnd, ID_HOTKEY_EDIT, &cfg.hotkey);
            set_control_text(hwnd, ID_DELAY_EDIT, &cfg.type_delay_ms.to_string());
            set_control_text(hwnd, ID_PROMPT_EDIT, &cfg.prompt);

            // Select model in combo
            use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;
            let model_combo = GetDlgItem(hwnd, ID_MODEL_COMBO as i32);
            let idx = match cfg.gemini_model.as_str() {
                "gemini-3-flash-preview" => 1,
                "gemini-3.8-flash" => 2,
                "gemini-3.5-transcribe" => 3,
                _ => 0, // default gemini-3.5-flash-lite
            };
            SendMessageW(model_combo, CB_SETCURSEL, idx, 0);

            // Select mode in combo
            let mode_combo = GetDlgItem(hwnd, ID_MODE_COMBO as i32);
            let mode_idx = if cfg.mode == "toggle" { 1 } else { 0 };
            SendMessageW(mode_combo, CB_SETCURSEL, mode_idx, 0);

            // Checkbox
            let check_btn = GetDlgItem(hwnd, ID_SOUND_CHECK as i32);
            let state = if cfg.sound_feedback { BST_CHECKED } else { BST_UNCHECKED };
            SendMessageW(check_btn, BM_SETCHECK, state, 0);
        }
    }
}

unsafe fn save_controls_into_config(hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;

    let api_key = get_control_text(hwnd, ID_API_KEY_EDIT);
    let hotkey = get_control_text(hwnd, ID_HOTKEY_EDIT);
    let delay_str = get_control_text(hwnd, ID_DELAY_EDIT);
    let prompt = get_control_text(hwnd, ID_PROMPT_EDIT);

    let type_delay_ms = delay_str.trim().parse::<u64>().unwrap_or(2);

    let model_combo = GetDlgItem(hwnd, ID_MODEL_COMBO as i32);
    let model_idx = SendMessageW(model_combo, CB_GETCURSEL, 0, 0) as usize;
    let gemini_model = match model_idx {
        1 => "gemini-3-flash-preview".to_string(),
        2 => "gemini-3.8-flash".to_string(),
        3 => "gemini-3.5-transcribe".to_string(),
        _ => "gemini-3.5-flash-lite".to_string(),
    };

    let mode_combo = GetDlgItem(hwnd, ID_MODE_COMBO as i32);
    let mode_idx = SendMessageW(mode_combo, CB_GETCURSEL, 0, 0) as usize;
    let mode = if mode_idx == 1 { "toggle".to_string() } else { "hold".to_string() };

    let check_btn = GetDlgItem(hwnd, ID_SOUND_CHECK as i32);
    let sound_feedback = SendMessageW(check_btn, BM_GETCHECK, 0, 0) as usize == BST_CHECKED;

    let new_cfg = VenessaConfig {
        gemini_api_key: api_key.trim().to_string(),
        gemini_model,
        hotkey: hotkey.trim().to_string(),
        mode,
        type_delay_ms,
        sound_feedback,
        prompt: prompt.trim().to_string(),
    };

    let _ = new_cfg.save();

    if let Some(cfg_lock) = GUI_CONFIG.get() {
        if let Ok(mut c) = cfg_lock.lock() {
            *c = new_cfg.clone();
        }
    }

    if let Some(tx) = GUI_UPDATED_TX.get() {
        let _ = tx.send(new_cfg);
    }
}

unsafe fn set_control_text(hwnd: HWND, id: usize, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;
    let ctrl = GetDlgItem(hwnd, id as i32);
    let text_u16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    SetWindowTextW(ctrl, text_u16.as_ptr());
}

unsafe fn get_control_text(hwnd: HWND, id: usize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;
    let ctrl = GetDlgItem(hwnd, id as i32);
    let len = GetWindowTextLengthW(ctrl);
    if len <= 0 {
        return String::new();
    }
    let mut buf: Vec<u16> = vec![0; (len + 1) as usize];
    GetWindowTextW(ctrl, buf.as_mut_ptr(), len + 1);
    buf.pop(); // remove null terminator
    String::from_utf16_lossy(&buf)
}

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (w_param & 0xFFFF) as usize;
            if id == ID_SAVE_BTN {
                save_controls_into_config(hwnd);
                ShowWindow(hwnd, SW_HIDE);
            } else if id == ID_CANCEL_BTN {
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}
