//! `Store`: the app's single view of engine data.
//!
//! Views read from it and send commands through it; they never hold an
//! `EngineHandle`. Engine updates arrive on engine threads, are forwarded into the
//! GPUI executor, refresh the cached snapshot and are re-emitted as [`StoreEvent`]s
//! (plus `cx.notify()`, so `cx.observe(&store, …)` is enough for most views).
//!
//! ```ignore
//! let store = Store::global(cx);
//! let snapshot = store.read(cx).snapshot();          // &Snapshot
//! let detail = store.read(cx).thread(&thread_id);    // Option<Rc<ThreadDetail>>
//! store.read(cx).send(Command::MarkVisited { thread_id });
//! cx.subscribe(&store, |this, _, event: &StoreEvent, cx| …).detach();
//! ```
//!
//! Until `EngineHandle::demo()` exists the store falls back to `fixtures`, behind
//! the same interface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use futures::StreamExt as _;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global};
use tern_core::{Command, Snapshot, ThreadDetail, ThreadId, Update};
use tern_engine::{EngineHandle, Paths};

use crate::fixtures;

/// What the store is backed by.
pub enum Backend {
    Engine(EngineHandle),
    /// Local fixture data; commands are dropped.
    Fixture,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StoreEvent {
    /// Projects, thread shells or provider statuses changed.
    Snapshot,
    /// One thread's transcript changed.
    Thread(ThreadId),
    /// A command failed; the shell shows it as an error toast.
    Error(String),
    /// A notice for the user; the shell shows it as a toast.
    Notice(String),
}

pub struct Store {
    backend: Backend,
    /// Started with `--demo`: project paths are made up.
    demo: bool,
    snapshot: Snapshot,
    threads: RefCell<HashMap<ThreadId, Rc<ThreadDetail>>>,
}

impl EventEmitter<StoreEvent> for Store {}

struct GlobalStore(Entity<Store>);
impl Global for GlobalStore {}

/// Runs an engine constructor that may still be a `todo!()` stub without printing
/// the panic.
fn try_engine(f: impl FnOnce() -> anyhow::Result<EngineHandle>) -> Option<EngineHandle> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let engine = f().ok()?;
        // The snapshot call must work too before we commit to this backend.
        let _ = engine.snapshot();
        Some(engine)
    }));
    std::panic::set_hook(hook);
    result.ok().flatten()
}

impl Store {
    /// Starts the engine (`demo` for `--demo`), falling back to fixtures when the
    /// engine is unavailable. Registers the store as a global.
    pub fn install(demo: bool, cx: &mut App) -> Entity<Store> {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Update>();
        let on_update: Box<dyn Fn(Update) + Send + Sync> = Box::new(move |update| {
            let _ = tx.unbounded_send(update);
        });
        let engine = if demo {
            try_engine(|| Ok(EngineHandle::demo(on_update)))
        } else {
            try_engine(|| EngineHandle::start(Paths::from_env(), on_update))
        };
        let backend = match engine {
            Some(engine) => Backend::Engine(engine),
            None => {
                eprintln!("tern: engine unavailable, showing fixture data");
                Backend::Fixture
            }
        };
        let store = cx.new(|cx: &mut Context<Store>| {
            cx.spawn(async move |this, cx| {
                while let Some(update) = rx.next().await {
                    if this
                        .update(cx, |store, cx| store.apply(update, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            let snapshot = match &backend {
                Backend::Engine(engine) => engine.snapshot(),
                Backend::Fixture => fixtures::snapshot(),
            };
            Store {
                backend,
                demo,
                snapshot,
                threads: RefCell::new(HashMap::new()),
            }
        });
        cx.set_global(GlobalStore(store.clone()));
        store
    }

    pub fn global(cx: &App) -> Entity<Store> {
        cx.global::<GlobalStore>().0.clone()
    }

    /// Demo or fixture data, whose project folders don't exist on this machine.
    pub fn is_demo(&self) -> bool {
        self.demo || self.is_fixture()
    }

    pub fn is_fixture(&self) -> bool {
        matches!(self.backend, Backend::Fixture)
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The full transcript of a thread, cached until the engine says it changed.
    pub fn thread(&self, id: &str) -> Option<Rc<ThreadDetail>> {
        if let Some(detail) = self.threads.borrow().get(id) {
            return Some(detail.clone());
        }
        let detail = match &self.backend {
            Backend::Engine(engine) => engine.thread(id),
            Backend::Fixture => fixtures::thread(&self.snapshot, id),
        }?;
        let detail = Rc::new(detail);
        self.threads
            .borrow_mut()
            .insert(id.to_string(), detail.clone());
        Some(detail)
    }

    /// Fire and forget; failures come back as [`StoreEvent::Error`].
    pub fn send(&self, command: Command) {
        match &self.backend {
            Backend::Engine(engine) => engine.send(command),
            Backend::Fixture => {}
        }
    }

    pub fn engine(&self) -> Option<&EngineHandle> {
        match &self.backend {
            Backend::Engine(engine) => Some(engine),
            Backend::Fixture => None,
        }
    }

    fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Snapshot => {
                if let Backend::Engine(engine) = &self.backend {
                    self.snapshot = engine.snapshot();
                }
                cx.emit(StoreEvent::Snapshot);
            }
            Update::Thread(id) => {
                self.threads.borrow_mut().remove(&id);
                cx.emit(StoreEvent::Thread(id));
            }
            Update::Error { message } => cx.emit(StoreEvent::Error(message)),
            Update::Notice { message } => cx.emit(StoreEvent::Notice(message)),
        }
        cx.notify();
    }
}
