//! Lazy event-driven owner for a terminal UI which has no Win32 message pump.
//! Only bounded owned text crosses this single-request mailbox; never a model,
//! Document, FileSession, callback into the application or clipboard history.
use std::{
    ffi::c_void,
    marker::PhantomData,
    ptr,
    rc::Rc,
    sync::{mpsc, Arc, Mutex},
    thread,
};
const REQUEST: u32 = 0x8c1d;
const LIMIT: usize = 2 * 1024 * 1024;
type Reply = mpsc::SyncSender<Result<Option<String>, String>>;
enum Request {
    Read { limit: usize, reply: Reply },
    Write { text: String, reply: Reply },
    Stop,
}
#[derive(Default)]
struct Mailbox {
    request: Option<Request>,
    closed: bool,
}
type Inbox = Arc<Mutex<Mailbox>>;
struct CloseMailbox(Inbox);
impl Drop for CloseMailbox {
    fn drop(&mut self) {
        let mut inbox = self.0.lock().unwrap_or_else(|p| p.into_inner());
        inbox.closed = true;
        inbox.request.take();
    }
}
#[repr(C)]
struct Message {
    window: *mut c_void,
    id: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    x: i32,
    y: i32,
    private: u32,
}
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
    fn GetMessageW(message: *mut Message, window: *mut c_void, first: u32, last: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn PostThreadMessageW(thread: u32, message: u32, wparam: usize, lparam: isize) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}
struct Window {
    handle: *mut c_void,
    _thread: PhantomData<Rc<()>>,
}
impl Window {
    fn create() -> Result<Self, String> {
        let class = [83u16, 84, 65, 84, 73, 67, 0]; // predefined STATIC, never WS_VISIBLE
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
struct Worker {
    inbox: Inbox,
    thread: u32,
    #[cfg(test)]
    window: usize,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    fn start() -> Result<Self, String> {
        let inbox: Inbox = Arc::new(Mutex::new(Mailbox::default()));
        let receiver = inbox.clone();
        let (ready, initialized) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("cide-clipboard".into())
            .spawn(move || {
                let _close_mailbox = CloseMailbox(receiver.clone());
                let window = match Window::create() {
                    Ok(w) => w,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                // CreateWindow already created this thread's message queue. Publish
                // readiness only now, before the first PostThreadMessage.
                if ready
                    .send(Ok((
                        unsafe { GetCurrentThreadId() },
                        window.handle as usize,
                    )))
                    .is_err()
                {
                    return;
                }
                loop {
                    let mut message: Message = unsafe { std::mem::zeroed() };
                    let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
                    if result <= 0 {
                        break;
                    }
                    if message.window.is_null() && message.id == REQUEST {
                        // Release the lock before calling another clipboard owner.
                        let request = receiver
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .request
                            .take();
                        match request {
                            Some(Request::Read { limit, reply }) => {
                                let _ = reply.send(super::clipboard::read_limited(
                                    window.handle as usize,
                                    limit,
                                ));
                            }
                            Some(Request::Write { text, reply }) => {
                                let _ = reply.send(
                                    super::clipboard::write(window.handle as usize, &text)
                                        .map(|()| None),
                                );
                            }
                            Some(Request::Stop) => break,
                            None => {}
                        }
                    } else {
                        unsafe {
                            TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                }
                // A failed message loop must drop a pending reply sender too, so a
                // requester cannot remain blocked on an exited owner's mailbox.
            })
            .map_err(|e| format!("Clipboard thread creation: {e}"))?;
        let initialized = initialized
            .recv()
            .map_err(|_| "Clipboard owner initialization failed".to_owned())
            .and_then(|r| r);
        match initialized {
            Ok((thread, window)) => {
                #[cfg(not(test))]
                let _ = window;
                Ok(Self {
                    inbox,
                    thread,
                    #[cfg(test)]
                    window,
                    handle: Some(handle),
                })
            }
            Err(error) => {
                let _ = handle.join();
                Err(error)
            }
        }
    }
    fn submit(&mut self, request: Request) -> Result<(), String> {
        if self.handle.as_ref().is_none_or(|h| h.is_finished()) {
            return Err("Clipboard owner stopped".into());
        }
        {
            let mut slot = self.inbox.lock().unwrap_or_else(|p| p.into_inner());
            if slot.closed {
                return Err("Clipboard owner stopped".into());
            }
            if slot.request.is_some() {
                return Err("Clipboard request already in flight".into());
            }
            slot.request = Some(request);
        }
        if unsafe { PostThreadMessageW(self.thread, REQUEST, 0, 0) } == 0 {
            self.inbox
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .request
                .take();
            return Err(format!(
                "Clipboard request delivery: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if handle.is_finished() {
            let _ = handle.join();
            return;
        }
        self.inbox.lock().unwrap_or_else(|p| p.into_inner()).request = Some(Request::Stop);
        if unsafe { PostThreadMessageW(self.thread, REQUEST, 0, 0) } != 0 {
            let _ = handle.join();
        }
        // On a failed post, detach rather than block forever on an unwakeable
        // queue. Process exit still owns/reclaims the thread; never kill it.
    }
}
#[derive(Default)]
pub struct WindowsClipboard {
    worker: Option<Worker>,
}
impl WindowsClipboard {
    fn worker(&mut self) -> Result<&mut Worker, String> {
        if self
            .worker
            .as_ref()
            .is_some_and(|w| w.handle.as_ref().is_none_or(|h| h.is_finished()))
        {
            self.worker = None;
        }
        if self.worker.is_none() {
            self.worker = Some(Worker::start()?);
        }
        Ok(self.worker.as_mut().expect("clipboard owner initialized"))
    }
    fn transfer(&mut self, make: impl FnOnce(Reply) -> Request) -> Result<Option<String>, String> {
        let (reply, result) = mpsc::sync_channel(1);
        self.worker()?.submit(make(reply))?;
        result
            .recv()
            .map_err(|_| "Clipboard owner closed during transfer".to_owned())?
    }
    pub fn read(&mut self, limit: usize) -> Result<Option<String>, String> {
        if limit == 0 || limit > LIMIT {
            return Err("Invalid clipboard destination budget".into());
        }
        self.transfer(|reply| Request::Read { limit, reply })
    }
    pub fn write(&mut self, text: &str) -> Result<(), String> {
        if text.len() > LIMIT || text.as_bytes().contains(&0) {
            return Err("Clipboard text exceeds 2 MiB or contains NUL".into());
        }
        self.transfer(|reply| Request::Write {
            text: text.to_owned(),
            reply,
        })
        .map(|_| ())
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
        fn SendMessageTimeoutW(
            window: *mut c_void,
            message: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            milliseconds: u32,
            result: *mut usize,
        ) -> isize;
    }
    #[test]
    fn owner_is_lazy_invisible_ours_and_destroyed_without_opening_clipboard() {
        let mut clipboard = WindowsClipboard::default();
        assert!(clipboard.worker.is_none());
        assert!(clipboard.read(0).is_err());
        assert!(clipboard.write("a\0b").is_err());
        assert!(clipboard.worker.is_none());
        let worker = clipboard.worker().unwrap();
        let handle = worker.window as *mut c_void;
        let mut process = 0;
        let thread = unsafe { GetWindowThreadProcessId(handle, &mut process) };
        assert_eq!(thread, worker.thread);
        assert_ne!(thread, unsafe { GetCurrentThreadId() });
        assert_eq!(process, std::process::id());
        assert_eq!(unsafe { IsWindowVisible(handle) }, 0);
        // A different UI thread must remain responsive even while CIDE's main
        // thread is not pumping messages. WM_NULL touches no clipboard data.
        for _ in 0..4 {
            let mut result = 0;
            assert_ne!(
                unsafe { SendMessageTimeoutW(handle, 0, 0, 0, 2, 1000, &mut result) },
                0
            );
        }
        drop(clipboard);
        assert_eq!(unsafe { IsWindow(handle) }, 0);
    }
}
