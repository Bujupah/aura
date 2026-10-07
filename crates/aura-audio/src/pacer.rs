use std::collections::VecDeque;

/// Turns bursty capture buffers into a stream that advances exactly with the
/// wall clock.
///
/// The realtime session timestamps its transcript by how much audio it has
/// received. Capture, however, may deliver nothing during silence and may
/// deliver late. The pacer is drained on a fixed tick and always yields one
/// tick's worth of audio: real samples when it has them, silence when it does
/// not. Session time therefore tracks real time, and two independent streams
/// stay comparable.
///
/// It is also the bound on memory: beyond `capacity` the oldest audio is
/// dropped, so a stalled network can never grow the buffer without limit.
pub struct Pacer {
    queue: VecDeque<i16>,
    tick: usize,
    capacity: usize,
    dropped: u64,
}

impl Pacer {
    /// `tick` is the number of samples per drain; `capacity` the most that
    /// may be held.
    pub fn new(tick: usize, capacity: usize) -> Self {
        assert!(tick > 0 && capacity >= tick * 2, "capacity must hold at least two ticks");
        Self {
            queue: VecDeque::with_capacity(capacity),
            tick,
            capacity,
            dropped: 0,
        }
    }

    pub fn push(&mut self, samples: &[i16]) {
        self.queue.extend(samples);
        let excess = self.queue.len().saturating_sub(self.capacity);
        if excess > 0 {
            self.queue.drain(..excess);
            self.dropped += excess as u64;
        }
    }

    /// Fills `out` with the audio for one tick.
    ///
    /// Normally that is exactly one tick. If padding has let a backlog build
    /// up (audio arrived after silence was already sent in its place), up to
    /// two ticks are released so the stream catches back up to real time.
    pub fn drain_tick(&mut self, out: &mut Vec<i16>) {
        out.clear();
        let backlog = self.queue.len() > self.tick * 3;
        let take = if backlog { self.tick * 2 } else { self.tick }.min(self.queue.len());
        out.extend(self.queue.drain(..take));
        if out.len() < self.tick {
            out.resize(self.tick, 0);
        }
    }

    /// Samples discarded because the consumer fell too far behind.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(pacer: &mut Pacer) -> Vec<i16> {
        let mut out = Vec::new();
        pacer.drain_tick(&mut out);
        out
    }

    #[test]
    fn yields_silence_when_nothing_was_captured() {
        let mut pacer = Pacer::new(4, 16);
        assert_eq!(drain(&mut pacer), vec![0, 0, 0, 0]);
    }

    #[test]
    fn passes_audio_through_in_order_one_tick_at_a_time() {
        let mut pacer = Pacer::new(4, 16);
        pacer.push(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(drain(&mut pacer), vec![1, 2, 3, 4]);
        // The remainder is padded so the stream keeps real-time length.
        assert_eq!(drain(&mut pacer), vec![5, 6, 0, 0]);
    }

    #[test]
    fn releases_a_backlog_at_double_speed_until_caught_up() {
        let mut pacer = Pacer::new(4, 64);
        pacer.push(&(1..=14).collect::<Vec<i16>>());
        assert_eq!(drain(&mut pacer).len(), 8);
        // Six left: no longer a backlog, back to one tick.
        assert_eq!(drain(&mut pacer), vec![9, 10, 11, 12]);
    }

    #[test]
    fn drops_the_oldest_audio_when_full_and_counts_it() {
        let mut pacer = Pacer::new(2, 4);
        pacer.push(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(pacer.dropped(), 2);
        assert_eq!(drain(&mut pacer), vec![3, 4]);
    }
}
