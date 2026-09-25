//! Epoch Program enums.
//!
//! Token and CarnageAction enums for internal use.
//! Note: EpochState stores these as u8 to avoid Borsh enum serialization complexity.
//! Source: Epoch_State_Machine_Spec.md Sections 4.2, 4.3

/// Token identifier for CRIME or FRAUD.
///
/// Used internally for type-safe operations.
/// Stored as u8 in EpochState (0 = CRIME, 1 = FRAUD).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Token {
    Crime = 0,
    Fraud = 1,
}

impl Token {
    /// Returns the opposite token.
    pub fn opposite(&self) -> Token {
        match self {
            Token::Crime => Token::Fraud,
            Token::Fraud => Token::Crime,
        }
    }

    /// Convert from u8 representation.
    /// Returns None for invalid values.
    pub fn from_u8(value: u8) -> Option<Token> {
        match value {
            0 => Some(Token::Crime),
            1 => Some(Token::Fraud),
            _ => None,
        }
    }

    /// Convert from u8 representation with fallback.
    /// Returns Crime for 0, Fraud for all other values.
    /// Used when we need a guaranteed Token without Option handling.
    pub fn from_u8_unchecked(value: u8) -> Token {
        match value {
            0 => Token::Crime,
            _ => Token::Fraud,
        }
    }

    /// Convert to u8 representation.
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Carnage action type.
///
/// Determines what happens to held tokens during Carnage.
/// Stored as u8 in EpochState (0 = None, 1 = Burn, 2 = Sell).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CarnageAction {
    /// No action pending
    None = 0,
    /// 98% path: burn held tokens, then buy
    Burn = 1,
    /// 2% path: sell held tokens to SOL, then buy
    Sell = 2,
}

impl CarnageAction {
    /// Convert from u8 representation.
    pub fn from_u8(value: u8) -> Option<CarnageAction> {
        match value {
            0 => Some(CarnageAction::None),
            1 => Some(CarnageAction::Burn),
            2 => Some(CarnageAction::Sell),
            _ => None,
        }
    }

    /// Convert to u8 representation.
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Token enum tests
    // =========================================================================

    #[test]
    fn test_token_to_u8() {
        assert_eq!(Token::Crime.to_u8(), 0);
        assert_eq!(Token::Fraud.to_u8(), 1);
    }

    #[test]
    fn test_token_from_u8() {
        assert_eq!(Token::from_u8(0), Some(Token::Crime));
        assert_eq!(Token::from_u8(1), Some(Token::Fraud));
        assert_eq!(Token::from_u8(2), None);
        assert_eq!(Token::from_u8(255), None);
    }

    #[test]
    fn test_token_from_u8_unchecked() {
        assert_eq!(Token::from_u8_unchecked(0), Token::Crime);
        assert_eq!(Token::from_u8_unchecked(1), Token::Fraud);
        // Any value != 0 falls back to Fraud
        assert_eq!(Token::from_u8_unchecked(2), Token::Fraud);
        assert_eq!(Token::from_u8_unchecked(255), Token::Fraud);
    }

    #[test]
    fn test_token_opposite() {
        assert_eq!(Token::Crime.opposite(), Token::Fraud);
        assert_eq!(Token::Fraud.opposite(), Token::Crime);
    }

    #[test]
    fn test_token_roundtrip() {
        // to_u8 -> from_u8 should return the original token
        assert_eq!(Token::from_u8(Token::Crime.to_u8()), Some(Token::Crime));
        assert_eq!(Token::from_u8(Token::Fraud.to_u8()), Some(Token::Fraud));
    }

    #[test]
    fn test_token_double_opposite() {
        // Opposite of opposite should be the original
        assert_eq!(Token::Crime.opposite().opposite(), Token::Crime);
        assert_eq!(Token::Fraud.opposite().opposite(), Token::Fraud);
    }

    // =========================================================================
    // CarnageAction enum tests
    // =========================================================================

    #[test]
    fn test_carnage_action_to_u8() {
        assert_eq!(CarnageAction::None.to_u8(), 0);
        assert_eq!(CarnageAction::Burn.to_u8(), 1);
        assert_eq!(CarnageAction::Sell.to_u8(), 2);
    }

    #[test]
    fn test_carnage_action_from_u8() {
        assert_eq!(CarnageAction::from_u8(0), Some(CarnageAction::None));
        assert_eq!(CarnageAction::from_u8(1), Some(CarnageAction::Burn));
        assert_eq!(CarnageAction::from_u8(2), Some(CarnageAction::Sell));
        assert_eq!(CarnageAction::from_u8(3), None);
        assert_eq!(CarnageAction::from_u8(255), None);
    }

    #[test]
    fn test_carnage_action_roundtrip() {
        assert_eq!(
            CarnageAction::from_u8(CarnageAction::None.to_u8()),
            Some(CarnageAction::None)
        );
        assert_eq!(
            CarnageAction::from_u8(CarnageAction::Burn.to_u8()),
            Some(CarnageAction::Burn)
        );
        assert_eq!(
            CarnageAction::from_u8(CarnageAction::Sell.to_u8()),
            Some(CarnageAction::Sell)
        );
    }
}
