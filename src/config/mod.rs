use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("OPENMAVEN_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openmaven")
        .join("config.toml")
}

pub fn db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openmaven")
        .join("data.db")
}

// ── Known provider presets ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KnownProvider {
    ClaudeHaiku,
    ClaudeSonnet,
    Gpt4oMini,
    Gpt4o,
    GroqFree,
    GroqDev,
    HuggingFace,
    Ollama,
    Custom,
}

pub struct ProviderPreset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub default_sentiment_model: &'static str,
    pub default_narrative_model: &'static str,
    pub tpm_limit: u32,
    pub context_window: u32,
    pub daily_token_limit: Option<u32>,
    pub provider_type: AiProviderType,
}

impl KnownProvider {
    pub fn preset(&self) -> ProviderPreset {
        match self {
            KnownProvider::ClaudeHaiku => ProviderPreset {
                name: "Anthropic — Claude Haiku",
                base_url: "https://api.anthropic.com",
                default_sentiment_model: "claude-haiku-4-5-20251001",
                default_narrative_model: "claude-haiku-4-5-20251001",
                tpm_limit: 50_000,
                context_window: 200_000,
                daily_token_limit: None,
                provider_type: AiProviderType::Anthropic,
            },
            KnownProvider::ClaudeSonnet => ProviderPreset {
                name: "Anthropic — Claude Sonnet",
                base_url: "https://api.anthropic.com",
                default_sentiment_model: "claude-haiku-4-5-20251001",
                default_narrative_model: "claude-sonnet-4-6",
                tpm_limit: 40_000,
                context_window: 200_000,
                daily_token_limit: None,
                provider_type: AiProviderType::Anthropic,
            },
            KnownProvider::Gpt4oMini => ProviderPreset {
                name: "OpenAI — GPT-4o mini",
                base_url: "https://api.openai.com",
                default_sentiment_model: "gpt-4o-mini",
                default_narrative_model: "gpt-4o-mini",
                tpm_limit: 200_000,
                context_window: 128_000,
                daily_token_limit: None,
                provider_type: AiProviderType::OpenAI,
            },
            KnownProvider::Gpt4o => ProviderPreset {
                name: "OpenAI — GPT-4o",
                base_url: "https://api.openai.com",
                default_sentiment_model: "gpt-4o-mini",
                default_narrative_model: "gpt-4o",
                tpm_limit: 30_000,
                context_window: 128_000,
                daily_token_limit: None,
                provider_type: AiProviderType::OpenAI,
            },
            KnownProvider::GroqFree => ProviderPreset {
                name: "Groq — Free tier (groq.com)",
                base_url: "https://api.groq.com/openai",
                default_sentiment_model: "llama-3.1-8b-instant",
                default_narrative_model: "llama-3.3-70b-versatile",
                tpm_limit: 6_000,
                context_window: 128_000,
                daily_token_limit: Some(100_000),
                provider_type: AiProviderType::OpenAI,
            },
            KnownProvider::GroqDev => ProviderPreset {
                name: "Groq — Dev tier (groq.com)",
                base_url: "https://api.groq.com/openai",
                default_sentiment_model: "llama-3.1-8b-instant",
                default_narrative_model: "llama-3.3-70b-versatile",
                tpm_limit: 100_000,
                context_window: 128_000,
                daily_token_limit: None,
                provider_type: AiProviderType::OpenAI,
            },
            KnownProvider::HuggingFace => ProviderPreset {
                name: "HuggingFace Inference API",
                base_url: "https://api-inference.huggingface.co/v1",
                default_sentiment_model: "meta-llama/Llama-3.1-8B-Instruct",
                default_narrative_model: "meta-llama/Llama-3.3-70B-Instruct",
                tpm_limit: 8_000,
                context_window: 128_000,
                daily_token_limit: Some(100_000),
                provider_type: AiProviderType::OpenAI,
            },
            KnownProvider::Ollama => ProviderPreset {
                name: "Ollama — local models",
                base_url: "http://localhost:11434",
                default_sentiment_model: "llama3",
                default_narrative_model: "llama3",
                tpm_limit: u32::MAX,
                context_window: 32_000,
                daily_token_limit: None,
                provider_type: AiProviderType::Ollama,
            },
            KnownProvider::Custom => ProviderPreset {
                name: "Custom / OpenAI-compatible endpoint",
                base_url: "",
                default_sentiment_model: "",
                default_narrative_model: "",
                tpm_limit: 10_000,
                context_window: 32_000,
                daily_token_limit: None,
                provider_type: AiProviderType::OpenAI,
            },
        }
    }

    pub fn simple_list() -> Vec<KnownProvider> {
        vec![
            KnownProvider::GroqFree,
            KnownProvider::GroqDev,
            KnownProvider::ClaudeHaiku,
            KnownProvider::ClaudeSonnet,
            KnownProvider::Gpt4oMini,
            KnownProvider::Gpt4o,
            KnownProvider::HuggingFace,
            KnownProvider::Ollama,
        ]
    }
}

// ── Config structs ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub sylvia: SylviaConfig,
    pub ai: AiConfig,
    pub tracking: TrackingConfig,
    pub notifications: NotificationConfig,
    pub web: WebConfig,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub meta: MetaConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetaConfig {
    /// Set to true after first-run diagnostics are sent so we don't repeat them
    pub diagnostics_sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SylviaConfig {
    pub api_key: String,
    pub base_url: String,
    /// Use per-subreddit live endpoints — cheaper and more targeted than firehose
    #[serde(default = "default_true")]
    pub use_subreddit_endpoints: bool,
    /// Cost per live request in USD — used for cost estimates in diagnostics
    #[serde(default = "default_sylvia_cost")]
    pub cost_per_request: f64,
    /// Max comments returned per poll — lower saves Sylvia credits
    #[serde(default = "default_max_comments")]
    pub max_comments_per_poll: u32,
}

fn default_true() -> bool { true }
fn default_sylvia_cost() -> f64 { 0.05 }
fn default_max_comments() -> u32 { 100 }

impl Default for SylviaConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.sylvia-api.com".to_string(),
            use_subreddit_endpoints: true,
            cost_per_request: 0.05,
            max_comments_per_poll: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderType {
    Anthropic,
    OpenAI,
    Ollama,
    Custom,
}

impl std::fmt::Display for AiProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiProviderType::Anthropic => write!(f, "Anthropic"),
            AiProviderType::OpenAI => write!(f, "OpenAI-compatible"),
            AiProviderType::Ollama => write!(f, "Ollama"),
            AiProviderType::Custom => write!(f, "Custom"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider_type: AiProviderType,
    pub api_key: Option<String>,
    pub base_url: String,
    /// Model for per-comment sentiment scoring — use a cheaper/faster model here
    pub sentiment_model: String,
    /// Model for narrative + prediction — use a smarter model here
    pub narrative_model: String,
    /// Tokens per minute limit for this provider/tier
    pub tpm_limit: u32,
    /// Context window in tokens
    pub context_window: u32,
    /// Optional daily token cap (e.g. Groq free = 100k/day)
    pub daily_token_limit: Option<u32>,
    /// Batch size override. 0 = auto-calculate from context window.
    #[serde(default)]
    pub batch_size: usize,
}

impl AiConfig {
    /// Safe batch size based on context window
    pub fn effective_batch_size(&self) -> usize {
        if self.batch_size > 0 {
            return self.batch_size;
        }
        // ~500 tokens per comment. Use 40% of context window.
        let safe_tokens = (self.context_window as f64 * 0.4) as usize;
        (safe_tokens / 500).clamp(3, 25)
    }

    /// Safe narrative interval based on TPM and subreddit count
    pub fn narrative_interval_secs(&self, num_subreddits: usize) -> u64 {
        if self.tpm_limit == u32::MAX {
            return 30; // unlimited (Ollama)
        }
        // Each narrative call ~1500 tokens
        let tokens_per_narrative = 1500u32;
        let calls_per_minute = (self.tpm_limit / tokens_per_narrative).max(1);
        let secs = (num_subreddits as f64 / calls_per_minute as f64 * 60.0).ceil() as u64;
        secs.clamp(30, 600)
    }

    /// Stagger delay between subreddit narrative timers in seconds
    pub fn narrative_stagger_secs(&self, num_subreddits: usize) -> u64 {
        if num_subreddits == 0 { return 0; }
        let interval = self.narrative_interval_secs(num_subreddits);
        (interval / num_subreddits as u64).max(5)
    }

    /// Estimated daily token usage — realistic based on actual observed usage
    pub fn estimate_daily_tokens(&self, num_subreddits: usize, poll_interval_secs: u64) -> u64 {
        // In practice most polls return ~5-15 new comments after dedup
        // Each comment is ~100-150 tokens in the prompt
        let polls_per_day = 86_400 / poll_interval_secs.max(1);
        let avg_new_comments_per_poll = 8u64; // conservative after dedup
        let tokens_per_comment = 120u64;
        let sentiment = polls_per_day * num_subreddits as u64 * avg_new_comments_per_poll * tokens_per_comment;
        // Narrative fires at calculated interval, ~1500 tokens each
        let narrative_interval = self.narrative_interval_secs(num_subreddits);
        let narratives_per_day = 86_400 / narrative_interval.max(1);
        let narrative = narratives_per_day * num_subreddits as u64 * 1500;
        sentiment + narrative
    }

    /// Estimated daily Sylvia cost in USD
    /// One request = one poll of one subreddit = $0.05 (live endpoint)
    pub fn estimate_daily_sylvia_cost(
        &self,
        num_subreddits: usize,
        poll_interval_secs: u64,
        cost_per_req: f64,
    ) -> f64 {
        // Each poll of each subreddit = 1 Sylvia request
        let polls_per_day = 86_400.0 / poll_interval_secs.max(1) as f64;
        polls_per_day * num_subreddits as f64 * cost_per_req
    }

    /// Human-readable bottleneck explanation
    pub fn bottleneck_explanation(&self, num_subreddits: usize, poll_interval_secs: u64) -> String {
        let daily_tokens = self.estimate_daily_tokens(num_subreddits, poll_interval_secs);
        let narrative_interval = self.narrative_interval_secs(num_subreddits);

        let mut lines = vec![];

        if self.tpm_limit == u32::MAX {
            lines.push("✓ No TPM limit — Ollama running locally, unlimited throughput.".to_string());
        } else {
            lines.push(format!(
                "TPM limit: {} tokens/min → narratives fire every ~{}s across {} subreddits.",
                self.tpm_limit, narrative_interval, num_subreddits
            ));
        }

        if let Some(daily_cap) = self.daily_token_limit {
            let hours_until_cap = daily_cap as f64 / (daily_tokens as f64 / 24.0);
            if daily_tokens > daily_cap as u64 {
                lines.push(format!(
                    "⚠ Daily token cap: {}. At current settings you'll hit it in {:.1}h. Consider increasing poll interval or reducing subreddits.",
                    daily_cap, hours_until_cap
                ));
            } else {
                lines.push(format!(
                    "✓ Daily token cap: {}. Estimated usage: {}/day — you're within limits.",
                    daily_cap, daily_tokens
                ));
            }
        } else {
            lines.push(format!("Estimated daily token usage: {}", daily_tokens));
        }

        lines.push(format!(
            "Sentiment model: {}  |  Narrative model: {}",
            self.sentiment_model, self.narrative_model
        ));
        lines.push(format!(
            "Batch size: {} comments/call  |  Context window: {} tokens",
            self.effective_batch_size(), self.context_window
        ));

        lines.join("\n")
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        let preset = KnownProvider::GroqFree.preset();
        Self {
            provider_type: AiProviderType::OpenAI,
            api_key: None,
            base_url: preset.base_url.to_string(),
            sentiment_model: preset.default_sentiment_model.to_string(),
            narrative_model: preset.default_narrative_model.to_string(),
            tpm_limit: preset.tpm_limit,
            context_window: preset.context_window,
            daily_token_limit: preset.daily_token_limit,
            batch_size: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubredditConfig {
    pub name: String,
    pub keywords: Vec<String>,
    pub poll_interval_secs: u64,
    pub sentiment_alert_threshold: f32,
    /// Per-subreddit narrative interval override. 0 = use auto-calculated value.
    #[serde(default)]
    pub narrative_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingConfig {
    pub subreddits: Vec<SubredditConfig>,
    pub default_poll_interval_secs: u64,
    pub default_alert_threshold: f32,
}

impl Default for TrackingConfig {
    fn default() -> Self {
        Self {
            subreddits: vec![],
            default_poll_interval_secs: 60,
            default_alert_threshold: 0.85,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    pub enabled: bool,
    pub port: u16,
}

impl Default for WebConfig {
    fn default() -> Self { Self { enabled: false, port: 7860 } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisMode {
    Raw,
    Narrative,
    Both,
}

impl Default for AnalysisMode {
    fn default() -> Self { AnalysisMode::Both }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    pub mode: AnalysisMode,
    pub cluster_min_comments: usize,
    pub narrative_score_threshold: f32,
    pub rag_enabled: bool,
    pub rag_lookback_days: u32,
    pub rag_max_context: usize,
    pub prediction_enabled: bool,
    pub prediction_min_confidence: f32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            mode: AnalysisMode::Both,
            cluster_min_comments: 5,
            narrative_score_threshold: 0.1,
            rag_enabled: true,
            rag_lookback_days: 14,
            rag_max_context: 5,
            prediction_enabled: true,
            prediction_min_confidence: 0.65,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read config at {}", path.display()))?;
        toml::from_str(&content).context("Failed to parse config file")
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn exists() -> bool {
        config_path().exists()
    }
}
