//! A bounded input reader wakes the UI; the UI blocks on input or task completion.
use crate::{app::TuiApp, render, state::LaunchOptions, terminal::TerminalSession};
use crossterm::event::{self, Event};
use jc_app::Wakeup;
use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const INPUT_EVENTS: usize = 64;
const INPUT_BYTES: usize = 8 * 1024 * 1024;
enum Input {
    Event(Event),
    Error(io::Error),
    RejectedPaste,
}
impl Input {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Event(Event::Paste(text)) => text.capacity(),
                _ => 0,
            }
    }
}
#[derive(Default)]
struct Queue {
    items: VecDeque<Input>,
    bytes: usize,
    closed: bool,
}
struct InputState {
    queue: Mutex<Queue>,
    space: Condvar,
    stop: AtomicBool,
    wakeup: Wakeup,
}
impl InputState {
    fn push(&self, mut event: Input) -> bool {
        if event.bytes() > INPUT_BYTES {
            event = Input::RejectedPaste;
        }
        let bytes = event.bytes();
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let mut queue = self
            .space
            .wait_while(queue, |q| {
                !q.closed && (q.items.len() >= INPUT_EVENTS || q.bytes + bytes > INPUT_BYTES)
            })
            .unwrap_or_else(|e| e.into_inner());
        if queue.closed {
            return false;
        }
        queue.bytes += bytes;
        queue.items.push_back(event);
        drop(queue);
        self.wakeup.signal();
        true
    }
    fn pop(&self) -> Option<Input> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let value = queue.items.pop_front()?;
        queue.bytes -= value.bytes();
        drop(queue);
        self.space.notify_one();
        Some(value)
    }
    fn pending(&self) -> bool {
        !self
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .is_empty()
    }
    fn close(&self) {
        self.stop.store(true, Ordering::Release);
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        self.space.notify_all();
    }
}
struct InputPump {
    shared: Arc<InputState>,
    handle: Option<JoinHandle<()>>,
}
impl InputPump {
    fn start(wakeup: Wakeup) -> io::Result<Self> {
        let shared = Arc::new(InputState {
            queue: Mutex::new(Queue::default()),
            space: Condvar::new(),
            stop: AtomicBool::new(false),
            wakeup,
        });
        let reader = shared.clone();
        let handle = thread::Builder::new()
            .name("jc-terminal-input".to_owned())
            .spawn(move || {
                while !reader.stop.load(Ordering::Acquire) {
                    let result = event::poll(Duration::from_millis(100)).and_then(|ready| {
                        if ready {
                            event::read().map(Some)
                        } else {
                            Ok(None)
                        }
                    });
                    match result {
                        Ok(Some(event)) => {
                            if !reader.push(Input::Event(event)) {
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            reader.push(Input::Error(error));
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            shared,
            handle: Some(handle),
        })
    }
}
impl Drop for InputPump {
    fn drop(&mut self) {
        self.shared.close();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(crate) fn run(options: LaunchOptions) -> io::Result<()> {
    let mut terminal = TerminalSession::enter()?;
    let wakeup = Wakeup::default();
    let mut app = TuiApp::with_wakeup(options, wakeup.clone());
    let input = InputPump::start(wakeup.clone())?;
    let (mut width, mut height) = terminal.size()?;
    let mut dirty = true;
    while !app.should_exit() {
        let ticket = wakeup.ticket();
        for _ in 0..32 {
            let Some(message) = input.shared.pop() else {
                break;
            };
            match message {
                Input::Event(event) => {
                    if let Event::Resize(w, h) = event {
                        width = w;
                        height = h;
                        terminal.invalidate();
                    }
                    if matches!(event, Event::FocusGained) {
                        terminal.invalidate();
                    }
                    if !matches!(event, Event::Mouse(_)) {
                        app.handle_event(event);
                        dirty = true;
                    }
                }
                Input::Error(error) => return Err(error),
                Input::RejectedPaste => {
                    app.status = "Paste exceeds the 8 MiB input budget".to_owned();
                    dirty = true;
                }
            }
        }
        let tasks = app.poll_tasks();
        dirty |= tasks.changed;
        if tasks.recover {
            terminal.invalidate();
            dirty = true;
        }
        if app.should_exit() {
            break;
        }
        if dirty {
            let presentation = app.prepare_frame(width, height);
            let screen = render::render(&presentation);
            terminal.draw(&screen)?;
            dirty = false;
        }
        if input.shared.pending() || app.tasks.has_completed() {
            continue;
        }
        wakeup.wait(ticket, app.tasks.next_deadline());
    }
    drop(input);
    drop(app);
    terminal.restore()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_queue_rejects_oversized_paste_and_accounts_retained_strings() {
        let shared = InputState {
            queue: Mutex::new(Queue::default()),
            space: Condvar::new(),
            stop: AtomicBool::new(false),
            wakeup: Wakeup::default(),
        };
        shared.push(Input::Event(Event::Paste(String::with_capacity(
            INPUT_BYTES + 1,
        ))));
        assert!(matches!(shared.pop(), Some(Input::RejectedPaste)));
        assert_eq!(shared.queue.lock().unwrap().bytes, 0);
        shared.push(Input::Event(Event::Paste("abc".to_owned())));
        assert!(shared.queue.lock().unwrap().bytes < INPUT_BYTES);
        shared.pop();
        assert_eq!(shared.queue.lock().unwrap().bytes, 0);
    }
    #[test]
    fn closing_a_full_queue_releases_a_blocked_reader() {
        let shared = Arc::new(InputState {
            queue: Mutex::new(Queue::default()),
            space: Condvar::new(),
            stop: AtomicBool::new(false),
            wakeup: Wakeup::default(),
        });
        for _ in 0..INPUT_EVENTS {
            assert!(shared.push(Input::RejectedPaste));
        }
        let writer = shared.clone();
        let handle = thread::spawn(move || writer.push(Input::RejectedPaste));
        shared.close();
        assert!(!handle.join().unwrap());
    }
}
