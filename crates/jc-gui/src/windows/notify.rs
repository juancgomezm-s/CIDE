//! A coalesced message to this window only. No bridge thread or live model pointer.
use super::ffi::{Hwnd, PostMessageW, TASKS_MESSAGE};
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct Bridge(Mutex<Delivery>);
#[derive(Default)]
struct Delivery {
    hwnd: usize,
    posted: bool,
    failed: bool,
}
impl Bridge {
    pub fn attach(&self, hwnd: Hwnd) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.hwnd = hwnd as usize;
        state.posted = false;
    }
    pub fn detach(&self) {
        // Serialize against PostMessage before HWND destruction/reuse. Never
        // hold this guard while joining workers or borrowing the UI model.
        self.attach(std::ptr::null_mut());
    }
    pub fn signal(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.hwnd == 0 || state.posted {
            return;
        }
        state.posted = true;
        if unsafe { PostMessageW(state.hwnd as Hwnd, TASKS_MESSAGE, 0, 0) } == 0 {
            state.posted = false;
            state.failed = true;
        }
    }
    pub fn acknowledge(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.posted = false;
        std::mem::take(&mut state.failed)
    }
}
