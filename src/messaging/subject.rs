//! Shared subject-language primitives for FABRIC declarations and placement.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use thiserror::Error;

/// Canonical subject token used for routing and matching.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SubjectToken {
    /// Literal subject segment.
    Literal(String),
    /// Single-segment wildcard (`*`).
    One,
    /// Tail wildcard (`>`), which must be terminal.
    Tail,
}

impl fmt::Display for SubjectToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(value) => write!(f, "{value}"),
            Self::One => write!(f, "*"),
            Self::Tail => write!(f, ">"),
        }
    }
}

/// Errors produced while parsing subject patterns or concrete subjects.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SubjectPatternError {
    /// The parsed subject contained no non-empty segments.
    #[error("subject pattern must contain at least one segment")]
    EmptyPattern,
    /// Empty path segments such as `a..b` are not legal subject syntax.
    #[error("subject pattern must not contain empty segments")]
    EmptySegment,
    /// Whitespace inside a token would make canonical matching ambiguous.
    #[error("subject segment `{0}` must not contain whitespace")]
    WhitespaceInSegment(String),
    /// A tail wildcard appeared anywhere other than the final segment.
    #[error("tail wildcard `>` must be terminal")]
    TailWildcardMustBeTerminal,
    /// More than one terminal tail wildcard was present.
    #[error("subject pattern may not contain more than one tail wildcard")]
    MultipleTailWildcards,
    /// A literal segment embedded wildcard characters rather than being a pure token.
    #[error("literal segment `{0}` embeds wildcard characters")]
    EmbeddedWildcard(String),
    /// Prefix morphisms only permit exact literal segment rewrites.
    #[error("pattern `{0}` must contain only literal segments for prefix morphisms")]
    LiteralOnlyPatternRequired(String),
    /// Concrete subjects cannot carry wildcard tokens.
    #[error("subject `{0}` must not contain wildcard tokens")]
    WildcardsNotAllowed(String),
}

/// Parsed subject pattern with NATS-style wildcard support.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubjectPattern {
    raw: String,
    segments: Vec<SubjectToken>,
}

impl SubjectPattern {
    /// Construct a validated subject pattern from the canonical dotted representation.
    #[must_use]
    pub fn new(pattern: impl AsRef<str>) -> Self {
        Self::parse(pattern.as_ref()).expect("subject pattern must be syntactically valid")
    }

    /// Parse and canonicalize a subject pattern.
    pub fn parse(raw: &str) -> Result<Self, SubjectPatternError> {
        let segments = parse_pattern_tokens(raw)?;
        Self::from_tokens(segments)
    }

    /// Build a pattern from already-tokenized segments.
    pub fn from_tokens(segments: Vec<SubjectToken>) -> Result<Self, SubjectPatternError> {
        validate_pattern_tokens(&segments)?;
        Ok(Self {
            raw: canonicalize_tokens(&segments),
            segments,
        })
    }

    /// Return the canonical dotted subject string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Return a stable string key used for hashing and diagnostics.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        self.raw.clone()
    }

    /// Return the canonical pattern segments.
    #[must_use]
    pub fn segments(&self) -> &[SubjectToken] {
        &self.segments
    }

    /// Return true when the pattern ends in a tail wildcard.
    #[must_use]
    pub fn is_full_wildcard(&self) -> bool {
        matches!(self.segments.last(), Some(SubjectToken::Tail))
    }

    /// Return true when the pattern contains any wildcard tokens.
    #[must_use]
    pub fn has_wildcards(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| !matches!(segment, SubjectToken::Literal(_)))
    }

    /// Return true if this pattern matches the provided concrete subject.
    #[must_use]
    pub fn matches(&self, subject: &Subject) -> bool {
        matches_subject_tokens(&self.segments, subject.tokens())
    }

    /// Return true if two patterns can match at least one common subject.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        overlaps_tokens(&self.segments, &other.segments)
    }
}

impl Default for SubjectPattern {
    fn default() -> Self {
        Self::new("fabric.default")
    }
}

impl fmt::Display for SubjectPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for SubjectPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SubjectPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Concrete subject without wildcard tokens.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subject {
    raw: String,
    tokens: Vec<String>,
}

impl Subject {
    /// Construct a validated concrete subject.
    #[must_use]
    pub fn new(subject: impl AsRef<str>) -> Self {
        Self::parse(subject.as_ref()).expect("subject must be syntactically valid")
    }

    /// Parse and canonicalize a concrete subject.
    pub fn parse(raw: &str) -> Result<Self, SubjectPatternError> {
        let pattern = SubjectPattern::parse(raw)?;
        if pattern.has_wildcards() {
            return Err(SubjectPatternError::WildcardsNotAllowed(
                pattern.as_str().to_owned(),
            ));
        }

        let tokens = pattern
            .segments()
            .iter()
            .map(|segment| match segment {
                SubjectToken::Literal(value) => value.clone(),
                SubjectToken::One | SubjectToken::Tail => unreachable!("wildcards rejected above"),
            })
            .collect::<Vec<_>>();

        Ok(Self {
            raw: pattern.as_str().to_owned(),
            tokens,
        })
    }

    /// Return the canonical dotted subject string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Return the literal subject tokens.
    #[must_use]
    pub fn tokens(&self) -> &[String] {
        &self.tokens
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Subject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Subject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl From<&Subject> for SubjectPattern {
    fn from(subject: &Subject) -> Self {
        let segments = subject
            .tokens()
            .iter()
            .cloned()
            .map(SubjectToken::Literal)
            .collect::<Vec<_>>();
        Self::from_tokens(segments).expect("concrete subjects always form valid patterns")
    }
}

fn parse_pattern_tokens(raw: &str) -> Result<Vec<SubjectToken>, SubjectPatternError> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err(SubjectPatternError::EmptyPattern);
    }

    let mut segments = Vec::new();
    for segment in normalized.split('.') {
        if segment.is_empty() {
            return Err(SubjectPatternError::EmptySegment);
        }
        if segment.chars().any(char::is_whitespace) {
            return Err(SubjectPatternError::WhitespaceInSegment(segment.to_owned()));
        }

        let token = match segment {
            "*" => SubjectToken::One,
            ">" => SubjectToken::Tail,
            literal if literal.contains('*') || literal.contains('>') => {
                return Err(SubjectPatternError::EmbeddedWildcard(literal.to_owned()));
            }
            literal => SubjectToken::Literal(literal.to_owned()),
        };
        segments.push(token);
    }

    validate_pattern_tokens(&segments)?;
    Ok(segments)
}

fn validate_pattern_tokens(segments: &[SubjectToken]) -> Result<(), SubjectPatternError> {
    if segments.is_empty() {
        return Err(SubjectPatternError::EmptyPattern);
    }

    let tail_count = segments
        .iter()
        .filter(|segment| matches!(segment, SubjectToken::Tail))
        .count();
    if tail_count > 1 {
        return Err(SubjectPatternError::MultipleTailWildcards);
    }

    if let Some(position) = segments
        .iter()
        .position(|segment| matches!(segment, SubjectToken::Tail))
        && position + 1 != segments.len()
    {
        return Err(SubjectPatternError::TailWildcardMustBeTerminal);
    }

    Ok(())
}

fn canonicalize_tokens(segments: &[SubjectToken]) -> String {
    segments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn matches_subject_tokens(pattern: &[SubjectToken], subject: &[String]) -> bool {
    match (pattern.split_first(), subject.split_first()) {
        (None, None) => true,
        (Some((SubjectToken::Tail, _)), Some(_)) => true,
        (Some((SubjectToken::One, pattern_tail)), Some((_, subject_tail))) => {
            matches_subject_tokens(pattern_tail, subject_tail)
        }
        (Some((SubjectToken::Literal(expected), pattern_tail)), Some((actual, subject_tail)))
            if expected == actual =>
        {
            matches_subject_tokens(pattern_tail, subject_tail)
        }
        _ => false,
    }
}

fn overlaps_tokens(left: &[SubjectToken], right: &[SubjectToken]) -> bool {
    match (left.split_first(), right.split_first()) {
        (None, Some(_)) | (Some(_), None) => false,
        (None, None)
        | (Some((SubjectToken::Tail, _)), Some(_))
        | (Some(_), Some((SubjectToken::Tail, _))) => true,
        (Some((left_head, left_tail)), Some((right_head, right_tail))) => {
            if segments_can_match(left_head, right_head) {
                overlaps_tokens(left_tail, right_tail)
            } else {
                false
            }
        }
    }
}

fn segments_can_match(left: &SubjectToken, right: &SubjectToken) -> bool {
    match (left, right) {
        (SubjectToken::Literal(left), SubjectToken::Literal(right)) => left == right,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Sublist: trie-based subject routing engine
// ---------------------------------------------------------------------------

use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Opaque subscription identifier assigned by the [`Sublist`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    /// Return the raw numeric identifier.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sub-{}", self.0)
    }
}

/// Subscriber entry stored inside the trie.
#[derive(Debug, Clone)]
struct Subscriber {
    id: SubscriptionId,
    /// Optional queue group name. Only one subscriber per group per message.
    queue_group: Option<String>,
}

/// Internal trie node keyed by subject token.
#[derive(Debug, Default)]
struct TrieNode {
    /// Literal-token children.
    children: BTreeMap<String, TrieNode>,
    /// Single-wildcard (`*`) child.
    wildcard_child: Option<Box<TrieNode>>,
    /// Tail-wildcard (`>`) leaf subscribers.
    tail_subscribers: Vec<Subscriber>,
    /// Exact-match leaf subscribers (no further tokens).
    leaf_subscribers: Vec<Subscriber>,
}

impl TrieNode {
    fn is_empty(&self) -> bool {
        self.children.is_empty()
            && self.wildcard_child.is_none()
            && self.tail_subscribers.is_empty()
            && self.leaf_subscribers.is_empty()
    }

    /// Remove subscriber by id from all positions in this node, returning
    /// true if anything was removed.
    fn remove_subscriber(&mut self, id: SubscriptionId) -> bool {
        let mut removed = false;

        let before = self.leaf_subscribers.len();
        self.leaf_subscribers.retain(|sub| sub.id != id);
        if self.leaf_subscribers.len() != before {
            removed = true;
        }

        let before = self.tail_subscribers.len();
        self.tail_subscribers.retain(|sub| sub.id != id);
        if self.tail_subscribers.len() != before {
            removed = true;
        }

        removed
    }
}

/// Result set from a [`Sublist::lookup`] call.
#[derive(Debug, Clone, Default)]
pub struct SublistResult {
    /// All non-queue-group subscribers that match.
    pub subscribers: Vec<SubscriptionId>,
    /// For each queue group, exactly one selected subscriber.
    pub queue_group_picks: Vec<(String, SubscriptionId)>,
}

impl SublistResult {
    /// Return total number of subscriptions that will receive the message.
    #[must_use]
    pub fn total(&self) -> usize {
        self.subscribers.len() + self.queue_group_picks.len()
    }
}

/// Thread-safe trie-based subject routing engine with generation-invalidated
/// caching, queue group support, and cancel-correct subscription guards.
///
/// Inspired by NATS server/sublist.go, adapted for Asupersync's structured
/// concurrency model.
pub struct Sublist {
    /// The core trie protected by an RwLock for concurrent reads.
    trie: RwLock<TrieNode>,
    /// Monotonic generation counter bumped on every mutation.
    generation: AtomicU64,
    /// Next subscription id counter.
    next_id: AtomicU64,
    /// Cache of literal-subject lookups, invalidated by generation changes.
    cache: RwLock<SublistCache>,
    /// Round-robin counter per queue group for deterministic selection.
    queue_round_robin: Mutex<HashMap<String, u64>>,
}

/// Generation-tagged cache entry storing raw matched subscriber info
/// (before queue group selection, which must run fresh each time).
#[derive(Debug, Clone)]
struct CacheEntry {
    generation: u64,
    /// Non-queue-group subscriber ids.
    plain_ids: Vec<SubscriptionId>,
    /// Queue-group subscriber ids grouped by group name.
    queue_groups: Vec<(String, Vec<SubscriptionId>)>,
}

/// Literal-subject lookup cache.
#[derive(Debug, Default)]
struct SublistCache {
    entries: HashMap<String, CacheEntry>,
}

impl Default for Sublist {
    fn default() -> Self {
        Self::new()
    }
}

impl Sublist {
    /// Create an empty routing engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            trie: RwLock::new(TrieNode::default()),
            generation: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            cache: RwLock::new(SublistCache::default()),
            queue_round_robin: Mutex::new(HashMap::new()),
        }
    }

    /// Return the current generation counter.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Insert a subscription for the given pattern, returning a guard that
    /// removes the subscription on drop (cancel-correct).
    pub fn subscribe(
        self: &Arc<Self>,
        pattern: &SubjectPattern,
        queue_group: Option<String>,
    ) -> SubscriptionGuard {
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let subscriber = Subscriber {
            id,
            queue_group: queue_group.clone(),
        };

        {
            let mut trie = self.trie.write();
            insert_into_trie(&mut trie, pattern.segments(), subscriber);
        }

        self.generation.fetch_add(1, Ordering::Release);

        SubscriptionGuard {
            id,
            pattern: pattern.clone(),
            sublist: Arc::clone(self),
        }
    }

    /// Remove a subscription by id and pattern. Called by [`SubscriptionGuard`]
    /// on drop.
    fn unsubscribe(&self, id: SubscriptionId, pattern: &SubjectPattern) {
        let mut trie = self.trie.write();
        remove_from_trie(&mut trie, pattern.segments(), id);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Look up all matching subscriptions for a concrete subject.
    ///
    /// For queue groups, exactly one subscriber per group is selected using
    /// round-robin. Queue group selection always runs fresh (not cached) so
    /// round-robin advances correctly on each call.
    #[must_use]
    pub fn lookup(&self, subject: &Subject) -> SublistResult {
        let current_gen = self.generation.load(Ordering::Acquire);

        // Check cache first (read lock only). Cache stores raw match sets;
        // queue group selection runs fresh each time.
        {
            let cache = self.cache.read();
            if let Some(entry) = cache.entries.get(subject.as_str()) {
                if entry.generation == current_gen {
                    return self
                        .apply_queue_selection(entry.plain_ids.clone(), &entry.queue_groups);
                }
            }
        }

        // Cache miss — walk the trie.
        let trie = self.trie.read();
        let mut raw_matches: Vec<&Subscriber> = Vec::new();
        collect_matches(&trie, subject.tokens(), &mut raw_matches);

        // Split into plain and queue-group buckets.
        let (plain_ids, queue_groups) = Self::split_matches(&raw_matches);

        // Store in cache (generation-tagged).
        {
            let mut cache = self.cache.write();
            let gen_now = self.generation.load(Ordering::Acquire);
            cache.entries.insert(
                subject.as_str().to_owned(),
                CacheEntry {
                    generation: gen_now,
                    plain_ids: plain_ids.clone(),
                    queue_groups: queue_groups.clone(),
                },
            );
        }

        self.apply_queue_selection(plain_ids, &queue_groups)
    }

    /// Return the count of all registered subscriptions.
    #[must_use]
    pub fn count(&self) -> usize {
        let trie = self.trie.read();
        count_subscribers(&trie)
    }

    /// Split raw matches into plain subscriber ids and queue-group buckets.
    fn split_matches(
        raw_matches: &[&Subscriber],
    ) -> (Vec<SubscriptionId>, Vec<(String, Vec<SubscriptionId>)>) {
        let mut plain = Vec::new();
        let mut groups: BTreeMap<String, Vec<SubscriptionId>> = BTreeMap::new();

        for sub in raw_matches {
            if let Some(group) = &sub.queue_group {
                groups.entry(group.clone()).or_default().push(sub.id);
            } else {
                plain.push(sub.id);
            }
        }

        let queue_groups = groups.into_iter().collect();
        (plain, queue_groups)
    }

    /// Apply round-robin queue group selection to produce the final result.
    fn apply_queue_selection(
        &self,
        subscribers: Vec<SubscriptionId>,
        queue_groups: &[(String, Vec<SubscriptionId>)],
    ) -> SublistResult {
        let mut queue_group_picks = Vec::new();
        if !queue_groups.is_empty() {
            let mut rr = self.queue_round_robin.lock();
            for (group, members) in queue_groups {
                if members.is_empty() {
                    continue;
                }
                let counter = rr.entry(group.clone()).or_insert(0);
                let index = (*counter as usize) % members.len();
                queue_group_picks.push((group.clone(), members[index]));
                *counter = counter.wrapping_add(1);
            }
        }

        SublistResult {
            subscribers,
            queue_group_picks,
        }
    }
}

impl fmt::Debug for Sublist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sublist")
            .field("generation", &self.generation.load(Ordering::Relaxed))
            .field("count", &self.count())
            .finish()
    }
}

/// RAII guard that removes the subscription from the [`Sublist`] on drop.
///
/// This ensures cancel-correctness: when a subscriber's scope/task is
/// cancelled, the subscription is automatically cleaned up with no ghost
/// interest remaining.
pub struct SubscriptionGuard {
    id: SubscriptionId,
    pattern: SubjectPattern,
    sublist: Arc<Sublist>,
}

impl SubscriptionGuard {
    /// Return the subscription identifier.
    #[must_use]
    pub fn id(&self) -> SubscriptionId {
        self.id
    }

    /// Return the subscribed pattern.
    #[must_use]
    pub fn pattern(&self) -> &SubjectPattern {
        &self.pattern
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        self.sublist.unsubscribe(self.id, &self.pattern);
    }
}

impl fmt::Debug for SubscriptionGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubscriptionGuard")
            .field("id", &self.id)
            .field("pattern", &self.pattern)
            .finish()
    }
}

// --- Trie operations ---

fn insert_into_trie(node: &mut TrieNode, segments: &[SubjectToken], subscriber: Subscriber) {
    match segments.split_first() {
        None => {
            // End of pattern — register as leaf subscriber.
            node.leaf_subscribers.push(subscriber);
        }
        Some((SubjectToken::Tail, _)) => {
            // Tail wildcard — register as tail subscriber at this node.
            node.tail_subscribers.push(subscriber);
        }
        Some((SubjectToken::One, rest)) => {
            // Single wildcard — descend into wildcard child.
            let child = node
                .wildcard_child
                .get_or_insert_with(|| Box::new(TrieNode::default()));
            insert_into_trie(child, rest, subscriber);
        }
        Some((SubjectToken::Literal(key), rest)) => {
            // Literal token — descend into named child.
            let child = node.children.entry(key.clone()).or_default();
            insert_into_trie(child, rest, subscriber);
        }
    }
}

fn remove_from_trie(node: &mut TrieNode, segments: &[SubjectToken], id: SubscriptionId) -> bool {
    match segments.split_first() {
        None => node.remove_subscriber(id),
        Some((SubjectToken::Tail, _)) => {
            let before = node.tail_subscribers.len();
            node.tail_subscribers.retain(|sub| sub.id != id);
            node.tail_subscribers.len() != before
        }
        Some((SubjectToken::One, rest)) => {
            let Some(child) = node.wildcard_child.as_mut() else {
                return false;
            };
            let removed = remove_from_trie(child, rest, id);
            if child.is_empty() {
                node.wildcard_child = None;
            }
            removed
        }
        Some((SubjectToken::Literal(key), rest)) => {
            let Some(child) = node.children.get_mut(key) else {
                return false;
            };
            let removed = remove_from_trie(child, rest, id);
            if child.is_empty() {
                node.children.remove(key);
            }
            removed
        }
    }
}

fn collect_matches<'a>(
    node: &'a TrieNode,
    subject_tokens: &[String],
    results: &mut Vec<&'a Subscriber>,
) {
    // Tail-wildcard subscribers at this node match any remaining tokens.
    if !subject_tokens.is_empty() {
        results.extend(node.tail_subscribers.iter());
    }

    match subject_tokens.split_first() {
        None => {
            // End of subject — collect leaf subscribers.
            results.extend(node.leaf_subscribers.iter());
        }
        Some((token, rest)) => {
            // Literal child match.
            if let Some(child) = node.children.get(token) {
                collect_matches(child, rest, results);
            }

            // Single-wildcard child match.
            if let Some(child) = node.wildcard_child.as_ref() {
                collect_matches(child, rest, results);
            }
        }
    }
}

fn count_subscribers(node: &TrieNode) -> usize {
    let mut count = node.leaf_subscribers.len() + node.tail_subscribers.len();
    for child in node.children.values() {
        count += count_subscribers(child);
    }
    if let Some(child) = node.wildcard_child.as_ref() {
        count += count_subscribers(child);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(value: &str) -> SubjectToken {
        SubjectToken::Literal(value.to_owned())
    }

    #[test]
    fn parses_valid_subject_patterns() {
        for raw in ["foo.bar.baz", "tenant.orders.*", "sys.>"] {
            let pattern = SubjectPattern::parse(raw).expect("pattern should parse");
            assert_eq!(pattern.as_str(), raw);
        }
    }

    #[test]
    fn rejects_invalid_subject_patterns() {
        assert_eq!(
            SubjectPattern::parse(""),
            Err(SubjectPatternError::EmptyPattern)
        );
        assert_eq!(
            SubjectPattern::parse("foo..bar"),
            Err(SubjectPatternError::EmptySegment)
        );
        assert_eq!(
            SubjectPattern::parse("sys.>.health"),
            Err(SubjectPatternError::TailWildcardMustBeTerminal)
        );
    }

    #[test]
    fn subject_matching_respects_literal_and_wildcard_tokens() {
        let literal = SubjectPattern::parse("tenant.orders.eu").expect("literal pattern");
        let single = SubjectPattern::parse("tenant.orders.*").expect("single wildcard");
        let tail = SubjectPattern::parse("tenant.orders.>").expect("tail wildcard");
        let subject = Subject::parse("tenant.orders.eu").expect("subject");

        assert!(literal.matches(&subject));
        assert!(single.matches(&subject));
        assert!(tail.matches(&subject));
        assert!(
            !SubjectPattern::parse("tenant.payments.*")
                .expect("payments wildcard")
                .matches(&subject)
        );
    }

    #[test]
    fn round_trips_patterns_through_string_and_serde() {
        let pattern = SubjectPattern::parse("tenant.orders.*").expect("pattern");
        let json = serde_json::to_string(&pattern).expect("serialize");
        let decoded: SubjectPattern = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded, pattern);
        assert_eq!(decoded.as_str(), "tenant.orders.*");
    }

    #[test]
    fn trims_outer_whitespace_but_preserves_literal_case() {
        let pattern = SubjectPattern::parse("  $SYS.Health.*  ").expect("pattern");
        let subject = Subject::parse("  Tenant.Orders.EU.123  ").expect("subject");

        assert_eq!(pattern.as_str(), "$SYS.Health.*");
        assert_eq!(subject.as_str(), "Tenant.Orders.EU.123");
    }

    #[test]
    fn subject_rejects_wildcards() {
        assert_eq!(
            Subject::parse("tenant.orders.*"),
            Err(SubjectPatternError::WildcardsNotAllowed(
                "tenant.orders.*".to_owned()
            ))
        );
    }

    #[test]
    fn tail_wildcard_requires_at_least_one_suffix_segment() {
        let wildcard = SubjectPattern::parse("orders.>").expect("wildcard");
        let expanded = Subject::parse("orders.created").expect("expanded");
        let bare_prefix = Subject::parse("orders").expect("bare prefix");

        assert!(wildcard.matches(&expanded));
        assert!(!wildcard.matches(&bare_prefix));
    }

    #[test]
    fn subject_pattern_parsing_matrix_covers_common_and_edge_shapes() {
        let valid_cases = [
            ("tenant", vec![lit("tenant")]),
            ("tenant.orders", vec![lit("tenant"), lit("orders")]),
            (
                "tenant.orders.*",
                vec![lit("tenant"), lit("orders"), SubjectToken::One],
            ),
            (
                "tenant.orders.>",
                vec![lit("tenant"), lit("orders"), SubjectToken::Tail],
            ),
            (
                "$SYS.health.*",
                vec![lit("$SYS"), lit("health"), SubjectToken::One],
            ),
            (
                "sys.audit.>",
                vec![lit("sys"), lit("audit"), SubjectToken::Tail],
            ),
            (
                "tenant.orders.eu.west.1",
                vec![
                    lit("tenant"),
                    lit("orders"),
                    lit("eu"),
                    lit("west"),
                    lit("1"),
                ],
            ),
            ("_INBOX.reply", vec![lit("_INBOX"), lit("reply")]),
            ("Tenant.Orders", vec![lit("Tenant"), lit("Orders")]),
            (
                "  tenant.trimmed.*  ",
                vec![lit("tenant"), lit("trimmed"), SubjectToken::One],
            ),
        ];

        for (raw, expected_segments) in valid_cases {
            let pattern = SubjectPattern::parse(raw).expect("valid pattern should parse");
            assert_eq!(
                pattern.segments(),
                expected_segments.as_slice(),
                "segments mismatch for {raw}"
            );
        }

        let invalid_cases = [
            ("", SubjectPatternError::EmptyPattern),
            ("   ", SubjectPatternError::EmptyPattern),
            (".tenant", SubjectPatternError::EmptySegment),
            ("tenant.", SubjectPatternError::EmptySegment),
            ("tenant..orders", SubjectPatternError::EmptySegment),
            (
                "tenant.order status",
                SubjectPatternError::WhitespaceInSegment("order status".to_owned()),
            ),
            (
                "tenant.>.orders",
                SubjectPatternError::TailWildcardMustBeTerminal,
            ),
            ("tenant.>.>", SubjectPatternError::MultipleTailWildcards),
            (
                "tenant.or*ders",
                SubjectPatternError::EmbeddedWildcard("or*ders".to_owned()),
            ),
            (
                "tenant.or>ders",
                SubjectPatternError::EmbeddedWildcard("or>ders".to_owned()),
            ),
        ];

        for (raw, expected_error) in invalid_cases {
            assert_eq!(
                SubjectPattern::parse(raw),
                Err(expected_error),
                "unexpected parse result for {raw}"
            );
        }
    }

    #[test]
    fn overlap_matrix_covers_literal_single_and_tail_wildcards() {
        let cases = [
            ("tenant.orders.*", "tenant.orders.eu", true),
            ("tenant.orders.*", "tenant.orders.*", true),
            ("tenant.orders.*", "tenant.payments.*", false),
            ("tenant.orders.>", "tenant.orders.*.*", true),
            ("tenant.orders.>", "tenant.payments.>", false),
            ("tenant.*.created", "tenant.orders.*", true),
            ("tenant.*.created", "tenant.orders.cancelled", false),
            ("tenant.orders.*", "tenant.orders.*.*", false),
        ];

        for (left, right, expected) in cases {
            let left = SubjectPattern::parse(left).expect("left pattern");
            let right = SubjectPattern::parse(right).expect("right pattern");
            assert_eq!(
                left.overlaps(&right),
                expected,
                "unexpected overlap result for {} vs {}",
                left,
                right
            );
            assert_eq!(
                right.overlaps(&left),
                expected,
                "unexpected symmetric overlap result for {} vs {}",
                right,
                left
            );
        }
    }

    #[test]
    fn pattern_from_tokens_and_subject_conversion_preserve_canonical_literals() {
        let pattern =
            SubjectPattern::from_tokens(vec![lit("tenant"), SubjectToken::One, lit("reply")])
                .expect("pattern from tokens");
        assert_eq!(pattern.as_str(), "tenant.*.reply");
        assert!(pattern.has_wildcards());
        assert!(!pattern.is_full_wildcard());

        let invalid =
            SubjectPattern::from_tokens(vec![lit("tenant"), SubjectToken::Tail, lit("reply")]);
        assert_eq!(
            invalid,
            Err(SubjectPatternError::TailWildcardMustBeTerminal)
        );

        let subject = Subject::parse("tenant.orders.reply").expect("concrete subject");
        let subject_pattern = SubjectPattern::from(&subject);
        assert_eq!(subject_pattern.as_str(), "tenant.orders.reply");
        assert_eq!(
            subject_pattern.segments(),
            &[lit("tenant"), lit("orders"), lit("reply")]
        );
        assert!(!subject_pattern.has_wildcards());
    }

    // -----------------------------------------------------------------------
    // Sublist routing engine tests
    // -----------------------------------------------------------------------

    fn sublist() -> Arc<Sublist> {
        Arc::new(Sublist::new())
    }

    #[test]
    fn sublist_literal_exact_match() {
        let sl = sublist();
        let pattern = SubjectPattern::new("foo.bar.baz");
        let _guard = sl.subscribe(&pattern, None);

        let hit = Subject::new("foo.bar.baz");
        let miss = Subject::new("foo.bar.qux");

        assert_eq!(sl.lookup(&hit).total(), 1);
        assert_eq!(sl.lookup(&miss).total(), 0);
    }

    #[test]
    fn sublist_single_wildcard_matches_one_token() {
        let sl = sublist();
        let pattern = SubjectPattern::new("foo.*");
        let _guard = sl.subscribe(&pattern, None);

        assert_eq!(sl.lookup(&Subject::new("foo.bar")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("foo.baz")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("foo.bar.baz")).total(), 0);
        assert_eq!(sl.lookup(&Subject::new("qux.bar")).total(), 0);
    }

    #[test]
    fn sublist_tail_wildcard_matches_one_or_more_tokens() {
        let sl = sublist();
        let pattern = SubjectPattern::new("foo.>");
        let _guard = sl.subscribe(&pattern, None);

        assert_eq!(sl.lookup(&Subject::new("foo.bar")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("foo.bar.baz")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("foo.bar.baz.qux")).total(), 1);
        // Tail wildcard requires at least one suffix token.
        assert_eq!(sl.lookup(&Subject::new("foo")).total(), 0);
    }

    #[test]
    fn sublist_combined_wildcards() {
        let sl = sublist();
        let p1 = SubjectPattern::new("foo.*.>");
        let _g1 = sl.subscribe(&p1, None);

        assert_eq!(sl.lookup(&Subject::new("foo.bar.baz")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("foo.qux.a.b.c")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("foo.bar")).total(), 0);
    }

    #[test]
    fn sublist_multiple_subscribers_same_pattern() {
        let sl = sublist();
        let pattern = SubjectPattern::new("orders.created");
        let _g1 = sl.subscribe(&pattern, None);
        let _g2 = sl.subscribe(&pattern, None);

        assert_eq!(
            sl.lookup(&Subject::new("orders.created")).subscribers.len(),
            2
        );
        assert_eq!(sl.count(), 2);
    }

    #[test]
    fn sublist_multiple_patterns_same_subject() {
        let sl = sublist();
        let _g1 = sl.subscribe(&SubjectPattern::new("orders.created"), None);
        let _g2 = sl.subscribe(&SubjectPattern::new("orders.*"), None);
        let _g3 = sl.subscribe(&SubjectPattern::new("orders.>"), None);

        let result = sl.lookup(&Subject::new("orders.created"));
        assert_eq!(result.subscribers.len(), 3);
    }

    #[test]
    fn sublist_drop_guard_removes_subscription() {
        let sl = sublist();
        let pattern = SubjectPattern::new("orders.created");
        let guard = sl.subscribe(&pattern, None);
        assert_eq!(sl.count(), 1);

        drop(guard);
        assert_eq!(sl.count(), 0);
        assert_eq!(sl.lookup(&Subject::new("orders.created")).total(), 0);
    }

    #[test]
    fn sublist_cancel_correctness_no_ghost_interest() {
        let sl = sublist();
        let pattern = SubjectPattern::new("events.>");
        let guard = sl.subscribe(&pattern, None);
        let id = guard.id();

        // Subscriber exists.
        let result = sl.lookup(&Subject::new("events.user.created"));
        assert!(result.subscribers.contains(&id));

        // Drop the guard (simulating cancel/scope exit).
        drop(guard);

        // Subscriber is gone — no ghost interest.
        let result = sl.lookup(&Subject::new("events.user.created"));
        assert!(!result.subscribers.contains(&id));
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn sublist_queue_group_single_delivery() {
        let sl = sublist();
        let pattern = SubjectPattern::new("work.items");
        let _g1 = sl.subscribe(&pattern, Some("workers".to_owned()));
        let _g2 = sl.subscribe(&pattern, Some("workers".to_owned()));
        let _g3 = sl.subscribe(&pattern, Some("workers".to_owned()));

        let result = sl.lookup(&Subject::new("work.items"));
        // No non-queue subscribers.
        assert_eq!(result.subscribers.len(), 0);
        // Exactly one pick for the "workers" group.
        assert_eq!(result.queue_group_picks.len(), 1);
        assert_eq!(result.queue_group_picks[0].0, "workers");
    }

    #[test]
    fn sublist_queue_group_round_robin() {
        let sl = sublist();
        let pattern = SubjectPattern::new("work.items");
        let g1 = sl.subscribe(&pattern, Some("workers".to_owned()));
        let g2 = sl.subscribe(&pattern, Some("workers".to_owned()));

        let subject = Subject::new("work.items");
        let pick1 = sl.lookup(&subject).queue_group_picks[0].1;
        let pick2 = sl.lookup(&subject).queue_group_picks[0].1;

        // Round-robin should alternate between the two subscribers.
        assert_ne!(pick1, pick2);
        assert!(pick1 == g1.id() || pick1 == g2.id());
        assert!(pick2 == g1.id() || pick2 == g2.id());
    }

    #[test]
    fn sublist_multiple_queue_groups() {
        let sl = sublist();
        let pattern = SubjectPattern::new("work.items");
        let _g1 = sl.subscribe(&pattern, Some("group-a".to_owned()));
        let _g2 = sl.subscribe(&pattern, Some("group-a".to_owned()));
        let _g3 = sl.subscribe(&pattern, Some("group-b".to_owned()));
        let _g4 = sl.subscribe(&pattern, None); // Non-queue subscriber.

        let result = sl.lookup(&Subject::new("work.items"));
        assert_eq!(result.subscribers.len(), 1); // Non-queue subscriber.
        assert_eq!(result.queue_group_picks.len(), 2); // One per group.
    }

    #[test]
    fn sublist_queue_group_removal() {
        let sl = sublist();
        let pattern = SubjectPattern::new("work.items");
        let g1 = sl.subscribe(&pattern, Some("workers".to_owned()));
        let g2 = sl.subscribe(&pattern, Some("workers".to_owned()));

        drop(g1);
        let result = sl.lookup(&Subject::new("work.items"));
        assert_eq!(result.queue_group_picks.len(), 1);
        assert_eq!(result.queue_group_picks[0].1, g2.id());
    }

    #[test]
    fn sublist_cache_hit_returns_same_result() {
        let sl = sublist();
        let _guard = sl.subscribe(&SubjectPattern::new("foo.bar"), None);

        let subject = Subject::new("foo.bar");
        let r1 = sl.lookup(&subject);
        let r2 = sl.lookup(&subject);

        assert_eq!(r1.subscribers, r2.subscribers);
    }

    #[test]
    fn sublist_cache_invalidated_on_mutation() {
        let sl = sublist();
        let pattern = SubjectPattern::new("foo.bar");
        let _g1 = sl.subscribe(&pattern, None);

        let subject = Subject::new("foo.bar");
        let gen_before = sl.generation();
        let _g2 = sl.subscribe(&pattern, None);
        let gen_after = sl.generation();

        assert!(gen_after > gen_before);
        // After mutation, lookup should reflect the new state.
        assert_eq!(sl.lookup(&subject).subscribers.len(), 2);
    }

    #[test]
    fn sublist_generation_bumps_on_subscribe_and_unsubscribe() {
        let sl = sublist();
        let gen0 = sl.generation();

        let guard = sl.subscribe(&SubjectPattern::new("test"), None);
        let gen1 = sl.generation();
        assert!(gen1 > gen0);

        drop(guard);
        let gen2 = sl.generation();
        assert!(gen2 > gen1);
    }

    #[test]
    fn sublist_empty_lookup_returns_empty_result() {
        let sl = sublist();
        let result = sl.lookup(&Subject::new("nonexistent.subject"));
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn sublist_single_token_subject() {
        let sl = sublist();
        let _guard = sl.subscribe(&SubjectPattern::new("orders"), None);

        assert_eq!(sl.lookup(&Subject::new("orders")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("payments")).total(), 0);
    }

    #[test]
    fn sublist_deep_nesting() {
        let sl = sublist();
        let deep = "a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t";
        let _guard = sl.subscribe(&SubjectPattern::new(deep), None);
        assert_eq!(sl.lookup(&Subject::new(deep)).total(), 1);
    }

    #[test]
    fn sublist_wildcard_at_various_positions() {
        let sl = sublist();
        let _g1 = sl.subscribe(&SubjectPattern::new("*.bar.baz"), None);
        let _g2 = sl.subscribe(&SubjectPattern::new("foo.*.baz"), None);
        let _g3 = sl.subscribe(&SubjectPattern::new("foo.bar.*"), None);

        let subject = Subject::new("foo.bar.baz");
        assert_eq!(sl.lookup(&subject).subscribers.len(), 3);
    }

    #[test]
    fn sublist_multiple_wildcards_in_pattern() {
        let sl = sublist();
        let _guard = sl.subscribe(&SubjectPattern::new("*.*.*"), None);

        assert_eq!(sl.lookup(&Subject::new("a.b.c")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("a.b")).total(), 0);
        assert_eq!(sl.lookup(&Subject::new("a.b.c.d")).total(), 0);
    }

    #[test]
    fn sublist_tail_wildcard_alone() {
        let sl = sublist();
        // ">" alone is not valid — needs at least one prefix segment.
        // But "foo.>" is valid: matches foo.anything.
        let _guard = sl.subscribe(&SubjectPattern::new("tenant.>"), None);

        assert_eq!(sl.lookup(&Subject::new("tenant.a")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("tenant.a.b")).total(), 1);
        assert_eq!(sl.lookup(&Subject::new("other.a")).total(), 0);
    }

    #[test]
    fn sublist_concurrent_read_access() {
        use std::thread;

        let sl = sublist();
        let _g1 = sl.subscribe(&SubjectPattern::new("orders.*"), None);
        let _g2 = sl.subscribe(&SubjectPattern::new("orders.>"), None);

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let sl_clone = Arc::clone(&sl);
                thread::spawn(move || {
                    for _ in 0..100 {
                        let result = sl_clone.lookup(&Subject::new("orders.created"));
                        assert!(result.total() >= 2);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread panicked");
        }
    }

    #[test]
    fn sublist_concurrent_subscribe_unsubscribe_lookup() {
        use std::thread;

        let sl = sublist();
        let barrier = Arc::new(std::sync::Barrier::new(3));

        let sl1 = Arc::clone(&sl);
        let b1 = Arc::clone(&barrier);
        let writer1 = thread::spawn(move || {
            b1.wait();
            for _ in 0..50 {
                let guard = sl1.subscribe(&SubjectPattern::new("test.subject"), None);
                let _ = sl1.lookup(&Subject::new("test.subject"));
                drop(guard);
            }
        });

        let sl2 = Arc::clone(&sl);
        let b2 = Arc::clone(&barrier);
        let writer2 = thread::spawn(move || {
            b2.wait();
            for _ in 0..50 {
                let guard = sl2.subscribe(&SubjectPattern::new("test.*"), None);
                let _ = sl2.lookup(&Subject::new("test.subject"));
                drop(guard);
            }
        });

        let sl3 = Arc::clone(&sl);
        let b3 = Arc::clone(&barrier);
        let reader = thread::spawn(move || {
            b3.wait();
            for _ in 0..200 {
                let _ = sl3.lookup(&Subject::new("test.subject"));
            }
        });

        writer1.join().expect("writer1");
        writer2.join().expect("writer2");
        reader.join().expect("reader");

        // After all threads complete, no subscriptions should remain.
        assert_eq!(sl.count(), 0);
    }

    #[test]
    fn sublist_subscription_guard_id_is_unique() {
        let sl = sublist();
        let g1 = sl.subscribe(&SubjectPattern::new("a"), None);
        let g2 = sl.subscribe(&SubjectPattern::new("b"), None);
        let g3 = sl.subscribe(&SubjectPattern::new("c"), None);

        assert_ne!(g1.id(), g2.id());
        assert_ne!(g2.id(), g3.id());
        assert_ne!(g1.id(), g3.id());
    }

    #[test]
    fn sublist_count_tracks_subscribe_and_unsubscribe() {
        let sl = sublist();
        assert_eq!(sl.count(), 0);

        let g1 = sl.subscribe(&SubjectPattern::new("a"), None);
        assert_eq!(sl.count(), 1);

        let g2 = sl.subscribe(&SubjectPattern::new("b"), None);
        assert_eq!(sl.count(), 2);

        drop(g1);
        assert_eq!(sl.count(), 1);

        drop(g2);
        assert_eq!(sl.count(), 0);
    }
}
