use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

use crate::config::db_path;
use crate::sentiment::SentimentResult;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct StoredResult {
    pub id: String,
    pub comment_id: String,
    pub subreddit: String,
    pub author: String,
    pub body: String,
    pub link_title: String,
    pub permalink: String,
    pub score: f32,
    pub label: String,
    pub confidence: f32,
    pub summary: String,
    pub created_at: String,
}

pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn connect() -> Result<Self> {
        let db_path = db_path();
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sentiment_results (
                id TEXT PRIMARY KEY,
                comment_id TEXT NOT NULL,
                subreddit TEXT NOT NULL,
                author TEXT NOT NULL,
                body TEXT NOT NULL,
                link_title TEXT NOT NULL DEFAULT '',
                permalink TEXT NOT NULL DEFAULT '',
                score REAL NOT NULL,
                label TEXT NOT NULL,
                confidence REAL NOT NULL,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_subreddit ON sentiment_results(subreddit)"
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_created_at ON sentiment_results(created_at)"
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub async fn save(&self, subreddit: &str, author: &str, body: &str, link_title: &str, permalink: &str, result: &SentimentResult) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT OR IGNORE INTO sentiment_results
             (id, comment_id, subreddit, author, body, link_title, permalink, score, label, confidence, summary, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(&result.comment_id)
        .bind(subreddit)
        .bind(author)
        .bind(body)
        .bind(link_title)
        .bind(permalink)
        .bind(result.score)
        .bind(&result.label)
        .bind(result.confidence)
        .bind(&result.summary)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn recent(&self, limit: i64) -> Result<Vec<StoredResult>> {
        let results = sqlx::query_as::<_, StoredResult>(
            "SELECT * FROM sentiment_results ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }

    #[allow(dead_code)]
    pub async fn by_subreddit(&self, subreddit: &str, limit: i64) -> Result<Vec<StoredResult>> {
        let results = sqlx::query_as::<_, StoredResult>(
            "SELECT * FROM sentiment_results WHERE subreddit = ? ORDER BY created_at DESC LIMIT ?"
        )
        .bind(subreddit)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }

    pub async fn stats(&self) -> Result<Vec<SubredditStats>> {
        let results = sqlx::query_as::<_, SubredditStats>(
            "SELECT
               subreddit,
               COUNT(*) as total,
               AVG(score) as avg_score,
               SUM(CASE WHEN label = 'positive' THEN 1 ELSE 0 END) as positive_count,
               SUM(CASE WHEN label = 'negative' THEN 1 ELSE 0 END) as negative_count,
               SUM(CASE WHEN label = 'neutral' THEN 1 ELSE 0 END) as neutral_count
             FROM sentiment_results
             GROUP BY subreddit
             ORDER BY total DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SubredditStats {
    pub subreddit: String,
    pub total: i64,
    pub avg_score: f64,
    pub positive_count: i64,
    pub negative_count: i64,
    pub neutral_count: i64,
}

// ── Narrative / RAG memory ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct StoredNarrative {
    pub id: String,
    pub subreddit: String,
    pub narrative: String,
    pub topics: String,
    pub avg_score: f32,
    pub comment_count: i64,
    pub signal_strength: String,
    pub prediction: Option<String>,
    pub prediction_confidence: Option<f32>,
    pub created_at: String,
}

impl Store {
    pub async fn migrate_narratives(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS narratives (
                id TEXT PRIMARY KEY,
                subreddit TEXT NOT NULL,
                narrative TEXT NOT NULL,
                topics TEXT NOT NULL DEFAULT '',
                avg_score REAL NOT NULL,
                comment_count INTEGER NOT NULL,
                signal_strength TEXT NOT NULL DEFAULT 'moderate',
                prediction TEXT,
                prediction_confidence REAL,
                created_at TEXT NOT NULL
            )"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_narratives_sub ON narratives(subreddit)"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_narratives_time ON narratives(created_at)"
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn save_narrative(&self, n: &StoredNarrative) -> Result<()> {
        sqlx::query(
            "INSERT INTO narratives
             (id, subreddit, narrative, topics, avg_score, comment_count, signal_strength, prediction, prediction_confidence, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&n.id)
        .bind(&n.subreddit)
        .bind(&n.narrative)
        .bind(&n.topics)
        .bind(n.avg_score)
        .bind(n.comment_count)
        .bind(&n.signal_strength)
        .bind(&n.prediction)
        .bind(n.prediction_confidence)
        .bind(&n.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch recent narratives for a subreddit for RAG context
    pub async fn recent_narratives(
        &self,
        subreddit: &str,
        lookback_days: u32,
        limit: i64,
    ) -> Result<Vec<StoredNarrative>> {
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::days(lookback_days as i64))
            .to_rfc3339();

        let results = sqlx::query_as::<_, StoredNarrative>(
            "SELECT * FROM narratives
             WHERE subreddit = ? AND created_at > ?
             ORDER BY created_at DESC LIMIT ?"
        )
        .bind(subreddit)
        .bind(&cutoff)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    /// Recent results for a subreddit within a time window (for clustering)
    pub async fn recent_for_subreddit_since(
        &self,
        subreddit: &str,
        since: &str,
        limit: i64,
    ) -> Result<Vec<StoredResult>> {
        let results = sqlx::query_as::<_, StoredResult>(
            "SELECT * FROM sentiment_results
             WHERE subreddit = ? AND created_at > ?
             ORDER BY created_at DESC LIMIT ?"
        )
        .bind(subreddit)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }

    /// All recent narratives across all subreddits (for web dashboard)
    pub async fn all_recent_narratives(&self, limit: i64) -> Result<Vec<StoredNarrative>> {
        let results = sqlx::query_as::<_, StoredNarrative>(
            "SELECT * FROM narratives ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }
}
