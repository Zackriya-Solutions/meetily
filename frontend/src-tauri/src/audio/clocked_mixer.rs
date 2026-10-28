use super::recording_state::DeviceType;
use std::collections::VecDeque;

/// Bounded, single-clock windows for app capture. Silence consumes a time slot;
/// it is not appended by a second producer competing with live audio.
pub struct ClockedMixer {
    mic: VecDeque<f32>,
    system: VecDeque<f32>,
    sample_rate: u32,
    emitted: usize,
}
impl ClockedMixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            mic: VecDeque::new(),
            system: VecDeque::new(),
            sample_rate,
            emitted: 0,
        }
    }
    pub fn push(&mut self, source: DeviceType, samples: Vec<f32>) {
        let queue = match source {
            DeviceType::Microphone => &mut self.mic,
            DeviceType::System => &mut self.system,
        };
        queue.extend(samples);
        // Bound latency and memory after a suspended process or delayed callback.
        let overflow = queue.len().saturating_sub(self.sample_rate as usize / 2);
        queue.drain(..overflow);
    }
    pub fn clear_pending(&mut self) {
        self.mic.clear();
        self.system.clear();
    }
    pub fn pop_until(&mut self, active_seconds: f64) -> Option<(Vec<f32>, Vec<f32>, f64)> {
        let window = self.sample_rate as usize / 20;
        let target = (active_seconds * self.sample_rate as f64) as usize;
        if self.emitted + window > target {
            return None;
        }
        let timestamp = self.emitted as f64 / self.sample_rate as f64;
        self.emitted += window;
        let mic = (0..window)
            .map(|_| self.mic.pop_front().unwrap_or(0.0))
            .collect();
        let system = (0..window)
            .map(|_| self.system.pop_front().unwrap_or(0.0))
            .collect();
        Some((mic, system, timestamp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sources_share_one_timeline_and_gaps_are_silence() {
        let mut mixer = ClockedMixer::new(1000);
        mixer.push(DeviceType::Microphone, vec![0.2; 50]);
        mixer.push(DeviceType::System, vec![0.4; 50]);
        let (mic, app, timestamp) = mixer.pop_until(0.05).unwrap();
        assert_eq!((mic, app, timestamp), (vec![0.2; 50], vec![0.4; 50], 0.0));
        assert!(mixer.pop_until(0.05).is_none());
        let (mic, app, timestamp) = mixer.pop_until(0.1).unwrap();
        assert_eq!((mic, app, timestamp), (vec![0.0; 50], vec![0.0; 50], 0.05));
        mixer.push(DeviceType::System, vec![0.7; 50]);
        let (mic, app, timestamp) = mixer.pop_until(0.15).unwrap();
        assert_eq!((mic, app, timestamp), (vec![0.0; 50], vec![0.7; 50], 0.1));
    }
    #[test]
    fn pause_discards_queued_audio_without_advancing_time() {
        let mut mixer = ClockedMixer::new(1000);
        mixer.push(DeviceType::System, vec![0.5; 50]);
        mixer.clear_pending();
        assert!(mixer.pop_until(0.0).is_none());
        let (_, app, timestamp) = mixer.pop_until(0.05).unwrap();
        assert_eq!((app, timestamp), (vec![0.0; 50], 0.0));
    }
}
