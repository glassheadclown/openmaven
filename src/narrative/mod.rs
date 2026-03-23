use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::{AnalysisConfig, AnalysisMode, AiConfig, AiProviderType, SubredditConfig};
use crate::errors::{check_response, log_api_error, SharedProviderHealth};
use crate::notify::Notifier;
use crate::store::{Store, StoredNarrative, StoredResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeReport {
    pub subreddit: String,
    pub narrative: String,
    pub topics: Vec<String>,
    pub avg_score: f32,
    pub comment_count: usize,
    pub direction: String,
    pub signal_strength: String, // weak | moderate | strong | critical
    pub prediction: Option<Prediction>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub text: String,
    pub confidence: f32,
    pub timeframe: String,
}

fn narrative_prompt(subreddit: &str, comments: &[StoredResult], rag_context: &str) -> String {
    let comment_block: Vec<String> = comments.iter().map(|c| {
        format!(
            "[score:{:+.2}|{}] {}",
            c.score,
            c.label,
            c.body.chars().take(150).collect::<String>().replace('\n', " ")
        )
    }).collect();

    let rag_section = if rag_context.is_empty() {
        String::new()
    } else {
        format!("\n\n## Historical context (past signals from this subreddit):\n{}", rag_context)
    };

    format!(
        r#"You are an intelligence analyst extracting real-world signals from Reddit sentiment data for r/{subreddit}.

## Sentiment data ({count} comments, avg score: {avg:.2}):
{comments}{rag}

Your job is NOT to summarize what people said. Your job is to extract what this sentiment DATA SIGNALS about real-world events, trends, and public opinion trajectory.

Write like a financial/political analyst briefing, not a content moderator report.

Respond ONLY with this JSON object:
{{
  "narrative": "<2-3 sentences of intelligence-style analysis. Example format: 'Public sentiment on [topic] is deteriorating sharply, suggesting [real-world implication]. The data indicates [underlying cause or event]. This trajectory points toward [likely outcome or shift].' Name specific events, figures, geopolitical situations. Never say 'commenters said' or 'users expressed'. Speak about the signal itself.>",
  "topics": ["<specific topic or entity>", "<topic2>", "<topic3>"],
  "direction": "<rising_negative|rising_positive|stable|mixed>",
  "signal_strength": "<weak|moderate|strong|critical>",
  "why": "<one sentence: the specific real-world event or development driving this sentiment shift>"
}}"#,
        subreddit = subreddit,
        count = comments.len(),
        avg = comments.iter().map(|c| c.score as f64).sum::<f64>() / comments.len().max(1) as f64,
        comments = comment_block.join("\n"),
        rag = rag_section,
    )
}

fn prediction_prompt(subreddit: &str, report: &NarrativeReport, rag_context: &str) -> String {
    let rag_section = if rag_context.is_empty() {
        String::new()
    } else {
        format!("\n\n## Historical signal patterns from this subreddit:\n{}", rag_context)
    };

    format!(
        r#"You are a geopolitical and market intelligence analyst. Based on the sentiment signal below, generate a forward-looking assessment.

## Current signal — r/{subreddit}:
{narrative}
Topics: {topics}
Direction: {direction} | Avg score: {score:+.2}{rag}

Generate a prediction about REAL-WORLD outcomes — not about what Reddit will say next.
Think: policy decisions, market moves, geopolitical developments, institutional responses, public behavior shifts.

Respond ONLY with this JSON:
{{
  "prediction": "<2-3 sentences. Predict real-world outcomes, not social media activity. Example: 'Sustained negative sentiment around [topic] at this velocity historically precedes [outcome]. Given the current trajectory, [specific development] is likely within [timeframe]. Watch for [leading indicator].' Be specific and falsifiable.>",
  "confidence": <0.0-1.0>,
  "timeframe": "<e.g. 'next 2-4 hours' or 'next 24-48 hours'>",
  "reasoning": "<one sentence: why this signal predicts this outcome>"
}}"#,
        subreddit = subreddit,
        narrative = report.narrative,
        topics = report.topics.join(", "),
        direction = report.direction,
        score = report.avg_score,
        rag = rag_section,
    )
}

fn parse_json(raw: &str) -> serde_json::Value {
    let cleaned = raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(cleaned).unwrap_or_default()
}

async fn ai_call(
    config: &AiConfig,
    prompt: &str,
    use_narrative_model: bool,
    health: &SharedProviderHealth,
) -> Result<String> {
    {
        let h = health.lock().await;
        if !h.is_available() {
            anyhow::bail!("{}", h.last_error.clone().unwrap_or_else(|| "Provider unavailable".into()));
        }
    }

    let client = reqwest::Client::new();
    let model = if use_narrative_model {
        &config.narrative_model
    } else {
        &config.sentiment_model
    };

    let result = match config.provider_type {
        AiProviderType::Anthropic => {
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": prompt}]
            });
            let res = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", config.api_key.as_deref().unwrap_or(""))
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await?;
            let status = res.status().as_u16();
            let text = res.text().await.unwrap_or_default();
            if let Some(err) = check_response(status, &text, "Anthropic") {
                health.lock().await.record_error(&err);
                anyhow::bail!("{}", log_api_error(&err, "narrative"));
            }
            health.lock().await.record_success();
            let data: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            data["content"][0]["text"].as_str().unwrap_or("{}").to_string()
        }

        AiProviderType::Ollama => {
            let base = config.base_url.trim_end_matches('/');
            let body = serde_json::json!({
                "model": model,
                "prompt": prompt,
                "stream": false
            });
            let res = client
                .post(format!("{}/api/generate", base))
                .json(&body)
                .send()
                .await?;
            let status = res.status().as_u16();
            let text = res.text().await.unwrap_or_default();
            if let Some(err) = check_response(status, &text, "Ollama") {
                health.lock().await.record_error(&err);
                anyhow::bail!("{}", log_api_error(&err, "narrative"));
            }
            health.lock().await.record_success();
            let data: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            data["response"].as_str().unwrap_or("{}").to_string()
        }

        AiProviderType::OpenAI | AiProviderType::Custom => {
            let base = config.base_url.trim_end_matches('/').trim_end_matches("/v1");
            let url = format!("{}/v1/chat/completions", base);
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 1024,
                "messages": [
                    {"role": "system", "content": "You are a precise analytical engine. Respond with valid JSON only. No markdown."},
                    {"role": "user", "content": prompt}
                ]
            });
            let res = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", config.api_key.as_deref().unwrap_or("")))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;
            let status = res.status().as_u16();
            let text = res.text().await.unwrap_or_default();
            if let Some(err) = check_response(status, &text, "AI provider") {
                health.lock().await.record_error(&err);
                anyhow::bail!("{}", log_api_error(&err, "narrative"));
            }
            health.lock().await.record_success();
            let data: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            data["choices"][0]["message"]["content"].as_str().unwrap_or("{}").to_string()
        }
    };

    Ok(result)
}

pub struct NarrativeEngine {
    store: Arc<Store>,
    ai_config: AiConfig,
    analysis_config: AnalysisConfig,
    notifier: Arc<Notifier>,
    health: SharedProviderHealth,
}

impl NarrativeEngine {
    pub fn new(
        store: Arc<Store>,
        ai_config: AiConfig,
        analysis_config: AnalysisConfig,
        notifier: Arc<Notifier>,
        health: SharedProviderHealth,
    ) -> Self {
        Self { store, ai_config, analysis_config, notifier, health }
    }

    pub async fn run_for_subreddit(&self, sub: &SubredditConfig) -> Result<Option<NarrativeReport>> {
        match self.analysis_config.mode {
            AnalysisMode::Raw => return Ok(None),
            _ => {}
        }

        // Use a 10-minute window for analysis
        let since = (Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
        let comments = self.store.recent_for_subreddit_since(&sub.name, &since, 30).await?;

        if comments.len() < self.analysis_config.cluster_min_comments {
            return Ok(None);
        }

        // Split into first half and second half of the window
        // to detect whether sentiment is actively shifting (event-driven)
        let mid = comments.len() / 2;
        let recent_half = &comments[..mid.min(comments.len())];
        let older_half = &comments[mid..];

        let avg_recent = if recent_half.is_empty() { 0.0 } else {
            recent_half.iter().map(|c| c.score as f64).sum::<f64>() / recent_half.len() as f64
        };
        let avg_older = if older_half.is_empty() { 0.0 } else {
            older_half.iter().map(|c| c.score as f64).sum::<f64>() / older_half.len() as f64
        };
        let avg = comments.iter().map(|c| c.score as f64).sum::<f64>() / comments.len() as f64;

        // Detect a meaningful shift — either overall sentiment is significant
        // OR there's an active delta between recent and older half (event happening now)
        let delta = (avg_recent - avg_older).abs();
        let is_shifting = delta > 0.15; // sentiment moved 0.15+ points in the window
        let is_significant = avg.abs() >= self.analysis_config.narrative_score_threshold as f64;

        info!(
            "r/{}: {} comments | avg {:.3} | delta {:.3} | shifting={} significant={}",
            sub.name, comments.len(), avg, delta, is_shifting, is_significant
        );

        if !is_shifting && !is_significant {
            return Ok(None);
        }

        info!("r/{}: signal detected, calling narrative AI...", sub.name);

        let rag_context = if self.analysis_config.rag_enabled {
            self.build_rag_context(&sub.name).await
        } else {
            String::new()
        };

        let prompt = narrative_prompt(&sub.name, &comments, &rag_context);
        let raw = ai_call(&self.ai_config, &prompt, true, &self.health).await?;
        let parsed = parse_json(&raw);

        let narrative_text = parsed["narrative"].as_str().unwrap_or("").to_string();
        if narrative_text.is_empty() {
            warn!("r/{}: empty narrative response", sub.name);
            return Ok(None);
        }

        let topics: Vec<String> = parsed["topics"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let direction = parsed["direction"].as_str().unwrap_or("stable").to_string();
        let signal_strength = parsed["signal_strength"].as_str().unwrap_or("moderate").to_string();
        let why = parsed["why"].as_str().unwrap_or("").to_string();
        let full_narrative = if why.is_empty() { narrative_text } else { format!("{} {}", narrative_text, why) };

        let mut report = NarrativeReport {
            subreddit: sub.name.clone(),
            narrative: full_narrative,
            topics,
            avg_score: avg as f32,
            comment_count: comments.len(),
            direction,
            signal_strength,
            prediction: None,
            created_at: Utc::now().to_rfc3339(),
        };

        if self.analysis_config.prediction_enabled {
            match self.generate_prediction(&sub.name, &report, &rag_context).await {
                Ok(Some(pred)) => report.prediction = Some(pred),
                Ok(None) => {}
                Err(e) => warn!("Prediction failed for r/{}: {}", sub.name, e),
            }
        }

        let stored = StoredNarrative {
            id: uuid::Uuid::new_v4().to_string(),
            subreddit: sub.name.clone(),
            narrative: report.narrative.clone(),
            topics: report.topics.join(","),
            avg_score: report.avg_score,
            comment_count: report.comment_count as i64,
            signal_strength: report.signal_strength.clone(),
            prediction: report.prediction.as_ref().map(|p| p.text.clone()),
            prediction_confidence: report.prediction.as_ref().map(|p| p.confidence),
            created_at: report.created_at.clone(),
        };
        self.store.save_narrative(&stored).await?;

        info!(
            "Narrative for r/{}: {} topics, score {:.2}, direction: {}",
            sub.name, report.topics.len(), report.avg_score, report.direction
        );

        self.notify(&report).await;
        Ok(Some(report))
    }

    async fn generate_prediction(
        &self,
        subreddit: &str,
        report: &NarrativeReport,
        rag_context: &str,
    ) -> Result<Option<Prediction>> {
        let prompt = prediction_prompt(subreddit, report, rag_context);
        let raw = ai_call(&self.ai_config, &prompt, true, &self.health).await?;
        let parsed = parse_json(&raw);

        let text = parsed["prediction"].as_str().unwrap_or("").to_string();
        let confidence = parsed["confidence"].as_f64().unwrap_or(0.0) as f32;
        let timeframe = parsed["timeframe"].as_str().unwrap_or("unknown").to_string();

        if text.is_empty() || confidence < self.analysis_config.prediction_min_confidence {
            return Ok(None);
        }

        Ok(Some(Prediction { text, confidence, timeframe }))
    }

    async fn build_rag_context(&self, subreddit: &str) -> String {
        match self.store.recent_narratives(
            subreddit,
            self.analysis_config.rag_lookback_days,
            self.analysis_config.rag_max_context as i64,
        ).await {
            Ok(narratives) if !narratives.is_empty() => {
                narratives.iter().map(|n| {
                    let pred = n.prediction.as_deref()
                        .map(|p| format!(" → {}", p))
                        .unwrap_or_default();
                    format!("[{}] {} | {:.2} | {}{}\n",
                        &n.created_at[..16], n.topics, n.avg_score, n.narrative, pred)
                }).collect()
            }
            _ => String::new(),
        }
    }

    async fn notify(&self, report: &NarrativeReport) {
        let (emoji, label) = match report.direction.as_str() {
            "rising_negative" => ("📉", "BEARISH SIGNAL"),
            "rising_positive" => ("📈", "BULLISH SIGNAL"),
            "mixed"           => ("⚡", "MIXED SIGNAL"),
            _                 => ("➡️", "STABLE"),
        };

        let strength_indicator = match report.signal_strength.as_str() {
            "critical" => "🔴 CRITICAL",
            "strong"   => "🟠 STRONG",
            "moderate" => "🟡 MODERATE",
            _          => "⚪ WEAK",
        };

        let topics_str = if report.topics.is_empty() {
            String::new()
        } else {
            format!("\n`{}`", report.topics.join("  ·  "))
        };

        let pred_str = if let Some(pred) = &report.prediction {
            format!(
                "\n\n🔮 *Assessment* — {} confidence | {}\n{}",
                format!("{:.0}%", pred.confidence * 100.0),
                pred.timeframe,
                pred.text
            )
        } else {
            String::new()
        };

        let msg = format!(
            "{} *r/{}* — {} {}\n            Score: `{:.2}` | {} comments{}\n\n            {}{}",
            emoji,
            report.subreddit,
            label,
            strength_indicator,
            report.avg_score,
            report.comment_count,
            topics_str,
            report.narrative,
            pred_str,
        );

        if let Err(e) = self.notifier.send_raw(&msg).await {
            warn!("Narrative notification failed: {}", e);
        }
    }
}

pub fn spawn_narrative_schedulers(
    subreddits: Vec<SubredditConfig>,
    store: Arc<Store>,
    ai_config: AiConfig,
    analysis_config: AnalysisConfig,
    notifier: Arc<Notifier>,
    health: SharedProviderHealth,
) {
    let num_subs = subreddits.len();
    let stagger = ai_config.narrative_stagger_secs(num_subs);

    for (i, sub) in subreddits.into_iter().enumerate() {
        let interval = if sub.narrative_interval_secs > 0 {
            sub.narrative_interval_secs
        } else {
            ai_config.narrative_interval_secs(num_subs)
        };

        let startup_delay = (i as u64) * stagger;

        let engine = NarrativeEngine::new(
            Arc::clone(&store),
            ai_config.clone(),
            analysis_config.clone(),
            Arc::clone(&notifier),
            Arc::clone(&health),
        );

        tokio::spawn(async move {
            if startup_delay > 0 {
                tokio::time::sleep(tokio::time::Duration::from_secs(startup_delay)).await;
            }
            info!(
                "Narrative engine for r/{} — every {}s (startup delay: {}s)",
                sub.name, interval, startup_delay
            );
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                if let Err(e) = engine.run_for_subreddit(&sub).await {
                    warn!("Narrative engine error for r/{}: {}", sub.name, e);
                }
            }
        });
    }
}
