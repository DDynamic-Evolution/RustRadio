use rand::seq::SliceRandom;

#[derive(Default)]
pub struct Rotation {
    remaining: Vec<usize>,
}

impl Rotation {
    pub fn reset(&mut self, len: usize, current: Option<usize>) {
        self.remaining = (0..len).filter(|index| Some(*index) != current).collect();
        self.remaining.shuffle(&mut rand::rng());
    }

    pub fn mark_played(&mut self, index: usize) {
        self.remaining.retain(|candidate| *candidate != index);
    }

    pub fn candidates(&mut self, len: usize, current: Option<usize>) -> Vec<usize> {
        if self.remaining.is_empty() {
            self.reset(len, None);
            if len > 1 && self.remaining.first().copied() == current {
                self.remaining.swap(0, 1);
            }
        }
        self.remaining.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_plays_each_track_and_avoids_a_repeat_at_wrap() {
        let mut rotation = Rotation::default();
        rotation.reset(4, Some(0));
        let mut played = vec![0];
        for _ in 0..3 {
            let next = rotation.candidates(4, played.last().copied())[0];
            assert!(!played.contains(&next));
            rotation.mark_played(next);
            played.push(next);
        }
        assert_ne!(rotation.candidates(4, played.last().copied())[0], played[3]);
        assert_eq!(Rotation::default().candidates(1, Some(0)), vec![0]);
    }
}
