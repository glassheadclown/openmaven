use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::config::{AnalysisMode, Config, SubredditConfig, SylviaConfig};
use crate::errors::SharedProviderHealth;
use crate::notify::Notifier;
use crate::sentiment::{build_provider, CommentInput, SentimentProvider};
use crate::store::Store;
use crate::trends::TrendTracker;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RedditComment {
    pub id: Option<String>,
    pub author: Option<String>,
    pub body: Option<String>,
    pub subreddit: Option<String>,
    pub created_utc: Option<f64>,
    // Extra fields Sylvia returns that are useful
    pub link_title: Option<String>,
    pub permalink: Option<String>,
    pub score: Option<i64>,
    pub over_18: Option<bool>,
}

pub struct Poller {
    config: Config,
    store: Arc<Store>,
    notifier: Arc<Notifier>,
    provider: Arc<dyn SentimentProvider>,
    tracker: Arc<TrendTracker>,
    health: SharedProviderHealth,
}

impl Poller {
    pub fn new(config: Config, store: Arc<Store>, tracker: Arc<TrendTracker>, health: SharedProviderHealth) -> Self {
        let provider = build_provider(&config.ai, Arc::clone(&health));
        let notifier = Notifier::new(config.notifications.clone());
        Self {
            config,
            store,
            notifier: Arc::new(notifier),
            provider: Arc::from(provider),
            tracker,
            health,
        }
    }

    pub async fn run(self) -> Result<()> {
        let subreddits = self.config.tracking.subreddits.clone();

        if subreddits.is_empty() {
            warn!("No subreddits configured. Exiting.");
            return Ok(());
        }

        info!("Starting poller for {} subreddit(s)", subreddits.len());

        let mut handles = vec![];

        for sub_config in subreddits {
            let sylvia = self.config.sylvia.clone();
            let store = Arc::clone(&self.store);
            let notifier = Arc::clone(&self.notifier);
            let provider = Arc::clone(&self.provider);
            let tracker = Arc::clone(&self.tracker);
            let batch_size = self.config.ai.effective_batch_size();

            let analysis_mode = self.config.analysis.mode.clone();
            let handle = tokio::spawn(async move {
                poll_subreddit(sub_config, sylvia, store, notifier, provider, tracker, batch_size, analysis_mode).await;
            });

            handles.push(handle);
        }

        // Wait for all tasks (they loop forever unless errored)
        for h in handles {
            if let Err(e) = h.await {
                error!("Poller task panicked: {}", e);
            }
        }

        Ok(())
    }
}

async fn poll_subreddit(
    sub: SubredditConfig,
    sylvia: SylviaConfig,
    store: Arc<Store>,
    notifier: Arc<Notifier>,
    provider: Arc<dyn SentimentProvider>,
    tracker: Arc<TrendTracker>,
    batch_size: usize,
    analysis_mode: AnalysisMode,
) {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/v1/reddit/r/{}/comments/live",
        sylvia.base_url.trim_end_matches('/'),
        sub.name
    );

    // Track seen comment IDs to deduplicate across polls
    let seen: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    info!("Polling r/{} every {}s", sub.name, sub.poll_interval_secs);

    loop {
        match fetch_comments(&client, &url, &sylvia.api_key).await {
            Ok(comments) => {
                let mut seen_lock = seen.lock().await;

                // Filter: new, non-empty, keyword match
                let new_comments: Vec<RedditComment> = comments
                    .into_iter()
                    .filter(|c| {
                        let id = c.id.as_deref().unwrap_or("");
                        let body = c.body.as_deref().unwrap_or("");
                        !id.is_empty()
                            && !body.is_empty()
                            && body != "[deleted]"
                            && body != "[removed]"
                            && !seen_lock.contains(id)
                            && keyword_match(body, &sub.keywords)
                    })
                    .collect();

                for c in &new_comments {
                    if let Some(id) = &c.id {
                        seen_lock.insert(id.clone());
                    }
                }
                drop(seen_lock);

                // Trim seen set to avoid unbounded growth
                {
                    let mut seen_lock = seen.lock().await;
                    if seen_lock.len() > 10_000 {
                        seen_lock.clear();
                    }
                }

                if !new_comments.is_empty() {
                    info!("r/{}: {} new comments to analyze", sub.name, new_comments.len());

                    // Batch into chunks for the AI
                    for chunk in new_comments.chunks(batch_size) {
                        let inputs: Vec<CommentInput> = chunk.iter().map(|c| CommentInput {
                            id: c.id.clone().unwrap_or_default(),
                            subreddit: sub.name.clone(),
                            author: c.author.clone().unwrap_or_default(),
                            body: c.body.clone().unwrap_or_default(),
                            link_title: c.link_title.clone().unwrap_or_default(),
                            permalink: c.permalink.clone().unwrap_or_default(),
                        }).collect();

                        match provider.analyze(&inputs).await {
                            Ok(results) => {
                                for (input, result) in inputs.iter().zip(results.iter()) {
                                    // Persist
                                    if let Err(e) = store.save(
                                        &sub.name,
                                        &input.author,
                                        &input.body,
                                        &input.link_title,
                                        &input.permalink,
                                        result,
                                    ).await {
                                        warn!("Failed to save result: {}", e);
                                    }

                                    // Feed trend tracker
                                    let matched_kw = sub.keywords.iter()
                                        .find(|k| input.body.to_lowercase().contains(&k.to_lowercase()))
                                        .cloned()
                                        .unwrap_or_else(|| "__all__".to_string());
                                    tracker.record(&sub.name, &matched_kw, result.score, &result.label).await;

                                    // Alert only in raw or both mode
                                    // Gate on BOTH score AND confidence to kill noise
                                    if analysis_mode != AnalysisMode::Narrative
                                        && result.score <= -sub.sentiment_alert_threshold
                                        && result.confidence >= 0.75
                                    {
                                        if let Err(e) = notifier.alert(&sub.name, result, &input.body).await {
                                            warn!("Notification failed: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Sentiment analysis failed for r/{}: {}", sub.name, e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to fetch r/{}: {}", sub.name, e);
            }
        }

        sleep(Duration::from_secs(sub.poll_interval_secs)).await;
    }
}

async fn fetch_comments(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> Result<Vec<RedditComment>> {
    let response = client
        .get(url)
        .header("X-API-KEY", api_key)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("API returned {}", response.status());
    }

    // Sylvia response envelope:
    // { "success": true, "request_id": "...", "data": { "comments": [...] } }
    let raw: serde_json::Value = response.json().await?;

    // Bail early if the API itself reported failure
    if raw.get("success").and_then(|v| v.as_bool()) == Some(false) {
        anyhow::bail!("Sylvia API returned success=false");
    }

    let comments_val = raw
        .get("data")
        .and_then(|d| d.get("comments"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let comments = serde_json::from_value::<Vec<RedditComment>>(
        serde_json::Value::Array(comments_val)
    )?;

    Ok(comments)
}

fn keyword_match(body: &str, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return true; // no filter = capture all
    }
    let body_lower = body.to_lowercase();
    keywords.iter().any(|kw| body_lower.contains(&kw.to_lowercase()))
}
