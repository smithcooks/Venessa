use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    pub target_vk: u32,
    pub require_ctrl: bool,
    pub require_alt: bool,
    pub require_shift: bool,
    pub require_win: bool,
    #[allow(dead_code)]
    pub is_modifier_only: bool,
}

impl HotkeyConfig {
    pub fn parse(hotkey_str: &str) -> Self {
        let mut require_ctrl = false;
        let mut require_alt = false;
        let mut require_shift = false;
        let mut require_win = false;
        let mut target_vk = 0u32;

        for part in hotkey_str.split('+') {
            let p = part.trim();
            match p.to_lowercase().as_str() {
                "ctrl" | "control" | "cmdorctrl" | "commandorcontrol" => require_ctrl = true,
                "alt" | "option" => require_alt = true,
                "shift" => require_shift = true,
                "win" | "meta" | "super" | "cmd" | "command" => require_win = true,
                "space" => target_vk = 0x20,
                "backtick" | "`" => target_vk = 0xC0, // VK_OEM_3
                "capslock" | "caps" => target_vk = 0x14, // VK_CAPITAL
                "tab" => target_vk = 0x09,
                "enter" | "return" => target_vk = 0x0D,
                "f1" => target_vk = 0x70,
                "f2" => target_vk = 0x71,
                "f3" => target_vk = 0x72,
                "f4" => target_vk = 0x73,
                "f5" => target_vk = 0x74,
                "f6" => target_vk = 0x75,
                "f7" => target_vk = 0x76,
                "f8" => target_vk = 0x77,
                "f9" => target_vk = 0x78,
                "f10" => target_vk = 0x79,
                "f11" => target_vk = 0x7A,
                "f12" => target_vk = 0x7B,
                other => {
                    if other.len() == 1 {
                        let c = other.chars().next().unwrap().to_ascii_uppercase();
                        if c.is_ascii_alphanumeric() {
                            target_vk = c as u32;
                        }
                    }
                }
            }
        }

        let is_modifier_only = target_vk == 0 && (require_ctrl || require_alt || require_shift || require_win);

        // Fallback to Space if nothing valid
        if target_vk == 0 && !is_modifier_only {
            target_vk = 0x20;
            require_ctrl = true;
        }

        Self {
            target_vk,
            require_ctrl,
            require_alt,
            require_shift,
            require_win,
            is_modifier_only,
        }
    }
}

static HOOK_SENDER: OnceLock<Sender<HotkeyEvent>> = OnceLock::new();
static HOOK_CONFIG: OnceLock<HotkeyConfig> = OnceLock::new();
static IS_KEY_DOWN: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
pub struct HotkeyManager {
    _thread_handle: Option<std::thread::JoinHandle<()>>,
    is_running: Arc<AtomicBool>,
}

#[cfg(windows)]
impl HotkeyManager {
    pub fn start(config: HotkeyConfig, sender: Sender<HotkeyEvent>) -> Self {
        let _ = HOOK_SENDER.set(sender);
        let _ = HOOK_CONFIG.set(config);
        IS_KEY_DOWN.store(false, Ordering::SeqCst);

        let is_running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&is_running);

        let thread_handle = std::thread::spawn(move || {
            use windows_sys::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
                VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
                TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG,
                WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
            };

            static mut HOOK_HANDLE: HHOOK = std::ptr::null_mut();

            unsafe extern "system" fn hook_proc(
                n_code: i32,
                w_param: WPARAM,
                l_param: LPARAM,
            ) -> LRESULT {
                if n_code >= 0 {
                    let kbd = *(l_param as *const KBDLLHOOKSTRUCT);
                    let is_down_event = w_param as u32 == WM_KEYDOWN || w_param as u32 == WM_SYSKEYDOWN;
                    let is_up_event = w_param as u32 == WM_KEYUP || w_param as u32 == WM_SYSKEYUP;

                    if let Some(cfg) = HOOK_CONFIG.get() {
                        let ctrl_down = (GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_LCONTROL as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_RCONTROL as i32) as u16 & 0x8000 != 0);

                        let alt_down = (GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_LMENU as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_RMENU as i32) as u16 & 0x8000 != 0);

                        let shift_down = (GetAsyncKeyState(VK_SHIFT as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_LSHIFT as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_RSHIFT as i32) as u16 & 0x8000 != 0);

                        let win_down = (GetAsyncKeyState(VK_LWIN as i32) as u16 & 0x8000 != 0)
                            || (GetAsyncKeyState(VK_RWIN as i32) as u16 & 0x8000 != 0);

                        let mods_satisfied = (!cfg.require_ctrl || ctrl_down)
                            && (!cfg.require_alt || alt_down)
                            && (!cfg.require_shift || shift_down)
                            && (!cfg.require_win || win_down);

                        if kbd.vkCode == cfg.target_vk {
                            if is_down_event && mods_satisfied {
                                if !IS_KEY_DOWN.swap(true, Ordering::SeqCst) {
                                    if let Some(s) = HOOK_SENDER.get() {
                                        let _ = s.send(HotkeyEvent::Pressed);
                                    }
                                }
                            } else if is_up_event {
                                if IS_KEY_DOWN.swap(false, Ordering::SeqCst) {
                                    if let Some(s) = HOOK_SENDER.get() {
                                        let _ = s.send(HotkeyEvent::Released);
                                    }
                                }
                            }
                        } else if IS_KEY_DOWN.load(Ordering::SeqCst) && !mods_satisfied {
                            // If a required modifier was released while target key was held
                            if IS_KEY_DOWN.swap(false, Ordering::SeqCst) {
                                if let Some(s) = HOOK_SENDER.get() {
                                    let _ = s.send(HotkeyEvent::Released);
                                }
                            }
                        }
                    }
                }
                CallNextHookEx(HOOK_HANDLE, n_code, w_param, l_param)
            }

            unsafe {
                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    0 as HINSTANCE,
                    0,
                );

                HOOK_HANDLE = hook;

                let mut msg: MSG = std::mem::zeroed();
                while running_clone.load(Ordering::SeqCst) && GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                if !hook.is_null() {
                    UnhookWindowsHookEx(hook);
                }
            }
        });

        Self {
            _thread_handle: Some(thread_handle),
            is_running,
        }
    }

    #[allow(dead_code)]
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}
