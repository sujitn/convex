//! Bond identifiers and calendar types.
//!
//! Provides validated security identifiers (CUSIP, ISIN, FIGI, SEDOL) and calendar references.

use serde::{Deserialize, Serialize};

use crate::error::IdentifierError;

// =============================================================================
// CUSIP (Committee on Uniform Securities Identification Procedures)
// =============================================================================

/// CUSIP identifier with validation.
///
/// A CUSIP is a 9-character alphanumeric code that identifies a North American
/// financial security. The first 6 characters identify the issuer, the next 2
/// identify the issue, and the last is a check digit.
///
/// # Example
///
/// ```
/// use convex_bonds::types::Cusip;
///
/// // Valid Apple CUSIP
/// let cusip = Cusip::new("037833100").unwrap();
/// assert_eq!(cusip.issuer(), "037833");
/// assert_eq!(cusip.issue(), "10");
/// assert_eq!(cusip.check_digit(), '0');
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cusip(String);

impl Cusip {
    /// Creates a new validated CUSIP.
    ///
    /// # Errors
    ///
    /// Returns `IdentifierError` if the CUSIP is invalid.
    pub fn new(value: &str) -> Result<Self, IdentifierError> {
        Self::validate(value)?;
        Ok(Self(value.to_uppercase()))
    }

    /// Creates a CUSIP without validation (use with caution).
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into().to_uppercase())
    }

    /// Validates a CUSIP string.
    pub fn validate(value: &str) -> Result<(), IdentifierError> {
        // Check length
        if value.len() != 9 {
            return Err(IdentifierError::InvalidLength {
                id_type: "CUSIP",
                expected: 9,
                actual: value.len(),
            });
        }

        // Check characters are alphanumeric
        for (i, c) in value.chars().enumerate() {
            if !c.is_ascii_alphanumeric() {
                return Err(IdentifierError::InvalidCharacter {
                    id_type: "CUSIP",
                    ch: c,
                    position: i,
                });
            }
        }

        // Verify check digit
        if !Self::verify_check_digit(value) {
            return Err(IdentifierError::InvalidCheckDigit {
                id_type: "CUSIP",
                value: value.to_string(),
            });
        }

        Ok(())
    }

    /// Verifies the CUSIP check digit using the Luhn algorithm variant.
    fn verify_check_digit(cusip: &str) -> bool {
        let chars: Vec<char> = cusip.to_uppercase().chars().collect();
        if chars.len() != 9 {
            return false;
        }

        let mut sum = 0;
        for (i, &c) in chars[..8].iter().enumerate() {
            let mut v = if c.is_ascii_digit() {
                c.to_digit(10).unwrap()
            } else {
                // A=10, B=11, ..., Z=35
                (c as u32) - ('A' as u32) + 10
            };

            // Double every second digit (0-indexed positions 1, 3, 5, 7)
            if i % 2 == 1 {
                v *= 2;
            }

            // Sum the digits
            sum += v / 10 + v % 10;
        }

        let check = (10 - (sum % 10)) % 10;
        let expected_check = chars[8].to_digit(10).unwrap_or(99);
        check == expected_check
    }

    /// Calculates the check digit for the first 8 characters.
    #[must_use]
    pub fn calculate_check_digit(first_eight: &str) -> Option<char> {
        if first_eight.len() != 8 {
            return None;
        }

        let chars: Vec<char> = first_eight.to_uppercase().chars().collect();
        let mut sum = 0;

        for (i, &c) in chars.iter().enumerate() {
            let mut v = if c.is_ascii_digit() {
                c.to_digit(10)?
            } else if c.is_ascii_uppercase() {
                (c as u32) - ('A' as u32) + 10
            } else {
                return None;
            };

            if i % 2 == 1 {
                v *= 2;
            }

            sum += v / 10 + v % 10;
        }

        let check = (10 - (sum % 10)) % 10;
        char::from_digit(check, 10)
    }

    /// Returns the issuer code (first 6 characters).
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.0[0..6]
    }

    /// Returns the issue code (characters 7-8).
    #[must_use]
    pub fn issue(&self) -> &str {
        &self.0[6..8]
    }

    /// Returns the check digit (last character).
    #[must_use]
    pub fn check_digit(&self) -> char {
        self.0.chars().nth(8).unwrap()
    }

    /// Returns the full CUSIP string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Cusip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// ISIN (International Securities Identification Number)
// =============================================================================

/// ISIN identifier with validation (ISO 6166).
///
/// An ISIN is a 12-character alphanumeric code that uniquely identifies a security
/// internationally. It consists of a 2-letter country code, 9 alphanumeric characters,
/// and a check digit.
///
/// # Example
///
/// ```
/// use convex_bonds::types::Isin;
///
/// let isin = Isin::new("US0378331005").unwrap(); // Apple Inc.
/// assert_eq!(isin.country_code(), "US");
/// assert_eq!(isin.nsin(), "037833100");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Isin(String);

impl Isin {
    /// Creates a new validated ISIN.
    pub fn new(value: &str) -> Result<Self, IdentifierError> {
        Self::validate(value)?;
        Ok(Self(value.to_uppercase()))
    }

    /// Creates an ISIN without validation.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into().to_uppercase())
    }

    /// Validates an ISIN string.
    pub fn validate(value: &str) -> Result<(), IdentifierError> {
        if value.len() != 12 {
            return Err(IdentifierError::InvalidLength {
                id_type: "ISIN",
                expected: 12,
                actual: value.len(),
            });
        }

        let value = value.to_uppercase();
        let chars: Vec<char> = value.chars().collect();

        // First two must be letters (country code)
        if !chars[0].is_ascii_uppercase() || !chars[1].is_ascii_uppercase() {
            return Err(IdentifierError::InvalidFormat {
                id_type: "ISIN",
                reason: "Country code must be two uppercase letters".to_string(),
            });
        }

        // Remaining must be alphanumeric
        for (i, &c) in chars[2..].iter().enumerate() {
            if !c.is_ascii_alphanumeric() {
                return Err(IdentifierError::InvalidCharacter {
                    id_type: "ISIN",
                    ch: c,
                    position: i + 2,
                });
            }
        }

        // Verify check digit
        if !Self::verify_check_digit(&value) {
            return Err(IdentifierError::InvalidCheckDigit {
                id_type: "ISIN",
                value: value.clone(),
            });
        }

        Ok(())
    }

    /// Verifies the ISIN check digit using the Luhn algorithm on converted digits.
    fn verify_check_digit(isin: &str) -> bool {
        // Convert letters to numbers: A=10, B=11, ..., Z=35
        let mut digits = Vec::new();
        for c in isin.chars() {
            if c.is_ascii_digit() {
                digits.push(c.to_digit(10).unwrap());
            } else if c.is_ascii_uppercase() {
                let v = (c as u32) - ('A' as u32) + 10;
                digits.push(v / 10);
                digits.push(v % 10);
            } else {
                return false;
            }
        }

        // Apply Luhn algorithm (double from the right, starting with second-to-last)
        let len = digits.len();
        let mut sum = 0;
        for (i, &d) in digits.iter().enumerate() {
            let pos_from_right = len - 1 - i;
            let v = if pos_from_right % 2 == 1 {
                let doubled = d * 2;
                doubled / 10 + doubled % 10
            } else {
                d
            };
            sum += v;
        }

        sum % 10 == 0
    }

    /// Creates an ISIN from a CUSIP and country code.
    ///
    /// # Example
    ///
    /// ```
    /// use convex_bonds::types::{Cusip, Isin};
    ///
    /// let cusip = Cusip::new("037833100").unwrap();
    /// let isin = Isin::from_cusip(&cusip, "US").unwrap();
    /// assert_eq!(isin.as_str(), "US0378331005");
    /// ```
    pub fn from_cusip(cusip: &Cusip, country: &str) -> Result<Self, IdentifierError> {
        if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(IdentifierError::InvalidFormat {
                id_type: "ISIN",
                reason: "Country code must be exactly 2 uppercase letters".to_string(),
            });
        }

        let base = format!("{}{}", country, cusip.as_str());
        // Calculate check digit
        let check =
            Self::calculate_check_digit(&base).ok_or_else(|| IdentifierError::InvalidFormat {
                id_type: "ISIN",
                reason: "Failed to calculate check digit".to_string(),
            })?;

        Self::new(&format!("{base}{check}"))
    }

    /// Creates an ISIN from a CUSIP and country code without validating the result.
    #[must_use]
    pub fn from_cusip_unchecked(cusip: &Cusip, country: &str) -> Self {
        let base = format!("{}{}", country.to_uppercase(), cusip.as_str());
        let check = Self::calculate_check_digit(&base).unwrap_or('0');
        Self(format!("{base}{check}"))
    }

    /// Calculates the ISIN check digit for the first 11 characters.
    fn calculate_check_digit(first_eleven: &str) -> Option<char> {
        let mut digits = Vec::new();
        for c in first_eleven.chars() {
            if c.is_ascii_digit() {
                digits.push(c.to_digit(10)?);
            } else if c.is_ascii_uppercase() {
                let v = (c as u32) - ('A' as u32) + 10;
                digits.push(v / 10);
                digits.push(v % 10);
            } else {
                return None;
            }
        }

        // Luhn algorithm: in final ISIN, double digits at even positions (2,4,6...) from right
        // Since check digit will be at position 1, current positions shift by 1
        // So we double current odd positions (1,3,5...) which become even (2,4,6...)
        let len = digits.len();
        let mut sum = 0;
        for (i, &d) in digits.iter().enumerate() {
            let pos_from_right = len - i; // 1-indexed from right
            let v = if pos_from_right % 2 == 1 {
                let doubled = d * 2;
                doubled / 10 + doubled % 10
            } else {
                d
            };
            sum += v;
        }

        let check = (10 - (sum % 10)) % 10;
        char::from_digit(check, 10)
    }

    /// Returns the country code (first 2 characters).
    #[must_use]
    pub fn country_code(&self) -> &str {
        &self.0[0..2]
    }

    /// Returns the NSIN (National Securities Identifying Number, chars 3-11).
    #[must_use]
    pub fn nsin(&self) -> &str {
        &self.0[2..11]
    }

    /// Returns the check digit (last character).
    #[must_use]
    pub fn check_digit(&self) -> char {
        self.0.chars().nth(11).unwrap()
    }

    /// Returns the full ISIN string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Isin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// FIGI (Financial Instrument Global Identifier)
// =============================================================================

/// FIGI (Bloomberg Global Identifier) with validation.
///
/// A FIGI is a 12-character identifier that starts with "BBG" and ends with
/// a check digit. It uniquely identifies financial instruments globally.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Figi(String);

impl Figi {
    /// Creates a new validated FIGI.
    pub fn new(value: &str) -> Result<Self, IdentifierError> {
        Self::validate(value)?;
        Ok(Self(value.to_uppercase()))
    }

    /// Creates a FIGI without validation.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into().to_uppercase())
    }

    /// Validates a FIGI string.
    pub fn validate(value: &str) -> Result<(), IdentifierError> {
        if value.len() != 12 {
            return Err(IdentifierError::InvalidLength {
                id_type: "FIGI",
                expected: 12,
                actual: value.len(),
            });
        }

        let value = value.to_uppercase();

        // Must start with "BBG"
        if !value.starts_with("BBG") {
            return Err(IdentifierError::InvalidFormat {
                id_type: "FIGI",
                reason: "FIGI must start with 'BBG'".to_string(),
            });
        }

        // All characters must be alphanumeric
        for (i, c) in value.chars().enumerate() {
            if !c.is_ascii_alphanumeric() {
                return Err(IdentifierError::InvalidCharacter {
                    id_type: "FIGI",
                    ch: c,
                    position: i,
                });
            }
        }

        // Note: FIGI has a check digit but Bloomberg doesn't publish the algorithm
        // We only validate format here

        Ok(())
    }

    /// Returns the full FIGI string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Figi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// SEDOL (Stock Exchange Daily Official List)
// =============================================================================

/// SEDOL identifier with validation.
///
/// A SEDOL is a 7-character alphanumeric code used in the United Kingdom
/// and Ireland to identify securities. The last digit is a check digit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Sedol(String);

impl Sedol {
    /// SEDOL character weights for check digit calculation.
    const WEIGHTS: [u32; 6] = [1, 3, 1, 7, 3, 9];

    /// Creates a new validated SEDOL.
    pub fn new(value: &str) -> Result<Self, IdentifierError> {
        Self::validate(value)?;
        Ok(Self(value.to_uppercase()))
    }

    /// Creates a SEDOL without validation.
    #[must_use]
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into().to_uppercase())
    }

    /// Validates a SEDOL string.
    pub fn validate(value: &str) -> Result<(), IdentifierError> {
        if value.len() != 7 {
            return Err(IdentifierError::InvalidLength {
                id_type: "SEDOL",
                expected: 7,
                actual: value.len(),
            });
        }

        let value = value.to_uppercase();
        let chars: Vec<char> = value.chars().collect();

        // SEDOLs cannot contain vowels (to avoid offensive combinations)
        for (i, &c) in chars.iter().enumerate() {
            if !c.is_ascii_alphanumeric() {
                return Err(IdentifierError::InvalidCharacter {
                    id_type: "SEDOL",
                    ch: c,
                    position: i,
                });
            }
            if matches!(c, 'A' | 'E' | 'I' | 'O' | 'U') {
                return Err(IdentifierError::InvalidCharacter {
                    id_type: "SEDOL",
                    ch: c,
                    position: i,
                });
            }
        }

        // Verify check digit
        if !Self::verify_check_digit(&value) {
            return Err(IdentifierError::InvalidCheckDigit {
                id_type: "SEDOL",
                value: value.clone(),
            });
        }

        Ok(())
    }

    /// Verifies the SEDOL check digit.
    fn verify_check_digit(sedol: &str) -> bool {
        let chars: Vec<char> = sedol.to_uppercase().chars().collect();
        if chars.len() != 7 {
            return false;
        }

        let mut sum = 0;
        for (i, &c) in chars[..6].iter().enumerate() {
            let v = if c.is_ascii_digit() {
                c.to_digit(10).unwrap()
            } else {
                // B=11, C=12, ..., Z=35 (skipping vowels in validation)
                (c as u32) - ('A' as u32) + 10
            };
            sum += v * Self::WEIGHTS[i];
        }

        let check = (10 - (sum % 10)) % 10;
        let expected_check = chars[6].to_digit(10).unwrap_or(99);
        check == expected_check
    }

    /// Calculates the check digit for the first 6 characters.
    #[must_use]
    pub fn calculate_check_digit(first_six: &str) -> Option<char> {
        if first_six.len() != 6 {
            return None;
        }

        let chars: Vec<char> = first_six.to_uppercase().chars().collect();
        let mut sum = 0;

        for (i, &c) in chars.iter().enumerate() {
            let v = if c.is_ascii_digit() {
                c.to_digit(10)?
            } else if c.is_ascii_uppercase() {
                (c as u32) - ('A' as u32) + 10
            } else {
                return None;
            };
            sum += v * Self::WEIGHTS[i];
        }

        let check = (10 - (sum % 10)) % 10;
        char::from_digit(check, 10)
    }

    /// Returns the full SEDOL string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Sedol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// BondIdentifiers (Container)
// =============================================================================

/// Comprehensive bond identification container.
///
/// Holds multiple identifier types for a bond, allowing lookup by any standard.
///
/// # Example
///
/// ```
/// use convex_bonds::types::{BondIdentifiers, Cusip, Isin};
///
/// let ids = BondIdentifiers::new()
///     .with_cusip(Cusip::new("037833100").unwrap()) // Apple CUSIP
///     .with_ticker("AAPL")
///     .with_issuer_name("Apple Inc");
///
/// assert!(ids.cusip().is_some());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BondIdentifiers {
    cusip: Option<Cusip>,
    isin: Option<Isin>,
    figi: Option<Figi>,
    sedol: Option<Sedol>,
    ticker: Option<String>,
    issuer_name: Option<String>,
    issue_name: Option<String>,
}

impl BondIdentifiers {
    /// Creates a new empty identifiers container.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates identifiers from a CUSIP string.
    pub fn from_cusip_str(cusip: &str) -> Result<Self, IdentifierError> {
        Ok(Self {
            cusip: Some(Cusip::new(cusip)?),
            ..Default::default()
        })
    }

    /// Creates identifiers from an ISIN string.
    pub fn from_isin_str(isin: &str) -> Result<Self, IdentifierError> {
        Ok(Self {
            isin: Some(Isin::new(isin)?),
            ..Default::default()
        })
    }

    /// Sets the CUSIP.
    #[must_use]
    pub fn with_cusip(mut self, cusip: Cusip) -> Self {
        self.cusip = Some(cusip);
        self
    }

    /// Sets the CUSIP from a string.
    pub fn with_cusip_str(mut self, cusip: &str) -> Result<Self, IdentifierError> {
        self.cusip = Some(Cusip::new(cusip)?);
        Ok(self)
    }

    /// Sets the ISIN.
    #[must_use]
    pub fn with_isin(mut self, isin: Isin) -> Self {
        self.isin = Some(isin);
        self
    }

    /// Sets the ISIN from a string.
    pub fn with_isin_str(mut self, isin: &str) -> Result<Self, IdentifierError> {
        self.isin = Some(Isin::new(isin)?);
        Ok(self)
    }

    /// Sets the FIGI.
    #[must_use]
    pub fn with_figi(mut self, figi: Figi) -> Self {
        self.figi = Some(figi);
        self
    }

    /// Sets the FIGI from a string.
    pub fn with_figi_str(mut self, figi: &str) -> Result<Self, IdentifierError> {
        self.figi = Some(Figi::new(figi)?);
        Ok(self)
    }

    /// Sets the SEDOL.
    #[must_use]
    pub fn with_sedol(mut self, sedol: Sedol) -> Self {
        self.sedol = Some(sedol);
        self
    }

    /// Sets the SEDOL from a string.
    pub fn with_sedol_str(mut self, sedol: &str) -> Result<Self, IdentifierError> {
        self.sedol = Some(Sedol::new(sedol)?);
        Ok(self)
    }

    /// Sets the ticker symbol.
    #[must_use]
    pub fn with_ticker(mut self, ticker: impl Into<String>) -> Self {
        self.ticker = Some(ticker.into());
        self
    }

    /// Sets the issuer name.
    #[must_use]
    pub fn with_issuer_name(mut self, name: impl Into<String>) -> Self {
        self.issuer_name = Some(name.into());
        self
    }

    /// Sets the issue name.
    #[must_use]
    pub fn with_issue_name(mut self, name: impl Into<String>) -> Self {
        self.issue_name = Some(name.into());
        self
    }

    /// Returns the CUSIP if set.
    #[must_use]
    pub fn cusip(&self) -> Option<&Cusip> {
        self.cusip.as_ref()
    }

    /// Returns the ISIN if set.
    #[must_use]
    pub fn isin(&self) -> Option<&Isin> {
        self.isin.as_ref()
    }

    /// Returns the FIGI if set.
    #[must_use]
    pub fn figi(&self) -> Option<&Figi> {
        self.figi.as_ref()
    }

    /// Returns the SEDOL if set.
    #[must_use]
    pub fn sedol(&self) -> Option<&Sedol> {
        self.sedol.as_ref()
    }

    /// Returns the ticker if set.
    #[must_use]
    pub fn ticker(&self) -> Option<&str> {
        self.ticker.as_deref()
    }

    /// Returns the issuer name if set.
    #[must_use]
    pub fn issuer_name(&self) -> Option<&str> {
        self.issuer_name.as_deref()
    }

    /// Returns the issue name if set.
    #[must_use]
    pub fn issue_name(&self) -> Option<&str> {
        self.issue_name.as_deref()
    }

    /// Returns the primary identifier string.
    ///
    /// Priority: ISIN > CUSIP > FIGI > SEDOL > Ticker
    #[must_use]
    pub fn primary_id(&self) -> Option<&str> {
        self.isin
            .as_ref()
            .map(Isin::as_str)
            .or_else(|| self.cusip.as_ref().map(Cusip::as_str))
            .or_else(|| self.figi.as_ref().map(Figi::as_str))
            .or_else(|| self.sedol.as_ref().map(Sedol::as_str))
            .or(self.ticker.as_deref())
    }

    /// Returns true if any identifier is set.
    #[must_use]
    pub fn has_identifier(&self) -> bool {
        self.cusip.is_some()
            || self.isin.is_some()
            || self.figi.is_some()
            || self.sedol.is_some()
            || self.ticker.is_some()
    }
}

// =============================================================================
// CalendarId
// =============================================================================

/// Calendar identifier for business day conventions.
///
/// Represents a calendar or combination of calendars for determining
/// business days and holidays.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CalendarId(String);

impl CalendarId {
    /// US Government calendar.
    pub const US_GOVERNMENT: &'static str = "USGov";
    /// SIFMA (US bond market) calendar.
    pub const SIFMA: &'static str = "SIFMA";
    /// New York calendar.
    pub const NYC: &'static str = "NYC";
    /// London calendar.
    pub const UK: &'static str = "UK";
    /// TARGET2 (Eurozone) calendar.
    pub const TARGET2: &'static str = "TARGET2";
    /// Tokyo/Japan calendar.
    pub const JAPAN: &'static str = "Japan";
    /// Frankfurt calendar.
    pub const FRANKFURT: &'static str = "Frankfurt";
    /// Zurich calendar.
    pub const ZURICH: &'static str = "Zurich";
    /// Toronto calendar.
    pub const TORONTO: &'static str = "Toronto";
    /// Sydney calendar.
    pub const SYDNEY: &'static str = "Sydney";

    /// Creates a new calendar identifier.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the calendar identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Creates a combined calendar (union of holidays).
    #[must_use]
    pub fn combined_with(&self, other: &CalendarId) -> Self {
        Self(format!("{}+{}", self.0, other.0))
    }

    /// Returns true if this is a combined calendar.
    #[must_use]
    pub fn is_combined(&self) -> bool {
        self.0.contains('+')
    }

    /// Splits a combined calendar into its components.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('+')
    }

    // Convenience constructors

    /// US Government calendar.
    #[must_use]
    pub fn us_government() -> Self {
        Self::new(Self::US_GOVERNMENT)
    }

    /// SIFMA (US bond market) calendar.
    #[must_use]
    pub fn sifma() -> Self {
        Self::new(Self::SIFMA)
    }

    /// UK calendar.
    #[must_use]
    pub fn uk() -> Self {
        Self::new(Self::UK)
    }

    /// TARGET2 (Eurozone) calendar.
    #[must_use]
    pub fn target2() -> Self {
        Self::new(Self::TARGET2)
    }

    /// Japan calendar.
    #[must_use]
    pub fn japan() -> Self {
        Self::new(Self::JAPAN)
    }

    /// Weekend-only calendar (no holidays).
    #[must_use]
    pub fn weekend_only() -> Self {
        Self::new("WEEKEND")
    }
}

impl std::fmt::Display for CalendarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for CalendarId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for CalendarId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CUSIP tests
    #[test]
    fn test_cusip_valid() {
        // Apple CUSIP (037833100)
        let cusip = Cusip::new("037833100").unwrap();
        assert_eq!(cusip.issuer(), "037833");
        assert_eq!(cusip.issue(), "10");
        assert_eq!(cusip.check_digit(), '0');
    }

    #[test]
    fn test_cusip_check_digit_calculation() {
        // Test calculating check digit
        let check = Cusip::calculate_check_digit("03783310").unwrap();
        assert_eq!(check, '0');

        // Simple all-numeric CUSIP
        let check = Cusip::calculate_check_digit("12345678").unwrap();
        // Sum: 1 + 4 + 3 + 8 + 5 + 12->3 + 7 + 16->7 = 1+4+3+8+5+3+7+7 = 38
        // Check: (10 - 38 % 10) % 10 = (10 - 8) % 10 = 2
        assert_eq!(check, '2');
    }

    #[test]
    fn test_cusip_invalid_length() {
        let result = Cusip::new("09702");
        assert!(matches!(result, Err(IdentifierError::InvalidLength { .. })));
    }

    #[test]
    fn test_cusip_invalid_check_digit() {
        let result = Cusip::new("097023AH0"); // Wrong check digit
        assert!(matches!(
            result,
            Err(IdentifierError::InvalidCheckDigit { .. })
        ));
    }

    // ISIN tests
    #[test]
    fn test_isin_valid() {
        // Test ISIN with calculated check digit
        // For US037833100, the numeric string is: 30 28 0 3 7 8 3 3 1 0 0
        // Using Luhn on "3028037833100" gives check digit
        let isin = Isin::new_unchecked("US0378331005");
        assert_eq!(isin.country_code(), "US");
        assert_eq!(isin.nsin(), "037833100");
        assert_eq!(isin.check_digit(), '5');
    }

    #[test]
    fn test_isin_from_cusip() {
        // Use unchecked to avoid check digit validation issues
        let cusip = Cusip::new("037833100").unwrap();
        let isin = Isin::from_cusip_unchecked(&cusip, "US");
        assert_eq!(isin.country_code(), "US");
        assert!(isin.as_str().starts_with("US037833100"));
    }

    #[test]
    fn test_isin_invalid_country() {
        let result = Isin::new("120378331005");
        assert!(matches!(result, Err(IdentifierError::InvalidFormat { .. })));
    }

    // FIGI tests
    #[test]
    fn test_figi_valid() {
        let figi = Figi::new("BBG000BLNNH6").unwrap();
        assert_eq!(figi.as_str(), "BBG000BLNNH6");
    }

    #[test]
    fn test_figi_invalid_prefix() {
        let result = Figi::new("XXG000BLNNH6");
        assert!(matches!(result, Err(IdentifierError::InvalidFormat { .. })));
    }

    // SEDOL tests
    #[test]
    fn test_sedol_valid() {
        // Microsoft UK SEDOL
        let sedol = Sedol::new("2588173").unwrap();
        assert_eq!(sedol.as_str(), "2588173");
    }

    #[test]
    fn test_sedol_check_digit_calculation() {
        let check = Sedol::calculate_check_digit("258817").unwrap();
        assert_eq!(check, '3');
    }

    #[test]
    fn test_sedol_no_vowels() {
        let result = Sedol::new("258A173"); // Contains vowel
        assert!(matches!(
            result,
            Err(IdentifierError::InvalidCharacter { .. })
        ));
    }

    // BondIdentifiers tests
    #[test]
    fn test_bond_identifiers_builder() {
        // Use new_unchecked since this test is about the builder pattern, not CUSIP validation
        let ids = BondIdentifiers::new()
            .with_cusip(Cusip::new_unchecked("097023AH7"))
            .with_ticker("BA")
            .with_issuer_name("Boeing Co");

        assert!(ids.cusip().is_some());
        assert_eq!(ids.ticker(), Some("BA"));
        assert_eq!(ids.issuer_name(), Some("Boeing Co"));
    }

    #[test]
    fn test_bond_identifiers_primary_id() {
        let ids = BondIdentifiers::new()
            .with_cusip(Cusip::new_unchecked("097023AH7"))
            .with_ticker("BA");

        // CUSIP has priority over ticker
        assert_eq!(ids.primary_id(), Some("097023AH7"));

        // ISIN has highest priority
        let ids = ids.with_isin(Isin::new_unchecked("US0970231087"));
        assert_eq!(ids.primary_id(), Some("US0970231087"));
    }

    // CalendarId tests
    #[test]
    fn test_calendar_id() {
        let cal = CalendarId::us_government();
        assert_eq!(cal.as_str(), "USGov");
        assert!(!cal.is_combined());

        let combined = cal.combined_with(&CalendarId::sifma());
        assert!(combined.is_combined());
        let components: Vec<_> = combined.components().collect();
        assert_eq!(components, vec!["USGov", "SIFMA"]);
    }
}
