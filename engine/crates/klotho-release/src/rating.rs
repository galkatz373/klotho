//! Age-rating questionnaires, descriptors, notices, and accessibility evidence.

use std::collections::BTreeMap;

use klotho_core::Hash;
use serde::{Deserialize, Serialize};

use crate::ReleaseError;

/// Rating boards required for a first-title desktop candidate.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RatingBoard {
    /// ESRB.
    Esrb,
    /// PEGI.
    Pegi,
    /// USK.
    Usk,
    /// IARC.
    Iarc,
}

/// Completed questionnaire and capture evidence for one board.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardRecord {
    /// Questionnaire finished.
    pub completed: bool,
    /// Hash of the questionnaire answers.
    pub questionnaire_hash: Hash,
    /// Hash of the capture / screenshot evidence.
    pub capture_hash: Hash,
}

/// Ratings, descriptors, credits, third-party notices, and CVAA-applicable a11y.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatingEvidence {
    /// One record per required board.
    pub boards: BTreeMap<RatingBoard, BoardRecord>,
    /// Content descriptors.
    pub descriptors: Vec<String>,
    /// Credits roll is present in the package.
    pub credits: bool,
    /// Third-party / open-source notices are present.
    pub third_party_notices: bool,
    /// Accessibility evidence including applicable CVAA review.
    pub accessibility: bool,
}

impl RatingEvidence {
    /// Every required board completed, notices present, hashes non-zero.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Scan`] when a board or notice is missing.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        for board in [
            RatingBoard::Esrb,
            RatingBoard::Pegi,
            RatingBoard::Usk,
            RatingBoard::Iarc,
        ] {
            let Some(record) = self.boards.get(&board) else {
                return Err(ReleaseError::scan(format!(
                    "rating board {board:?} missing"
                )));
            };
            if !record.completed
                || record.questionnaire_hash == Hash::ZERO
                || record.capture_hash == Hash::ZERO
            {
                return Err(ReleaseError::scan(format!(
                    "rating board {board:?} incomplete"
                )));
            }
        }
        if self.descriptors.is_empty() {
            return Err(ReleaseError::scan("content descriptors missing"));
        }
        if !self.credits {
            return Err(ReleaseError::scan("credits evidence missing"));
        }
        if !self.third_party_notices {
            return Err(ReleaseError::scan("third-party notices missing"));
        }
        if !self.accessibility {
            return Err(ReleaseError::scan("accessibility rating evidence missing"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_prove::hash_bytes;

    fn complete() -> RatingEvidence {
        let record = BoardRecord {
            completed: true,
            questionnaire_hash: hash_bytes(b"q"),
            capture_hash: hash_bytes(b"c"),
        };
        let mut boards = BTreeMap::new();
        for board in [
            RatingBoard::Esrb,
            RatingBoard::Pegi,
            RatingBoard::Usk,
            RatingBoard::Iarc,
        ] {
            boards.insert(board, record.clone());
        }
        RatingEvidence {
            boards,
            descriptors: vec!["fantasy violence".into()],
            credits: true,
            third_party_notices: true,
            accessibility: true,
        }
    }

    #[test]
    fn complete_ratings_validate() {
        complete().validate().unwrap();
    }

    #[test]
    fn missing_board_fails() {
        let mut ev = complete();
        ev.boards.remove(&RatingBoard::Usk);
        assert!(ev.validate().is_err());
    }
}
