//! One owner-bound navigation request. No library or filesystem writes.
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}, mpsc};

pub const MAIN: &str = "main";
pub const SOURCE: &str = "sync";
const UNAVAILABLE: &str = "The main window could not open Display All Missing Files.";

#[derive(Default)]
pub struct Requests {
    next: AtomicU64,
    pending: Mutex<Option<(u64, mpsc::Sender<bool>)>>,
}

pub struct Pending {
    pub id: u64,
    pub answer: mpsc::Receiver<bool>,
}

impl Requests {
    pub fn begin(&self, owner: &str) -> Result<Pending, &'static str> {
        if owner != SOURCE { return Err("Only Sync Manager can request this handoff."); }
        let mut pending = self.pending.lock().map_err(|_| UNAVAILABLE)?;
        if pending.is_some() { return Err("A missing-file handoff is already in progress."); }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (sender, answer) = mpsc::channel();
        *pending = Some((id, sender));
        Ok(Pending { id, answer })
    }

    pub fn reply(&self, owner: &str, id: u64, opened: bool) -> Result<bool, &'static str> {
        if owner != MAIN { return Err("Only the main window can acknowledge the missing-file manager."); }
        let mut pending = self.pending.lock().map_err(|_| UNAVAILABLE)?;
        if pending.as_ref().is_none_or(|(current, _)| *current != id) { return Ok(false); }
        Ok(pending.take().is_some_and(|(_, sender)| sender.send(opened).is_ok()))
    }

    pub fn cancel(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            if pending.as_ref().is_some_and(|(current, _)| *current == id) { pending.take(); }
        }
    }

    pub fn window_closed(&self, owner: &str) {
        if matches!(owner, MAIN | SOURCE) {
            if let Ok(mut pending) = self.pending.lock() { pending.take(); }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "disposable owner-protocol fixtures")]
mod tests {
    use super::*;

    #[test]
    fn only_the_correct_windows_can_request_and_confirm_a_handoff() {
        let requests = Requests::default();
        assert!(requests.begin(MAIN).is_err());
        assert!(requests.begin("preferences").is_err());
        let pending = requests.begin(SOURCE).unwrap();
        assert!(requests.begin(SOURCE).is_err());
        assert!(requests.reply(SOURCE, pending.id, true).is_err());
        assert!(!requests.reply(MAIN, pending.id + 1, true).unwrap());
        assert!(pending.answer.try_recv().is_err());
        assert!(requests.reply(MAIN, pending.id, true).unwrap());
        assert!(pending.answer.recv().unwrap());
        assert!(!requests.reply(MAIN, pending.id, true).unwrap());
    }

    #[test]
    fn cancellation_and_window_loss_retire_only_their_pending_request() {
        let requests = Requests::default();
        let old = requests.begin(SOURCE).unwrap();
        requests.cancel(old.id);
        assert!(old.answer.recv().is_err());
        let new = requests.begin(SOURCE).unwrap();
        requests.cancel(old.id);
        assert!(!requests.reply(MAIN, old.id, true).unwrap());
        requests.window_closed("preferences");
        assert!(new.answer.try_recv().is_err());
        requests.window_closed(SOURCE);
        assert!(new.answer.recv().is_err());
        let pending = requests.begin(SOURCE).unwrap();
        requests.window_closed(MAIN);
        assert!(pending.answer.recv().is_err());
    }
}
