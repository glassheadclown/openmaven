use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::config::Config;
use crate::poller::RedditComment;
use crate::sentiment::{build_provider, CommentInput};
use crate::errors::new_provider_health;
use crate::store::Store;

/// Backfill modes
#[derive(Debug, Clone)]
pub enum BackfillTarget {
    /// Fetch hot/new/top posts and their comments for a subreddit
    Subreddit { name: String, sort: PostSort, limit: u32 },
    /// Fetch a single submission by ID and all its comments
    Submission { id: String },
}

#[derive(Debug, Clone, Copy)]
pub enum PostSort {
    Hot,
    New,
    Top,
    Rising,
}

impl PostSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            PostSort::Hot => "hot",
            PostSort::New => "new",
            PostSort::Top => "top",
            PostSort::Rising => "rising",
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct Post {
    id: Option<String>,
    #[allow(dead_code)]
    title: Option<String>,
    #[allow(dead_code)]
    author: Option<String>,
    #[allow(dead_code)]
    selftext: Option<String>,
    #[allow(dead_code)]
    subreddit: Option<String>,
    #[allow(dead_code)]
    permalink: Option<String>,
    #[allow(dead_code)]
    score: Option<i64>,
    #[allow(dead_code)]
    num_comments: Option<i64>,
    #[allow(dead_code)]
    created_utc: Option<f64>,
    #[allow(dead_code)]
    url: Option<String>,
}

pub struct Backfill {
    config: Config,
    store: Arc<Store>,
    client: reqwest::Client,
}

impl Backfill {
    pub fn new(config: Config, store: Arc<Store>) -> Self {
        Self {
            config,
            store,
            client: reqwest::Client::new(),
        }
    }

    /// Run a backfill job. Returns number of comments analyzed.
    pub async fn run(&self, target: BackfillTarget, keywords: &[String]) -> Result<usize> {
        let provider = build_provider(&self.config.ai, new_provider_health());
        let batch_size = self.config.ai.batch_size;

        let comments = match &target {
            BackfillTarget::Subreddit { name, sort, limit } => {
                self.fetch_subreddit_comments(name, *sort, *limit).await?
            }
            BackfillTarget::Submission { id } => {
                self.fetch_submission_comments(id).await?
            }
        };

        info!("Backfill: fetched {} raw comments", comments.len());

        // Filter
        let filtered: Vec<RedditComment> = comments
            .into_iter()
            .filter(|c| {
                let body = c.body.as_deref().unwrap_or("");
                !body.is_empty()
                    && body != "[deleted]"
                    && body != "[removed]"
                    && keyword_match(body, keywords)
            })
            .collect();

        info!("Backfill: {} comments after filtering", filtered.len());

        let mut total = 0usize;

        for chunk in filtered.chunks(batch_size) {
            let inputs: Vec<CommentInput> = chunk.iter().map(|c| CommentInput {
                id: c.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                subreddit: c.subreddit.clone().unwrap_or_default(),
                author: c.author.clone().unwrap_or_default(),
                body: c.body.clone().unwrap_or_default(),
                link_title: c.link_title.clone().unwrap_or_default(),
                permalink: c.permalink.clone().unwrap_or_default(),
            }).collect();

            match provider.analyze(&inputs).await {
                Ok(results) => {
                    for (input, result) in inputs.iter().zip(results.iter()) {
                        if let Err(e) = self.store.save(
                            &input.subreddit,
                            &input.author,
                            &input.body,
                            &input.link_title,
                            &input.permalink,
                            result,
                        ).await {
                            warn!("Backfill store error: {}", e);
                        }
                        total += 1;
                    }
                }
                Err(e) => {
                    warn!("Backfill sentiment error: {}", e);
                }
            }

            // Small delay between batches to avoid hammering the AI API
            sleep(Duration::from_millis(500)).await;
        }

        Ok(total)
    }

    async fn fetch_subreddit_comments(&self, name: &str, sort: PostSort, limit: u32) -> Result<Vec<RedditComment>> {
        // Step 1: get posts
        let url = format!(
            "{}/v1/reddit/r/{}/{}?limit={}",
            self.config.sylvia.base_url.trim_end_matches('/'),
            name,
            sort.as_str(),
            limit.min(25)
        );

        let resp = self.client
            .get(&url)
            .header("X-API-KEY", &self.config.sylvia.api_key)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let raw: serde_json::Value = resp.json().await?;
        let posts_val = raw
            .get("data").and_then(|d| d.get("posts"))
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let posts: Vec<Post> = serde_json::from_value(serde_json::Value::Array(posts_val))
            .unwrap_or_default();

        info!("Backfill: found {} posts in r/{}", posts.len(), name);

        // Step 2: fetch comments for each post
        let mut all_comments = vec![];
        for post in &posts {
            if let Some(post_id) = &post.id {
                match self.fetch_submission_comments(post_id).await {
                    Ok(mut comments) => {
                        all_comments.append(&mut comments);
                    }
                    Err(e) => {
                        warn!("Failed to fetch comments for post {}: {}", post_id, e);
                    }
                }
                // Rate limit courtesy
                sleep(Duration::from_millis(200)).await;
            }
        }

        Ok(all_comments)
    }

    async fn fetch_submission_comments(&self, submission_id: &str) -> Result<Vec<RedditComment>> {
        let url = format!(
            "{}/v1/reddit/submission/{}/full",
            self.config.sylvia.base_url.trim_end_matches('/'),
            submission_id
        );

        let resp = self.client
            .get(&url)
            .header("X-API-KEY", &self.config.sylvia.api_key)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let raw: serde_json::Value = resp.json().await?;
        let comments_val = raw
            .get("data").and_then(|d| d.get("comments"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        let comments: Vec<RedditComment> = serde_json::from_value(
            serde_json::Value::Array(comments_val)
        ).unwrap_or_default();

        Ok(comments)
    }
}

fn keyword_match(body: &str, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return true;
    }
    let lower = body.to_lowercase();
    keywords.iter().any(|k| lower.contains(&k.to_lowercase()))
}
