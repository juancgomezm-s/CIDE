//! Lazy, thread-owned invisible HWND for terminal CF_UNICODETEXT ownership.
use std::{ffi::c_void, marker::PhantomData, ptr, rc::Rc};
#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        parameter: *mut c_void,
    ) -> *mut c_void;
    fn DestroyWindow(window: *mut c_void) -> i32;
}

#[derive(Default)]
pub struct WindowsClipboard {
    window: Option<Window>,
}
struct Window {
    handle: *mut c_void,
    _thread: PhantomData<Rc<()>>,
}
impl Window {
    fn create() -> Result<Self, String> {
        let class = [
            b'S' as u16,
            b'T' as u16,
            b'A' as u16,
            b'T' as u16,
            b'I' as u16,
            b'C' as u16,
            0,
        ];
        // STATIC is a predefined system class. HWND_MESSAGE=-3, no WS_VISIBLE.
        // No app WndProc, callback into the model or delayed rendering is used.
        let handle = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                0,
                0,
                -3isize as *mut c_void,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if handle.is_null() {
            return Err(format!(
                "Clipboard owner creation: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self {
            handle,
            _thread: PhantomData,
        })
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}
impl WindowsClipboard {
    /// Default construction performs no native calls; owner is lazy and !Send.
    fn owner(&mut self) -> Result<usize, String> {
        if self.window.is_none() {
            self.window = Some(Window::create()?);
        }
        Ok(self.window.as_ref().expect("owner initialized").handle as usize)
    }
    pub fn read(&mut self, limit: usize) -> Result<Option<String>, String> {
        super::clipboard::read_limited(self.owner()?, limit)
    }
    pub fn write(&mut self, text: &str) -> Result<(), String> {
        super::clipboard::write(self.owner()?, text)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[link(name = "user32")]
    extern "system" {
        fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
        fn IsWindowVisible(window: *mut c_void) -> i32;
        fn IsWindow(window: *mut c_void) -> i32;
    }
    #[test]
    fn owner_is_lazy_invisible_ours_and_destroyed_without_opening_clipboard() {
        let mut clipboard = WindowsClipboard::default();
        assert!(clipboard.window.is_none());
        let handle = clipboard.owner().unwrap() as *mut c_void;
        let mut process = 0;
        unsafe {
            assert_ne!(GetWindowThreadProcessId(handle, &mut process), 0);
            assert_eq!(IsWindowVisible(handle), 0);
        }
        assert_eq!(process, std::process::id());
        assert_eq!(clipboard.owner().unwrap(), handle as usize);
        drop(clipboard);
        assert_eq!(unsafe { IsWindow(handle) }, 0);
    }
}
