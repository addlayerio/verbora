//! A two-buffer rewrite pipeline.
//!
//! Metaphone is specified as a sequence of whole-string rewrites — twenty-two
//! The reference methods, thirty regular expressions — and the reference allocates a
//! fresh string for every one of them. Keeping that staged shape is worth a lot
//! for auditability: each rule can be read beside the pattern it ports. Keeping
//! the allocations is not. [`Pipe`] holds two buffers and swaps them after each
//! rule, so an encoding costs two allocations no matter how many rewrites run.

use crate::units::Unit;

/// Two buffers, alternating as input and output for a chain of rewrites.
pub(crate) struct Pipe<U> {
    cur: Vec<U>,
    next: Vec<U>,
}

impl<U: Unit> Pipe<U> {
    /// Starts a pipeline over a copy of `input`.
    pub(crate) fn new(input: &[U]) -> Self {
        Self {
            cur: input.to_vec(),
            // Rules can grow the text (`x` becomes `ks`), so leave headroom.
            next: Vec::with_capacity(input.len() + 8),
        }
    }

    /// Applies one rewrite: `rule` reads the current text and writes the next.
    #[inline]
    pub(crate) fn apply(&mut self, rule: impl Fn(&[U], &mut Vec<U>)) {
        self.next.clear();
        rule(&self.cur, &mut self.next);
        std::mem::swap(&mut self.cur, &mut self.next);
    }

    /// The text after every applied rewrite.
    pub(crate) fn finish(self) -> Vec<U> {
        self.cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_compose_in_order() {
        let mut p = Pipe::new(b"abc");
        p.apply(|input, out| out.extend(input.iter().map(|&b| b + 1)));
        p.apply(|input, out| out.extend(input.iter().rev()));
        assert_eq!(p.finish(), b"dcb");
    }

    #[test]
    fn buffers_are_reused_not_reallocated() {
        let mut p = Pipe::new(b"abc");
        for _ in 0..10 {
            p.apply(|input, out| out.extend_from_slice(input));
        }
        assert_eq!(p.finish(), b"abc");
    }
}
