use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A single data point in a keyword's trend history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPoint {
    pub timestamp: DateTime<Utc>,
    pub score: f32,
    pub volume: usize,       // number of comments in this window
    pub label_dist: LabelDist,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LabelDist {
    pub positive: usize,
    pub negative: usize,
    pub neutral: usize,
}

/// Rolling window of scores for a single (subreddit, keyword) pair
#[derive(Debug)]
struct KeywordWindow {
    /// Raw score + timestamp entries, kept for the last `window_secs`
    entries: VecDeque<(DateTime<Utc>, f32, String)>, // (time, score, label)
    /// Aggregated snapshots taken every `snapshot_interval_secs`
    snapshots: VecDeque<TrendPoint>,
    window_secs: i64,
    max_snapshots: usize,
}

impl KeywordWindow {
    fn new(window_secs: i64, max_snapshots: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            snapshots: VecDeque::new(),
            window_secs,
            max_snapshots,
        }
    }

    fn push(&mut self, score: f32, label: &str) {
        let now = Utc::now();
        self.entries.push_back((now, score, label.to_string()));
        self.evict_old();
    }

    fn evict_old(&mut self) {
        let cutoff = Utc::now() - chrono::Duration::seconds(self.window_secs);
        while self.entries.front().map(|(t, _, _)| t < &cutoff).unwrap_or(false) {
            self.entries.pop_front();
        }
    }

    fn snapshot(&mut self) {
        self.evict_old();
        if self.entries.is_empty() {
            return;
        }

        let volume = self.entries.len();
        let avg_score = self.entries.iter().map(|(_, s, _)| s).sum::<f32>() / volume as f32;
        let mut dist = LabelDist::default();
        for (_, _, label) in &self.entries {
            match label.as_str() {
                "positive" => dist.positive += 1,
                "negative" => dist.negative += 1,
                _ => dist.neutral += 1,
            }
        }

        let point = TrendPoint {
            timestamp: Utc::now(),
            score: avg_score,
            volume,
            label_dist: dist,
        };

        self.snapshots.push_back(point);
        if self.snapshots.len() > self.max_snapshots {
            self.snapshots.pop_front();
        }
    }

    fn current_score(&self) -> Option<f32> {
        if self.entries.is_empty() {
            return None;
        }
        let sum: f32 = self.entries.iter().map(|(_, s, _)| s).sum();
        Some(sum / self.entries.len() as f32)
    }

    fn trend_direction(&self) -> TrendDirection {
        if self.snapshots.len() < 2 {
            return TrendDirection::Flat;
        }
        let recent: Vec<f32> = self.snapshots.iter().rev().take(3).map(|p| p.score).collect();
        let older: Vec<f32> = self.snapshots.iter().rev().skip(3).take(3).map(|p| p.score).collect();

        if recent.is_empty() || older.is_empty() {
            return TrendDirection::Flat;
        }

        let recent_avg = recent.iter().sum::<f32>() / recent.len() as f32;
        let older_avg = older.iter().sum::<f32>() / older.len() as f32;
        let delta = recent_avg - older_avg;

        if delta > 0.1 { TrendDirection::Up }
        else if delta < -0.1 { TrendDirection::Down }
        else { TrendDirection::Flat }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrendDirection {
    Up,
    Down,
    Flat,
}

impl std::fmt::Display for TrendDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrendDirection::Up => write!(f, "↑"),
            TrendDirection::Down => write!(f, "↓"),
            TrendDirection::Flat => write!(f, "→"),
        }
    }
}

/// The full trend tracker, shared across the poller tasks via Arc<RwLock<>>
pub struct TrendTracker {
    /// key: (subreddit, keyword_or_"__all__")
    windows: RwLock<HashMap<(String, String), KeywordWindow>>,
    window_secs: i64,
    max_snapshots: usize,
}

impl TrendTracker {
    pub fn new(window_secs: i64, max_snapshots: usize) -> Arc<Self> {
        Arc::new(Self {
            windows: RwLock::new(HashMap::new()),
            window_secs,
            max_snapshots,
        })
    }

    /// Record a new sentiment result for a subreddit + matched keyword
    pub async fn record(&self, subreddit: &str, keyword: &str, score: f32, label: &str) {
        let mut windows = self.windows.write().await;
        let key = (subreddit.to_string(), keyword.to_string());
        let window = windows.entry(key).or_insert_with(|| {
            KeywordWindow::new(self.window_secs, self.max_snapshots)
        });
        window.push(score, label);
    }

    /// Take snapshots for all windows — call this on a timer (e.g. every 60s)
    pub async fn snapshot_all(&self) {
        let mut windows = self.windows.write().await;
        for window in windows.values_mut() {
            window.snapshot();
        }
    }

    /// Get current summary for all tracked (subreddit, keyword) pairs
    pub async fn summary(&self) -> Vec<TrendSummary> {
        let windows = self.windows.read().await;
        windows.iter().map(|((sub, kw), w)| TrendSummary {
            subreddit: sub.clone(),
            keyword: kw.clone(),
            current_score: w.current_score(),
            direction: w.trend_direction(),
            volume: w.entries.len(),
            snapshots: w.snapshots.iter().cloned().collect(),
        }).collect()
    }

    /// Get trend for a specific subreddit
    #[allow(dead_code)]
    pub async fn for_subreddit(&self, subreddit: &str) -> Vec<TrendSummary> {
        let all = self.summary().await;
        all.into_iter().filter(|s| s.subreddit == subreddit).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendSummary {
    pub subreddit: String,
    pub keyword: String,
    pub current_score: Option<f32>,
    pub direction: TrendDirection,
    pub volume: usize,
    pub snapshots: Vec<TrendPoint>,
}
