use std::time::Duration;

pub struct Retry {
    attempts_remaining: usize,
    next_duration: Duration,
}

impl Retry {
    pub fn new(start: Duration, count: usize) -> Self {
        Self { attempts_remaining: count, next_duration: start }
    }

    pub fn fail(&mut self) -> Option<Duration> {
        if self.attempts_remaining == 0 {
            return None;
        }
        let backoff = self.next_duration;
        self.next_duration *= 2;
        Some(backoff)
    }
}
