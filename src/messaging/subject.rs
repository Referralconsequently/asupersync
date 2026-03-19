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

#[cfg(test)]
mod tests {
    use super::*;

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
}
