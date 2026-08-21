//! Training data: events, and the sample that collects them.

use rustc_hash::FxHashMap;

use crate::dynval::DynValue;

/// One observed event: an outcome, and the contextual predicates that held.
///
/// The predicates are a **set**: duplicates are dropped on construction, the
/// first occurrence winning, because a feature is an indicator of membership
/// and cannot fire twice for one context. The surviving order is preserved and
/// is observable — it fixes the order in which a context's scores accumulate.
///
/// ```
/// use verbora_classifiers::Event;
///
/// let e = Event::new("weather", ["word=rain", "prefix=ra", "word=rain"]);
/// assert_eq!(e.outcome(), "weather");
/// assert_eq!(e.predicates(), ["word=rain", "prefix=ra"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    outcome: String,
    predicates: Vec<String>,
}

impl Event {
    /// An event pairing `outcome` with the set of `predicates`.
    ///
    /// Any string is accepted for either: an outcome is a label and a predicate
    /// is an opaque key, so the empty string is a legal (if unhelpful) value for
    /// both, and neither is trimmed, lowercased or tokenised. Deriving
    /// predicates from text is the caller's job, which is exactly why no
    /// tokenizer or stemmer is fingerprinted into a maximum-entropy model.
    pub fn new(
        outcome: impl Into<String>,
        predicates: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut kept: Vec<String> = Vec::new();
        for predicate in predicates {
            let predicate = predicate.into();
            if !kept.contains(&predicate) {
                kept.push(predicate);
            }
        }
        Self {
            outcome: outcome.into(),
            predicates: kept,
        }
    }

    /// The observed outcome.
    pub fn outcome(&self) -> &str {
        &self.outcome
    }

    /// The context: distinct predicates, in first-occurrence order.
    pub fn predicates(&self) -> &[String] {
        &self.predicates
    }

    /// The serialised shape [`MaxEntClassifier::to_json`](crate::MaxEntClassifier::to_json)
    /// writes.
    pub(crate) fn to_value(&self) -> DynValue {
        DynValue::Obj(vec![
            ("outcome".to_owned(), DynValue::Str(self.outcome.clone())),
            (
                "predicates".to_owned(),
                DynValue::Arr(self.predicates.iter().cloned().map(DynValue::Str).collect()),
            ),
        ])
    }

    /// Reads one event back. A member that is absent or of the wrong type is
    /// read as its empty value, so a truncated file yields a smaller sample
    /// rather than a wrong one.
    pub(crate) fn from_value(value: &DynValue) -> Self {
        let outcome = value
            .get("outcome")
            .and_then(DynValue::as_str)
            .unwrap_or_default();
        let predicates: Vec<&str> = match value.get("predicates") {
            Some(DynValue::Arr(items)) => items.iter().filter_map(DynValue::as_str).collect(),
            _ => Vec::new(),
        };
        Self::new(outcome, predicates)
    }
}

/// The observed events a model is fitted to.
///
/// A sample is an ordered multiset: events keep their insertion order and
/// duplicates are kept, because `N` and every expectation are counted over
/// occurrences. The outcome list is maintained alongside in **first-appearance
/// order**, and that order is the model's outcome order — it decides how tied
/// probabilities are ranked and how the exponentials of a context are summed.
///
/// ```
/// use verbora_classifiers::{Event, Sample};
///
/// let sample: Sample = [
///     Event::new("sun", ["sky=blue"]),
///     Event::new("rain", ["sky=grey"]),
///     Event::new("sun", ["sky=blue"]),
/// ]
/// .into_iter()
/// .collect();
///
/// assert_eq!(sample.len(), 3);
/// assert_eq!(sample.outcomes(), ["sun", "rain"]);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    events: Vec<Event>,
    outcomes: Vec<String>,
}

impl Sample {
    /// An empty sample.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one event.
    pub fn push(&mut self, event: Event) {
        if !self.outcomes.contains(&event.outcome) {
            self.outcomes.push(event.outcome.clone());
        }
        self.events.push(event);
    }

    /// Records an event built from an outcome and its predicates.
    ///
    /// ```
    /// use verbora_classifiers::Sample;
    ///
    /// let mut sample = Sample::new();
    /// sample.add("spam", ["has=link", "sender=unknown"]);
    /// assert_eq!(sample.len(), 1);
    /// ```
    pub fn add(
        &mut self,
        outcome: impl Into<String>,
        predicates: impl IntoIterator<Item = impl Into<String>>,
    ) {
        self.push(Event::new(outcome, predicates));
    }

    /// The events, in insertion order, duplicates included.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The distinct outcomes, in first-appearance order.
    pub fn outcomes(&self) -> &[String] {
        &self.outcomes
    }

    /// How many events the sample holds.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the sample holds no events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// How often each outcome was observed, aligned with [`Self::outcomes`].
    ///
    /// ```
    /// use verbora_classifiers::Sample;
    ///
    /// let mut sample = Sample::new();
    /// sample.add("a", ["p"]);
    /// sample.add("b", ["p"]);
    /// sample.add("a", ["q"]);
    /// assert_eq!(sample.outcome_counts(), vec![2, 1]);
    /// ```
    pub fn outcome_counts(&self) -> Vec<usize> {
        let index: FxHashMap<&str, usize> = self
            .outcomes
            .iter()
            .enumerate()
            .map(|(i, o)| (o.as_str(), i))
            .collect();
        let mut counts = vec![0usize; self.outcomes.len()];
        for event in &self.events {
            if let Some(&i) = index.get(event.outcome.as_str()) {
                counts[i] += 1;
            }
        }
        counts
    }

    /// The serialised shape [`MaxEntClassifier::to_json`](crate::MaxEntClassifier::to_json)
    /// writes.
    ///
    /// Only the events are written: the outcome list is a function of them and
    /// is rebuilt on load, so it cannot disagree with them.
    pub(crate) fn to_value(&self) -> DynValue {
        DynValue::Obj(vec![(
            "events".to_owned(),
            DynValue::Arr(self.events.iter().map(Event::to_value).collect()),
        )])
    }

    /// Reads a sample back from [`Self::to_value`] output.
    pub(crate) fn from_value(value: &DynValue) -> Self {
        let mut sample = Self::new();
        if let Some(DynValue::Arr(events)) = value.get("events") {
            for event in events {
                sample.push(Event::from_value(event));
            }
        }
        sample
    }
}

impl FromIterator<Event> for Sample {
    fn from_iter<I: IntoIterator<Item = Event>>(iter: I) -> Self {
        let mut sample = Self::new();
        sample.extend(iter);
        sample
    }
}

impl Extend<Event> for Sample {
    fn extend<I: IntoIterator<Item = Event>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.events.reserve(iter.size_hint().0);
        for event in iter {
            self.push(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_is_a_set_keeping_first_occurrence_order() {
        let e = Event::new("y", ["b", "a", "b", "c", "a"]);
        assert_eq!(e.predicates(), ["b", "a", "c"]);
    }

    #[test]
    fn an_event_accepts_empty_strings_verbatim() {
        let e = Event::new("", ["", " x "]);
        assert_eq!(e.outcome(), "");
        assert_eq!(e.predicates(), ["", " x "]);
    }

    #[test]
    fn outcomes_are_first_appearance_order_and_events_keep_duplicates() {
        let mut sample = Sample::new();
        sample.add("b", ["p"]);
        sample.add("a", ["p"]);
        sample.add("b", ["p"]);
        assert_eq!(sample.outcomes(), ["b", "a"]);
        assert_eq!(sample.len(), 3);
        assert_eq!(sample.outcome_counts(), vec![2, 1]);
    }

    #[test]
    fn an_empty_sample_reports_itself_as_one() {
        let sample = Sample::new();
        assert!(sample.is_empty());
        assert_eq!(sample.len(), 0);
        assert!(sample.outcomes().is_empty());
        assert_eq!(sample.outcome_counts(), Vec::<usize>::new());
    }

    #[test]
    fn a_sample_collects_from_an_iterator_of_events() {
        let sample: Sample = (0..4)
            .map(|i| Event::new(if i % 2 == 0 { "even" } else { "odd" }, [format!("i={i}")]))
            .collect();
        assert_eq!(sample.len(), 4);
        assert_eq!(sample.outcomes(), ["even", "odd"]);
    }

    #[test]
    fn events_round_trip_through_their_serialised_shape() {
        let mut sample = Sample::new();
        sample.add("Ελλάδα", ["w=Αθήνα", "w=😀"]);
        sample.add("日本語", ["w=東京"]);
        let revived = Sample::from_value(&sample.to_value());
        assert_eq!(revived, sample);
    }

    #[test]
    fn a_truncated_event_reads_as_its_empty_value() {
        let value = DynValue::Obj(vec![("outcome".to_owned(), DynValue::Str("x".to_owned()))]);
        assert_eq!(
            Event::from_value(&value),
            Event::new("x", Vec::<&str>::new())
        );
        assert_eq!(
            Event::from_value(&DynValue::Null),
            Event::new("", Vec::<&str>::new())
        );
    }
}
