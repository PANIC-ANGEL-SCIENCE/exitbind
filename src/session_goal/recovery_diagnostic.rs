//! Scoped, per-thread wall-time observations for the large recovery fixture.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Acquire,
    Newlines,
    ParseResolve,
    Validate,
    CloneRemove,
    HashValue,
    SemanticLink,
    Canonical,
    Digest,
}

#[derive(Debug, Default)]
struct Sample {
    elapsed: Duration,
    calls: usize,
}

#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub(crate) input_bytes: usize,
    pub(crate) newlines: usize,
    pub(crate) physical_precount: usize,
    pub(crate) attempted_records: usize,
    pub(crate) completed_records: usize,
    pub(crate) processed_record_bytes: usize,
    pub(crate) budget_refusal: Option<&'static str>,
    acquire: Sample,
    newlines_time: Sample,
    parse_resolve: Sample,
    validate: Sample,
    clone_remove: Sample,
    hash_value: Sample,
    semantic_link: Sample,
    canonical: Sample,
    digest: Sample,
}

impl Snapshot {
    fn sample(&mut self, phase: Phase) -> &mut Sample {
        match phase {
            Phase::Acquire => &mut self.acquire,
            Phase::Newlines => &mut self.newlines_time,
            Phase::ParseResolve => &mut self.parse_resolve,
            Phase::Validate => &mut self.validate,
            Phase::CloneRemove => &mut self.clone_remove,
            Phase::HashValue => &mut self.hash_value,
            Phase::SemanticLink => &mut self.semantic_link,
            Phase::Canonical => &mut self.canonical,
            Phase::Digest => &mut self.digest,
        }
    }

    pub(crate) fn top_level(&self) -> Duration {
        self.acquire.elapsed
            + self.newlines_time.elapsed
            + self.parse_resolve.elapsed
            + self.validate.elapsed
    }
}

#[derive(Default)]
struct State {
    next: u64,
    active: Option<(u64, Snapshot)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub(crate) struct Scope {
    id: u64,
    finished: bool,
    not_send: PhantomData<Rc<()>>,
}

pub(crate) fn start() -> Scope {
    let id = STATE.with(|state| {
        let mut state = state.borrow_mut();
        assert!(state.active.is_none(), "recovery diagnostic already active");
        state.next = state.next.wrapping_add(1);
        let id = state.next;
        state.active = Some((id, Snapshot::default()));
        id
    });
    Scope {
        id,
        finished: false,
        not_send: PhantomData,
    }
}

impl Scope {
    pub(crate) fn finish(mut self) -> Snapshot {
        let snapshot = STATE.with(|state| {
            let mut state = state.borrow_mut();
            match state.active.take() {
                Some((id, snapshot)) if id == self.id => snapshot,
                other => {
                    state.active = other;
                    panic!("recovery diagnostic scope changed")
                }
            }
        });
        self.finished = true;
        snapshot
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if !self.finished {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                if state.active.as_ref().is_some_and(|(id, _)| *id == self.id) {
                    state.active = None;
                }
            });
        }
    }
}

pub(crate) struct Timer {
    phase: Phase,
    started: Option<(u64, Instant)>,
    not_send: PhantomData<Rc<()>>,
}

pub(crate) fn timer(phase: Phase) -> Timer {
    let id = STATE.with(|state| state.borrow().active.as_ref().map(|(id, _)| *id));
    Timer {
        phase,
        started: id.map(|id| (id, Instant::now())),
        not_send: PhantomData,
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some((id, started)) = self.started {
            let elapsed = started.elapsed();
            STATE.with(|state| {
                if let Some((active_id, snapshot)) = state.borrow_mut().active.as_mut() {
                    if *active_id == id {
                        let sample = snapshot.sample(self.phase);
                        sample.elapsed += elapsed;
                        sample.calls += 1;
                    }
                }
            });
        }
    }
}

fn update(f: impl FnOnce(&mut Snapshot)) {
    STATE.with(|state| {
        if let Some((_, snapshot)) = state.borrow_mut().active.as_mut() {
            f(snapshot);
        }
    });
}

pub(crate) fn input_bytes(bytes: usize) {
    update(|snapshot| snapshot.input_bytes = bytes);
}

pub(crate) fn newline_counts(newlines: usize, physical_precount: usize) {
    update(|snapshot| {
        snapshot.newlines = newlines;
        snapshot.physical_precount = physical_precount;
    });
}

pub(crate) fn attempt(bytes: usize) {
    update(|snapshot| {
        snapshot.attempted_records += 1;
        snapshot.processed_record_bytes += bytes;
    });
}

pub(crate) fn completed() {
    update(|snapshot| snapshot.completed_records += 1);
}

pub(crate) fn budget_refusal(checkpoint: &'static str) {
    update(|snapshot| snapshot.budget_refusal = Some(checkpoint));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_is_thread_local_and_ignores_inactive_calls() {
        let scope = start();
        drop(timer(Phase::Acquire));
        let other = std::thread::spawn(|| {
            drop(timer(Phase::Acquire));
            let scope = start();
            drop(timer(Phase::Digest));
            scope.finish()
        })
        .join()
        .unwrap();
        let this = scope.finish();
        assert_eq!(this.acquire.calls, 1);
        assert_eq!(this.digest.calls, 0);
        assert_eq!(other.acquire.calls, 0);
        assert_eq!(other.digest.calls, 1);
    }

    #[test]
    fn early_return_records_its_timer_and_old_timers_cannot_cross_scopes() {
        fn early() -> Result<(), ()> {
            let _timer = timer(Phase::ParseResolve);
            Err(())
        }
        let scope = start();
        assert!(early().is_err());
        let stale = timer(Phase::Digest);
        let first = scope.finish();
        assert_eq!(first.parse_resolve.calls, 1);
        assert_eq!(first.digest.calls, 0);
        let scope = start();
        drop(stale);
        assert_eq!(scope.finish().digest.calls, 0);
    }
}
