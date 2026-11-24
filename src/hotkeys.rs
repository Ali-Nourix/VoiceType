//! A dedicated Win32 message pump keeps shortcuts independent of the UI window.
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, HotKeyState,
};
use std::{collections::HashMap, sync::mpsc, time::Duration};
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
            MOD_SHIFT, VK_ESCAPE,
        },
        WindowsAndMessaging::{
            GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
            WM_QUIT,
        },
    },
};
const COMMAND: u32 = WM_APP + 17;
enum Command {
    Register(HotKey, mpsc::Sender<Result<(), String>>),
    Unregister(HotKey, mpsc::Sender<Result<(), String>>),
    Quit,
}
pub struct Manager {
    thread_id: u32,
    tx: mpsc::Sender<Command>,
}
impl Manager {
    pub fn new(
        events: mpsc::Sender<GlobalHotKeyEvent>,
        ctx: eframe::egui::Context,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("VoiceType-hotkey".into())
            .spawn(move || unsafe {
                let mut msg: MSG = std::mem::zeroed();
                PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                let _ = ready_tx.send(GetCurrentThreadId());
                let mut registered: HashMap<u32, (i32, HotKey)> = HashMap::new();
                let mut next = 1i32;
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    if msg.message == COMMAND {
                        while let Ok(command) = rx.try_recv() {
                            match command {
                                Command::Register(key, reply) => {
                                    let vk = vk(key.key);
                                    let result = match vk {
                                        Some(vk) => {
                                            let mut mods = MOD_NOREPEAT;
                                            if key.mods.contains(Modifiers::ALT) {
                                                mods |= MOD_ALT
                                            };
                                            if key.mods.contains(Modifiers::CONTROL) {
                                                mods |= MOD_CONTROL
                                            };
                                            if key.mods.contains(Modifiers::SHIFT) {
                                                mods |= MOD_SHIFT
                                            };
                                            if registered.contains_key(&key.id()) {
                                                Ok(())
                                            } else if RegisterHotKey(
                                                std::ptr::null_mut(),
                                                next,
                                                mods,
                                                vk,
                                            ) == 0
                                            {
                                                Err(std::io::Error::last_os_error().to_string())
                                            } else {
                                                registered.insert(key.id(), (next, key));
                                                next += 1;
                                                Ok(())
                                            }
                                        }
                                        None => Err("Unsupported shortcut key".into()),
                                    };
                                    let _ = reply.send(result);
                                }
                                Command::Unregister(key, reply) => {
                                    let result = if let Some((id, _)) = registered.remove(&key.id())
                                    {
                                        if UnregisterHotKey(std::ptr::null_mut(), id) == 0 {
                                            Err(std::io::Error::last_os_error().to_string())
                                        } else {
                                            Ok(())
                                        }
                                    } else {
                                        Ok(())
                                    };
                                    let _ = reply.send(result);
                                }
                                Command::Quit => {
                                    for (_, (id, _)) in registered.drain() {
                                        UnregisterHotKey(std::ptr::null_mut(), id);
                                    }
                                    return;
                                }
                            }
                        }
                    } else if msg.message == WM_HOTKEY {
                        if let Some((_, key)) = registered
                            .values()
                            .find(|(id, _)| *id == msg.wParam as i32)
                            .copied()
                        {
                            let _ = events.send(GlobalHotKeyEvent {
                                id: key.id(),
                                state: HotKeyState::Pressed,
                            });
                            ctx.request_repaint();
                            let sender = events.clone();
                            let wake = ctx.clone();
                            let code = ((msg.lParam as u32 >> 16) & 0xffff) as i32;
                            std::thread::spawn(move || {
                                let deadline = std::time::Instant::now() + Duration::from_secs(35);
                                loop {
                                    if GetAsyncKeyState(code) as u16 & 0x8000 == 0
                                        || std::time::Instant::now() >= deadline
                                    {
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(12));
                                }
                                let _ = sender.send(GlobalHotKeyEvent {
                                    id: key.id(),
                                    state: HotKeyState::Released,
                                });
                                wake.request_repaint();
                            });
                        }
                    } else if msg.message == WM_QUIT {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "Shortcut thread did not start".to_owned())?;
        Ok(Self { thread_id, tx })
    }
    fn send(&self, cmd: Command) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "Shortcut thread stopped".to_string())?;
        if unsafe { PostThreadMessageW(self.thread_id, COMMAND, 0, 0) } == 0 {
            return Err("Could not wake shortcut thread".into());
        }
        Ok(())
    }
    pub fn register(&self, key: HotKey) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::Register(key, tx))?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Shortcut registration timed out".to_string())?
    }
    pub fn unregister(&self, key: HotKey) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::Unregister(key, tx))?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Shortcut unregistration timed out".to_string())?
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.send(Command::Quit);
    }
}
fn vk(key: Code) -> Option<u32> {
    if key == Code::Escape {
        return Some(VK_ESCAPE as u32);
    }
    let name = format!("{key}");
    let letter = name.strip_prefix("Key")?;
    let bytes = letter.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_uppercase() {
        Some(bytes[0] as u32)
    } else {
        None
    }
}
