use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::{AiConfig, AiProviderType};
use crate::errors::{check_response, log_api_error, SharedProviderHealth};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentResult {
    pub comment_id: String,
    pub score: f32,
    pub label: String,
    pub confidence: f32,
    pub summary: String,
}

#[async_trait]
pub trait SentimentProvider: Send + Sync {
    async fn analyze(&self, comments: &[CommentInput]) -> Result<Vec<SentimentResult>>;
}

#[derive(Debug, Clone)]
pub struct CommentInput {
    pub id: String,
    pub subreddit: String,
    pub author: String,
    pub body: String,
    pub link_title: String,
    pub permalink: String,
}

pub fn build_provider(config: &AiConfig, health: SharedProviderHealth) -> Box<dyn SentimentProvider> {
    Box::new(UnifiedProvider::new(config.clone(), health))
}

const SYSTEM_PROMPT: &str = r#"You are a sentiment analysis engine. You will receive a batch of Reddit comments and return a JSON array.

For each comment return exactly:
{"id":"<id>","score":<-1.0 to 1.0>,"label":"<positive|negative|neutral>","confidence":<0.0 to 1.0>,"summary":"<one sentence>"}

score: -1.0=very negative, 0.0=neutral, 1.0=very positive
Return ONLY a valid JSON array. No markdown. No explanation."#;

fn build_prompt(comments: &[CommentInput]) -> String {
    let items: Vec<serde_json::Value> = comments.iter().map(|c| {
        serde_json::json!({
            "id": c.id,
            "subreddit": c.subreddit,
            "body": c.body.chars().take(200).collect::<String>()
        })
    }).collect();
    format!("Analyze:\n{}", serde_json::to_string(&items).unwrap_or_default())
}

fn parse_results(raw: &str, comments: &[CommentInput]) -> Vec<SentimentResult> {
    let cleaned = raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    match serde_json::from_str::<Vec<serde_json::Value>>(cleaned) {
        Ok(arr) => arr.iter().filter_map(|v| {
            Some(SentimentResult {
                comment_id: v["id"].as_str()?.to_string(),
                score: v["score"].as_f64()? as f32,
                label: v["label"].as_str()?.to_string(),
                confidence: v["confidence"].as_f64()? as f32,
                summary: v["summary"].as_str()?.to_string(),
            })
        }).collect(),
        Err(_) => comments.iter().map(|c| SentimentResult {
            comment_id: c.id.clone(),
            score: 0.0,
            label: "neutral".into(),
            confidence: 0.0,
            summary: "parse error".into(),
        }).collect(),
    }
}

pub struct UnifiedProvider {
    config: AiConfig,
    health: SharedProviderHealth,
    client: reqwest::Client,
}

impl UnifiedProvider {
    pub fn new(config: AiConfig, health: SharedProviderHealth) -> Self {
        Self { config, health, client: reqwest::Client::new() }
    }

    async fn call_api(&self, prompt: &str) -> Result<String> {
        {
            let h = self.health.lock().await;
            if !h.is_available() {
                let msg = h.last_error.clone().unwrap_or_else(|| "Provider temporarily unavailable".into());
                anyhow::bail!("{}", msg);
            }
        }

        let result = match self.config.provider_type {
            AiProviderType::Anthropic => self.call_anthropic(prompt).await,
            AiProviderType::Ollama => self.call_ollama(prompt).await,
            AiProviderType::OpenAI | AiProviderType::Custom => {
                self.call_openai_compat(prompt, &self.config.sentiment_model.clone()).await
            }
        };

        if result.is_ok() {
            self.health.lock().await.record_success();
        }

        result
    }

    async fn call_anthropic(&self, prompt: &str) -> Result<String> {
        let body = serde_json::json!({
            "model": self.config.sentiment_model,
            "max_tokens": 2048,
            "system": SYSTEM_PROMPT,
            "messages": [{"role": "user", "content": prompt}]
        });

        let res = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.config.api_key.as_deref().unwrap_or(""))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();

        if let Some(err) = check_response(status, &text, "Anthropic") {
            let msg = log_api_error(&err, "sentiment");
            self.health.lock().await.record_error(&err);
            anyhow::bail!("{}", msg);
        }

        let data: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        Ok(data["content"][0]["text"].as_str().unwrap_or("[]").to_string())
    }

    async fn call_ollama(&self, prompt: &str) -> Result<String> {
        let base = self.config.base_url.trim_end_matches('/');
        let body = serde_json::json!({
            "model": self.config.sentiment_model,
            "prompt": format!("{}\n\n{}", SYSTEM_PROMPT, prompt),
            "stream": false
        });

        let res = self.client
            .post(format!("{}/api/generate", base))
            .json(&body)
            .send()
            .await?;

        let status = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();

        if let Some(err) = check_response(status, &text, "Ollama") {
            let msg = log_api_error(&err, "sentiment");
            self.health.lock().await.record_error(&err);
            anyhow::bail!("{}", msg);
        }

        let data: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        Ok(data["response"].as_str().unwrap_or("[]").to_string())
    }

    async fn call_openai_compat(&self, prompt: &str, model: &str) -> Result<String> {
        let base = self.config.base_url.trim_end_matches('/').trim_end_matches("/v1");
        let url = format!("{}/v1/chat/completions", base);

        let body = serde_json::json!({
            "model": model,
            "max_tokens": 2048,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": prompt}
            ]
        });

        let res = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key.as_deref().unwrap_or("")))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();

        if let Some(err) = check_response(status, &text, "AI provider") {
            let msg = log_api_error(&err, "sentiment");
            self.health.lock().await.record_error(&err);
            anyhow::bail!("{}", msg);
        }

        let data: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        Ok(data["choices"][0]["message"]["content"].as_str().unwrap_or("[]").to_string())
    }
}

#[async_trait]
impl SentimentProvider for UnifiedProvider {
    async fn analyze(&self, comments: &[CommentInput]) -> Result<Vec<SentimentResult>> {
        let prompt = build_prompt(comments);
        let raw = self.call_api(&prompt).await?;
        Ok(parse_results(&raw, comments))
    }
}
