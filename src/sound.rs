use crate::model::{Status, Task};

/// An actual transition, never a heartbeat or a replayed historical record.
pub fn transition(before: Option<Status>, after: Status) -> bool {
    before != Some(after) && matches!(after, Status::Waiting | Status::Done)
}
pub fn still_relevant(task: &Task, state: Status) -> bool {
    !task.stale && task.status() == state
}
/// Locally synthesized 16 kHz mono PCM16. Short envelopes prevent clicks.
pub fn pcm(state: Status) -> Vec<u8> {
    let notes: &[(f32, usize)] = if state == Status::Waiting {
        &[(660.0, 180), (0.0, 110), (660.0, 260)]
    } else {
        &[(523.25, 130), (659.25, 130), (783.99, 300)]
    };
    let mut bytes = Vec::new();
    for &(frequency, milliseconds) in notes {
        let count = milliseconds * 16;
        for i in 0..count {
            let attack = (i as f32 / 160.0).min(1.0);
            let release = ((count - i) as f32 / 480.0).min(1.0);
            let phase = std::f32::consts::TAU * frequency * i as f32 / 16000.0;
            let sample =
                ((phase.sin() + 0.15 * (2.0 * phase).sin()) * attack * release * 14000.0) as i16;
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    bytes
}
pub fn wav(state: Status) -> Vec<u8> {
    let pcm = pcm(state);
    let length = pcm.len() as u32;
    let mut bytes = Vec::with_capacity(pcm.len() + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&32000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&pcm);
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chimes_only_on_transition_and_are_short_distinct_pcm() {
        assert!(transition(Some(Status::Working), Status::Waiting));
        assert!(transition(Some(Status::Working), Status::Done));
        assert!(!transition(Some(Status::Waiting), Status::Waiting));
        assert!(!transition(Some(Status::Done), Status::Done));
        assert!(!transition(Some(Status::Waiting), Status::Working));
        assert_ne!(pcm(Status::Waiting), pcm(Status::Done));
        for status in [Status::Waiting, Status::Done] {
            let bytes = pcm(status);
            assert!(bytes.len() < 32000 && bytes.len() > 8000);
            assert_eq!(bytes.len() % 2, 0);
            let encoded = wav(status);
            assert_eq!(&encoded[..4], b"RIFF");
            assert_eq!(&encoded[8..16], b"WAVEfmt ");
            assert_eq!(&encoded[24..28], &16000u32.to_le_bytes());
            assert_eq!(&encoded[44..], &bytes);
        }
    }
}
