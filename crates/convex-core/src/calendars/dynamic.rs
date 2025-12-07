//! Dynamic holiday calendar for runtime-configurable calendars.
//!
//! This module provides a flexible calendar that can be loaded from JSON,
//! constructed programmatically, or modified at runtime.
//!
//! # Example
//!
//! ```
//! use convex_core::calendars::{DynamicCalendar, WeekendType, Calendar};
//! use convex_core::types::Date;
//!
//! // Create from a list of holiday dates
//! let holidays = vec![
//!     Date::from_ymd(2025, 1, 1).unwrap(),
//!     Date::from_ymd(2025, 12, 25).unwrap(),
//! ];
//! let cal = DynamicCalendar::from_dates("Custom", WeekendType::SaturdaySunday, holidays);
//!
//! assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
//! assert!(cal.is_business_day(Date::from_ymd(2025, 1, 2).unwrap()));
//! ```

use super::bitmap::{HolidayBitmap, WeekendType, MAX_YEAR, MIN_YEAR};
use super::Calendar;
use crate::error::{ConvexError, ConvexResult};
use crate::types::Date;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// A dynamic holiday calendar that can be loaded and modified at runtime.
///
/// Unlike the static calendars (SIFMA, TARGET2, etc.), `DynamicCalendar`
/// allows holidays to be added, removed, and loaded from external sources.
///
/// # Performance
///
/// - `is_business_day()`: O(1), same as static calendars
/// - Memory: ~12KB base + name string
///
/// # Example
///
/// ```
/// use convex_core::calendars::{DynamicCalendar, WeekendType, Calendar};
/// use convex_core::types::Date;
///
/// let mut cal = DynamicCalendar::new("My Calendar", WeekendType::SaturdaySunday);
/// cal.add_holiday(Date::from_ymd(2025, 1, 1).unwrap());
///
/// assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
/// ```
#[derive(Clone)]
pub struct DynamicCalendar {
    /// Name of the calendar
    name: String,
    /// Underlying bitmap for O(1) lookups
    bitmap: HolidayBitmap,
}

impl std::fmt::Debug for DynamicCalendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicCalendar")
            .field("name", &self.name)
            .field("weekend", &self.bitmap.weekend_type())
            .field("holiday_count", &self.bitmap.count_holidays())
            .finish()
    }
}

impl DynamicCalendar {
    /// Create a new empty dynamic calendar.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the calendar
    /// * `weekend` - Weekend type (e.g., SaturdaySunday)
    pub fn new(name: impl Into<String>, weekend: WeekendType) -> Self {
        Self {
            name: name.into(),
            // Use a leaked static string for the bitmap name
            // This is safe because the name is also stored in self.name
            bitmap: HolidayBitmap::new("Dynamic", weekend),
        }
    }

    /// Create a calendar from a list of holiday dates.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the calendar
    /// * `weekend` - Weekend type
    /// * `holidays` - List of holiday dates
    pub fn from_dates(
        name: impl Into<String>,
        weekend: WeekendType,
        holidays: impl IntoIterator<Item = Date>,
    ) -> Self {
        let mut cal = Self::new(name, weekend);
        for date in holidays {
            cal.add_holiday(date);
        }
        cal
    }

    /// Create a calendar from NaiveDate holiday list.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the calendar
    /// * `weekend` - Weekend type
    /// * `holidays` - List of NaiveDate holidays
    pub fn from_naive_dates(
        name: impl Into<String>,
        weekend: WeekendType,
        holidays: impl IntoIterator<Item = NaiveDate>,
    ) -> Self {
        let mut cal = Self::new(name, weekend);
        for date in holidays {
            cal.bitmap.add_holiday(date);
        }
        cal
    }

    /// Load a calendar from JSON data.
    ///
    /// # Arguments
    ///
    /// * `json` - JSON string containing calendar data
    ///
    /// # JSON Format
    ///
    /// ```json
    /// {
    ///   "name": "My Calendar",
    ///   "weekend": "SaturdaySunday",
    ///   "holidays": ["2025-01-01", "2025-12-25"]
    /// }
    /// ```
    pub fn from_json(json: &str) -> ConvexResult<Self> {
        let data: CalendarData =
            serde_json::from_str(json).map_err(|e| ConvexError::CalendarError {
                reason: format!("Failed to parse JSON: {}", e),
            })?;
        Self::from_calendar_data(data)
    }

    /// Load a calendar from a JSON file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to JSON file
    pub fn from_json_file(path: impl AsRef<Path>) -> ConvexResult<Self> {
        let content =
            std::fs::read_to_string(path.as_ref()).map_err(|e| ConvexError::CalendarError {
                reason: format!("Failed to read file: {}", e),
            })?;
        Self::from_json(&content)
    }

    /// Load a calendar from CalendarData struct.
    ///
    /// This is useful when you have already parsed the data from JSON
    /// or constructed it programmatically.
    pub fn from_calendar_data(data: CalendarData) -> ConvexResult<Self> {
        let weekend = data.weekend.unwrap_or_default();
        let mut cal = Self::new(data.name, weekend);

        // Add holidays from date strings
        for date_str in data.holidays {
            let holiday_date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").map_err(|e| {
                ConvexError::CalendarError {
                    reason: format!("Invalid date '{}': {}", date_str, e),
                }
            })?;
            cal.bitmap.add_holiday(holiday_date);
        }

        Ok(cal)
    }

    /// Create a calendar by loading holidays from a function.
    ///
    /// This allows holidays to be loaded from any source (database, API, etc.)
    /// by providing a function that returns the holidays for a year range.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the calendar
    /// * `weekend` - Weekend type
    /// * `start_year` - First year to load
    /// * `end_year` - Last year to load
    /// * `loader` - Function that returns holidays for a given year
    ///
    /// # Example
    ///
    /// ```
    /// use convex_core::calendars::{DynamicCalendar, WeekendType};
    /// use chrono::NaiveDate;
    ///
    /// let cal = DynamicCalendar::from_loader(
    ///     "Database Calendar",
    ///     WeekendType::SaturdaySunday,
    ///     2025,
    ///     2025,
    ///     |year| {
    ///         // In reality, this would query a database
    ///         vec![NaiveDate::from_ymd_opt(year, 1, 1).unwrap()]
    ///     },
    /// );
    /// ```
    pub fn from_loader<F>(
        name: impl Into<String>,
        weekend: WeekendType,
        start_year: i32,
        end_year: i32,
        loader: F,
    ) -> Self
    where
        F: Fn(i32) -> Vec<NaiveDate>,
    {
        let mut cal = Self::new(name, weekend);
        let start = start_year.max(MIN_YEAR);
        let end = end_year.min(MAX_YEAR);

        for year in start..=end {
            for date in loader(year) {
                cal.bitmap.add_holiday(date);
            }
        }
        cal
    }

    /// Get the name of this calendar.
    pub fn calendar_name(&self) -> &str {
        &self.name
    }

    /// Get the weekend type.
    pub fn weekend_type(&self) -> WeekendType {
        self.bitmap.weekend_type()
    }

    /// Add a holiday date.
    pub fn add_holiday(&mut self, date: Date) {
        self.bitmap.add_holiday(date.as_naive_date());
    }

    /// Add a holiday from NaiveDate.
    pub fn add_holiday_naive(&mut self, date: NaiveDate) {
        self.bitmap.add_holiday(date);
    }

    /// Add multiple holidays.
    pub fn add_holidays(&mut self, dates: impl IntoIterator<Item = Date>) {
        for date in dates {
            self.bitmap.add_holiday(date.as_naive_date());
        }
    }

    /// Add multiple holidays from NaiveDates.
    pub fn add_holidays_naive(&mut self, dates: impl IntoIterator<Item = NaiveDate>) {
        for date in dates {
            self.bitmap.add_holiday(date);
        }
    }

    /// Remove a holiday date.
    pub fn remove_holiday(&mut self, date: Date) {
        self.bitmap.remove_holiday(date.as_naive_date());
    }

    /// Remove a holiday from NaiveDate.
    pub fn remove_holiday_naive(&mut self, date: NaiveDate) {
        self.bitmap.remove_holiday(date);
    }

    /// Add holidays for a specific year using a generator function.
    ///
    /// # Arguments
    ///
    /// * `year` - Year to add holidays for
    /// * `generator` - Function that returns holidays for the year
    pub fn add_holidays_for_year<F>(&mut self, year: i32, generator: F)
    where
        F: FnOnce(i32) -> Vec<NaiveDate>,
    {
        for date in generator(year) {
            self.bitmap.add_holiday(date);
        }
    }

    /// Merge holidays from another calendar.
    ///
    /// Adds all holidays from the other calendar to this one.
    pub fn merge(&mut self, other: &DynamicCalendar) {
        // We need to iterate through all possible dates and check if they're holidays
        // This is inefficient but necessary since we can't directly access the other bitmap
        for year in MIN_YEAR..=MAX_YEAR {
            for ordinal in 1..=366 {
                if let Some(date) = NaiveDate::from_yo_opt(year, ordinal) {
                    if other.bitmap.is_holiday(date) {
                        self.bitmap.add_holiday(date);
                    }
                }
            }
        }
    }

    /// Merge holidays from a static calendar that implements Calendar trait.
    pub fn merge_from<C: Calendar>(&mut self, other: &C) {
        for year in MIN_YEAR..=MAX_YEAR {
            for ordinal in 1..=366 {
                if let Some(naive) = NaiveDate::from_yo_opt(year, ordinal) {
                    if let Ok(date) = Date::from_ymd(naive.year(), naive.month(), naive.day()) {
                        // Check if it's a holiday (not weekend, but not business day)
                        let weekday = naive.weekday();
                        let is_weekend = self.bitmap.weekend_type().is_weekend(weekday);
                        if !is_weekend && !other.is_business_day(date) {
                            self.bitmap.add_holiday(naive);
                        }
                    }
                }
            }
        }
    }

    /// Count total holidays in this calendar.
    pub fn holiday_count(&self) -> usize {
        self.bitmap.count_holidays()
    }

    /// Check if a date is a holiday (excluding weekends).
    pub fn is_holiday_date(&self, date: Date) -> bool {
        self.bitmap.is_holiday(date.as_naive_date())
    }

    /// Check if a NaiveDate is a holiday (excluding weekends).
    pub fn is_holiday_naive(&self, date: NaiveDate) -> bool {
        self.bitmap.is_holiday(date)
    }

    /// Export calendar data to a serializable struct.
    pub fn to_calendar_data(&self) -> CalendarData {
        let mut holidays = Vec::new();

        // Collect all holidays
        for year in MIN_YEAR..=MAX_YEAR {
            for ordinal in 1..=366 {
                if let Some(date) = NaiveDate::from_yo_opt(year, ordinal) {
                    if self.bitmap.is_holiday(date) {
                        holidays.push(date.format("%Y-%m-%d").to_string());
                    }
                }
            }
        }

        CalendarData {
            name: self.name.clone(),
            weekend: Some(self.bitmap.weekend_type()),
            holidays,
        }
    }

    /// Export to JSON string.
    pub fn to_json(&self) -> ConvexResult<String> {
        let data = self.to_calendar_data();
        serde_json::to_string_pretty(&data).map_err(|e| ConvexError::CalendarError {
            reason: format!("Failed to serialize calendar: {}", e),
        })
    }

    /// Export to JSON file.
    pub fn to_json_file(&self, path: impl AsRef<Path>) -> ConvexResult<()> {
        let json = self.to_json()?;
        std::fs::write(path.as_ref(), json).map_err(|e| ConvexError::CalendarError {
            reason: format!("Failed to write file: {}", e),
        })
    }
}

impl Calendar for DynamicCalendar {
    fn name(&self) -> &'static str {
        // Return a static string since the trait requires it
        // The actual name is stored in self.name
        "Dynamic"
    }

    fn is_business_day(&self, date: Date) -> bool {
        self.bitmap.is_business_day(date.as_naive_date())
    }
}

/// Calendar data structure for JSON serialization.
///
/// This struct represents the JSON format for loading/saving calendars.
///
/// # JSON Format
///
/// ```json
/// {
///   "name": "My Calendar",
///   "weekend": "SaturdaySunday",
///   "holidays": ["2025-01-01", "2025-12-25"]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarData {
    /// Name of the calendar
    pub name: String,

    /// Weekend type (optional, defaults to SaturdaySunday)
    #[serde(default)]
    pub weekend: Option<WeekendType>,

    /// List of holiday dates in YYYY-MM-DD format
    #[serde(default)]
    pub holidays: Vec<String>,
}

impl CalendarData {
    /// Create a new CalendarData.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            weekend: None,
            holidays: Vec::new(),
        }
    }

    /// Set the weekend type.
    pub fn with_weekend(mut self, weekend: WeekendType) -> Self {
        self.weekend = Some(weekend);
        self
    }

    /// Add a holiday date string.
    pub fn with_holiday(mut self, date: &str) -> Self {
        self.holidays.push(date.to_string());
        self
    }

    /// Add multiple holiday date strings.
    pub fn with_holidays(mut self, dates: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.holidays.extend(dates.into_iter().map(|s| s.into()));
        self
    }

    /// Build into a DynamicCalendar.
    pub fn build(self) -> ConvexResult<DynamicCalendar> {
        DynamicCalendar::from_calendar_data(self)
    }
}

// Implement serde for WeekendType
impl Serialize for WeekendType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            WeekendType::SaturdaySunday => "SaturdaySunday",
            WeekendType::FridaySaturday => "FridaySaturday",
            WeekendType::ThursdayFriday => "ThursdayFriday",
            WeekendType::SundayOnly => "SundayOnly",
            WeekendType::None => "None",
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for WeekendType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "SaturdaySunday" | "saturday_sunday" => Ok(WeekendType::SaturdaySunday),
            "FridaySaturday" | "friday_saturday" => Ok(WeekendType::FridaySaturday),
            "ThursdayFriday" | "thursday_friday" => Ok(WeekendType::ThursdayFriday),
            "SundayOnly" | "sunday_only" => Ok(WeekendType::SundayOnly),
            "None" | "none" => Ok(WeekendType::None),
            _ => Err(serde::de::Error::custom(format!(
                "Unknown weekend type: {}",
                s
            ))),
        }
    }
}

/// Builder for creating custom calendars with flexible configuration.
///
/// # Example
///
/// ```
/// use convex_core::calendars::{CustomCalendarBuilder, WeekendType, Calendar};
/// use convex_core::types::Date;
/// use chrono::NaiveDate;
///
/// let cal = CustomCalendarBuilder::new("Trading Calendar")
///     .weekend(WeekendType::SaturdaySunday)
///     .add_fixed_holiday(1, 1)  // New Year's Day
///     .add_fixed_holiday(12, 25)  // Christmas
///     .add_custom(|year| {
///         // Add company-specific holidays
///         vec![NaiveDate::from_ymd_opt(year, 7, 4).unwrap()]
///     })
///     .build();
///
/// assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
/// ```
pub struct CustomCalendarBuilder {
    name: String,
    weekend: WeekendType,
    start_year: i32,
    end_year: i32,
    holidays: HashSet<NaiveDate>,
    generators: Vec<Box<dyn Fn(i32) -> Vec<NaiveDate>>>,
}

impl CustomCalendarBuilder {
    /// Create a new custom calendar builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            weekend: WeekendType::SaturdaySunday,
            start_year: MIN_YEAR,
            end_year: MAX_YEAR,
            holidays: HashSet::new(),
            generators: Vec::new(),
        }
    }

    /// Set the weekend type.
    pub fn weekend(mut self, weekend: WeekendType) -> Self {
        self.weekend = weekend;
        self
    }

    /// Set the year range for holiday generation.
    pub fn year_range(mut self, start: i32, end: i32) -> Self {
        self.start_year = start.max(MIN_YEAR);
        self.end_year = end.min(MAX_YEAR);
        self
    }

    /// Add a specific holiday date.
    pub fn add_date(mut self, date: NaiveDate) -> Self {
        self.holidays.insert(date);
        self
    }

    /// Add multiple specific holiday dates.
    pub fn add_dates(mut self, dates: impl IntoIterator<Item = NaiveDate>) -> Self {
        self.holidays.extend(dates);
        self
    }

    /// Add a fixed holiday (same date every year).
    pub fn add_fixed_holiday(mut self, month: u32, day: u32) -> Self {
        for year in self.start_year..=self.end_year {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                self.holidays.insert(date);
            }
        }
        self
    }

    /// Add a fixed holiday with weekend observation.
    pub fn add_fixed_holiday_observed(mut self, month: u32, day: u32) -> Self {
        for year in self.start_year..=self.end_year {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                self.holidays.insert(super::bitmap::observed_date(date));
            }
        }
        self
    }

    /// Add a fixed holiday starting from a specific year.
    pub fn add_fixed_holiday_from(mut self, month: u32, day: u32, from_year: i32) -> Self {
        for year in from_year.max(self.start_year)..=self.end_year {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                self.holidays.insert(date);
            }
        }
        self
    }

    /// Add nth weekday of month holiday (e.g., 3rd Monday).
    pub fn add_nth_weekday(
        mut self,
        month: u32,
        weekday: chrono::Weekday,
        occurrence: u32,
    ) -> Self {
        for year in self.start_year..=self.end_year {
            if let Some(date) =
                super::bitmap::nth_weekday_of_month(year, month, weekday, occurrence)
            {
                self.holidays.insert(date);
            }
        }
        self
    }

    /// Add last weekday of month holiday.
    pub fn add_last_weekday(mut self, month: u32, weekday: chrono::Weekday) -> Self {
        for year in self.start_year..=self.end_year {
            if let Some(date) = super::bitmap::last_weekday_of_month(year, month, weekday) {
                self.holidays.insert(date);
            }
        }
        self
    }

    /// Add Easter-based holiday (offset from Easter Sunday).
    pub fn add_easter_offset(mut self, offset_days: i32) -> Self {
        for year in self.start_year..=self.end_year {
            if let Some(easter) = super::bitmap::easter_sunday(year) {
                if let Some(date) =
                    easter.checked_add_signed(chrono::Duration::days(offset_days as i64))
                {
                    self.holidays.insert(date);
                }
            }
        }
        self
    }

    /// Add Good Friday.
    pub fn add_good_friday(self) -> Self {
        self.add_easter_offset(-2)
    }

    /// Add Easter Monday.
    pub fn add_easter_monday(self) -> Self {
        self.add_easter_offset(1)
    }

    /// Add a custom holiday generator function.
    ///
    /// The function is called for each year in the range and should return
    /// a list of holiday dates for that year.
    pub fn add_custom<F>(mut self, generator: F) -> Self
    where
        F: Fn(i32) -> Vec<NaiveDate> + 'static,
    {
        self.generators.push(Box::new(generator));
        self
    }

    /// Build the dynamic calendar.
    pub fn build(mut self) -> DynamicCalendar {
        // Run all generators
        for generator in &self.generators {
            for year in self.start_year..=self.end_year {
                self.holidays.extend(generator(year));
            }
        }

        DynamicCalendar::from_naive_dates(self.name, self.weekend, self.holidays)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_calendar_new() {
        let cal = DynamicCalendar::new("Test", WeekendType::SaturdaySunday);
        assert_eq!(cal.calendar_name(), "Test");
        assert_eq!(cal.holiday_count(), 0);
    }

    #[test]
    fn test_dynamic_calendar_add_holiday() {
        let mut cal = DynamicCalendar::new("Test", WeekendType::SaturdaySunday);

        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        cal.add_holiday(new_years);

        assert!(!cal.is_business_day(new_years));
        assert_eq!(cal.holiday_count(), 1);
    }

    #[test]
    fn test_dynamic_calendar_from_dates() {
        let holidays = vec![
            Date::from_ymd(2025, 1, 1).unwrap(),
            Date::from_ymd(2025, 12, 25).unwrap(),
        ];
        let cal = DynamicCalendar::from_dates("Test", WeekendType::SaturdaySunday, holidays);

        assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
        assert!(!cal.is_business_day(Date::from_ymd(2025, 12, 25).unwrap()));
        assert!(cal.is_business_day(Date::from_ymd(2025, 1, 2).unwrap()));
        assert_eq!(cal.holiday_count(), 2);
    }

    #[test]
    fn test_dynamic_calendar_weekend() {
        let cal = DynamicCalendar::new("Test", WeekendType::SaturdaySunday);

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        assert!(!cal.is_business_day(saturday));
        assert!(!cal.is_business_day(sunday));
        assert!(cal.is_business_day(monday));
    }

    #[test]
    fn test_dynamic_calendar_from_json() {
        let json = r#"{
            "name": "Test Calendar",
            "weekend": "SaturdaySunday",
            "holidays": ["2025-01-01", "2025-12-25"]
        }"#;

        let cal = DynamicCalendar::from_json(json).unwrap();

        assert_eq!(cal.calendar_name(), "Test Calendar");
        assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
        assert!(!cal.is_business_day(Date::from_ymd(2025, 12, 25).unwrap()));
    }

    #[test]
    fn test_dynamic_calendar_from_json_minimal() {
        let json = r#"{
            "name": "Minimal",
            "holidays": []
        }"#;

        let cal = DynamicCalendar::from_json(json).unwrap();
        assert_eq!(cal.calendar_name(), "Minimal");
        assert_eq!(cal.weekend_type(), WeekendType::SaturdaySunday);
    }

    #[test]
    fn test_dynamic_calendar_to_json() {
        let holidays = vec![Date::from_ymd(2025, 1, 1).unwrap()];
        let cal = DynamicCalendar::from_dates("Test", WeekendType::SaturdaySunday, holidays);

        let json = cal.to_json().unwrap();
        assert!(json.contains("\"name\": \"Test\""));
        assert!(json.contains("2025-01-01"));
    }

    #[test]
    fn test_dynamic_calendar_from_loader() {
        let cal = DynamicCalendar::from_loader(
            "Loader Test",
            WeekendType::SaturdaySunday,
            2025,
            2025,
            |year| vec![NaiveDate::from_ymd_opt(year, 1, 1).unwrap()],
        );

        assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
        assert_eq!(cal.holiday_count(), 1);
    }

    #[test]
    fn test_dynamic_calendar_remove_holiday() {
        let mut cal = DynamicCalendar::new("Test", WeekendType::SaturdaySunday);

        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        cal.add_holiday(new_years);
        assert!(!cal.is_business_day(new_years));

        cal.remove_holiday(new_years);
        assert!(cal.is_business_day(new_years));
    }

    #[test]
    fn test_calendar_data_builder() {
        let data = CalendarData::new("Builder Test")
            .with_weekend(WeekendType::FridaySaturday)
            .with_holiday("2025-01-01")
            .with_holidays(vec!["2025-12-25", "2025-12-26"]);

        let cal = data.build().unwrap();
        assert_eq!(cal.calendar_name(), "Builder Test");
        assert_eq!(cal.weekend_type(), WeekendType::FridaySaturday);
        assert_eq!(cal.holiday_count(), 3);
    }

    #[test]
    fn test_custom_calendar_builder() {
        let cal = CustomCalendarBuilder::new("Custom")
            .weekend(WeekendType::SaturdaySunday)
            .year_range(2025, 2025)
            .add_fixed_holiday(1, 1) // New Year's
            .add_fixed_holiday(12, 25) // Christmas
            .add_nth_weekday(1, chrono::Weekday::Mon, 3) // MLK Day
            .build();

        assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 1).unwrap()));
        assert!(!cal.is_business_day(Date::from_ymd(2025, 12, 25).unwrap()));
        assert!(!cal.is_business_day(Date::from_ymd(2025, 1, 20).unwrap())); // MLK Day
    }

    #[test]
    fn test_custom_calendar_with_easter() {
        let cal = CustomCalendarBuilder::new("Easter Test")
            .year_range(2025, 2025)
            .add_good_friday()
            .add_easter_monday()
            .build();

        // 2025: Easter is April 20, Good Friday is April 18, Easter Monday is April 21
        assert!(!cal.is_business_day(Date::from_ymd(2025, 4, 18).unwrap()));
        assert!(!cal.is_business_day(Date::from_ymd(2025, 4, 21).unwrap()));
    }

    #[test]
    fn test_custom_calendar_with_custom_generator() {
        let cal = CustomCalendarBuilder::new("Generator Test")
            .year_range(2025, 2025)
            .add_custom(|year| {
                // Company anniversary on March 15
                vec![NaiveDate::from_ymd_opt(year, 3, 15).unwrap()]
            })
            .build();

        assert!(!cal.is_business_day(Date::from_ymd(2025, 3, 15).unwrap()));
    }

    #[test]
    fn test_weekend_type_serde() {
        let json = r#""SaturdaySunday""#;
        let weekend: WeekendType = serde_json::from_str(json).unwrap();
        assert_eq!(weekend, WeekendType::SaturdaySunday);

        let json = r#""FridaySaturday""#;
        let weekend: WeekendType = serde_json::from_str(json).unwrap();
        assert_eq!(weekend, WeekendType::FridaySaturday);

        // Test snake_case alternative
        let json = r#""friday_saturday""#;
        let weekend: WeekendType = serde_json::from_str(json).unwrap();
        assert_eq!(weekend, WeekendType::FridaySaturday);
    }

    #[test]
    fn test_calendar_data_serde() {
        let data = CalendarData {
            name: "Test".to_string(),
            weekend: Some(WeekendType::SaturdaySunday),
            holidays: vec!["2025-01-01".to_string()],
        };

        let json = serde_json::to_string(&data).unwrap();
        let parsed: CalendarData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.weekend, Some(WeekendType::SaturdaySunday));
        assert_eq!(parsed.holidays.len(), 1);
    }
}
