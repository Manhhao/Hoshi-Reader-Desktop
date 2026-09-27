use std::collections::HashMap;

use chrono::NaiveDate;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::library;
use crate::statistics::{self, ReadingSession, Sessions, StatisticsDay};
use crate::sync::model::Timestamped;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TtuStatistics {
    pub date_key: String,
    pub characters_read: i64,
    pub reading_time: f64,
    pub last_statistic_modified: i64,
}

impl TtuStatistics {
    pub fn has_activity(&self) -> bool {
        self.characters_read > 0 || self.reading_time > 0.0
    }

    pub fn merged(statistics: &[TtuStatistics]) -> Vec<TtuStatistics> {
        let mut grouped: HashMap<String, TtuStatistics> = HashMap::new();
        for statistic in statistics {
            if grouped.get(&statistic.date_key).is_some_and(|existing| {
                existing.last_statistic_modified >= statistic.last_statistic_modified
            }) {
                continue;
            }
            grouped.insert(statistic.date_key.clone(), statistic.clone());
        }
        let mut result: Vec<TtuStatistics> = grouped.into_values().collect();
        result.sort_by(|a, b| a.date_key.cmp(&b.date_key));
        result
    }

    pub fn legacy_sessions(statistics: &[TtuStatistics], key: &str) -> Sessions {
        let mut sessions = HashMap::new();
        for statistic in Self::merged(statistics)
            .into_iter()
            .filter(TtuStatistics::has_activity)
        {
            sessions.insert(
                Self::legacy_id(key, &statistic.date_key),
                Timestamped {
                    modified: statistic.last_statistic_modified,
                    value: Some(statistic.session()),
                },
            );
        }
        sessions
    }

    pub fn legacy_id(key: &str, date_key: &str) -> String {
        let key: String = key.nfc().collect();
        let hash = Sha256::digest(format!("{key}\n{date_key}\nlegacy").as_bytes());
        Uuid::from_bytes(hash[..16].try_into().unwrap())
            .to_string()
            .to_uppercase()
    }

    fn session(&self) -> ReadingSession {
        let reset_time = statistics::statistics_reset_time();
        let parts: Vec<i32> = self
            .date_key
            .split('-')
            .map(|part| part.parse().unwrap())
            .collect();
        let date = NaiveDate::from_ymd_opt(parts[0], parts[1] as u32, parts[2] as u32).unwrap();
        let mut start =
            statistics::local_apple(date, (reset_time / 60) as u32, (reset_time % 60) as u32);

        let estimated_end = library::ms_to_apple(self.last_statistic_modified);
        let estimated_start = estimated_end - self.reading_time;
        let day = statistics::local_date(start);
        if StatisticsDay::date(estimated_start, reset_time) == day
            && StatisticsDay::date(estimated_end, reset_time) == day
        {
            start = estimated_start;
        }

        ReadingSession {
            started_at: library::apple_to_ms(start),
            ended_at: library::apple_to_ms(start + self.reading_time),
            characters_read: self.characters_read,
            reading_time: self.reading_time,
        }
    }
}
