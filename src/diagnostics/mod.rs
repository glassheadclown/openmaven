use crate::config::Config;
use crate::notify::Notifier;
use std::sync::Arc;
use tracing::info;

/// Build the diagnostics report string
pub fn build_report(cfg: &Config) -> String {
    let num_subs = cfg.tracking.subreddits.len();
    let poll_interval = cfg.tracking.default_poll_interval_secs;
    let narrative_interval = cfg.ai.narrative_interval_secs(num_subs);
    let stagger = cfg.ai.narrative_stagger_secs(num_subs);
    let batch_size = cfg.ai.effective_batch_size();
    let daily_tokens = cfg.ai.estimate_daily_tokens(num_subs, poll_interval);
    let daily_sylvia = cfg.ai.estimate_daily_sylvia_cost(
        num_subs,
        poll_interval,
        cfg.sylvia.cost_per_request,
    );

    let daily_cap_line = if let Some(cap) = cfg.ai.daily_token_limit {
        if daily_tokens > cap as u64 {
            format!(
                "⚠  Daily token cap: {}\n   Estimated usage: {}/day — you will hit the cap in ~{:.1}h\n   → Increase poll interval or reduce subreddit count to stay within limits",
                cap,
                daily_tokens,
                cap as f64 / (daily_tokens as f64 / 24.0)
            )
        } else {
            format!(
                "✓  Daily token cap: {}\n   Estimated usage: {}/day — within limits",
                cap, daily_tokens
            )
        }
    } else {
        format!("   Estimated daily tokens: {} (no daily cap)", daily_tokens)
    };

    let bottleneck = if cfg.ai.tpm_limit == u32::MAX {
        "   No TPM limit — Ollama running locally".to_string()
    } else {
        format!(
            "   TPM limit: {} tokens/min\n   → Narratives fire every ~{}s, staggered {}s apart across {} subreddits",
            cfg.ai.tpm_limit, narrative_interval, stagger, num_subs
        )
    };

    let mode_str = match cfg.analysis.mode {
        crate::config::AnalysisMode::Raw => "Raw alerts only",
        crate::config::AnalysisMode::Narrative => "Narrative + predictions only",
        crate::config::AnalysisMode::Both => "Raw alerts + narrative + predictions",
    };

    format!(
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
        🔍 openmaven — system diagnostics\n\
        ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
        \n\
        📡  Data source\n\
           Sylvia API  (sylvia-api.com)\n\
           {} subreddits  •  polling every {}s\n\
           Est. Sylvia cost: ${:.2}/day  •  ${:.2}/month\n\
        \n\
        🤖  AI provider\n\
           Sentiment model:  {}\n\
           Narrative model:  {}\n\
           Batch size: {} comments/call\n\
           Context window: {} tokens\n\
        \n\
        ⚡  Rate limits\n\
        {}\n\
        {}\n\
        \n\
        🧠  Analysis\n\
           Mode: {}\n\
           RAG memory: {}  •  lookback: {} days\n\
           Predictions: {}\n\
        \n\
        ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
        openmaven is running. Good luck.",
        num_subs,
        poll_interval,
        daily_sylvia,
        daily_sylvia * 30.0,
        cfg.ai.sentiment_model,
        cfg.ai.narrative_model,
        batch_size,
        cfg.ai.context_window,
        bottleneck,
        daily_cap_line,
        mode_str,
        if cfg.analysis.rag_enabled { "enabled" } else { "disabled" },
        cfg.analysis.rag_lookback_days,
        if cfg.analysis.prediction_enabled { "enabled" } else { "disabled" },
    )
}

/// Print diagnostics to stdout / logs
pub fn print_report(cfg: &Config) {
    let report = build_report(cfg);
    for line in report.lines() {
        info!("{}", line);
    }
}

/// Send diagnostics to Discord/Telegram on first run
pub async fn send_first_run_report(cfg: &mut Config, notifier: Arc<Notifier>) {
    if cfg.meta.diagnostics_sent {
        return;
    }

    let report = build_report(cfg);
    print_report(cfg);

    if let Err(e) = notifier.send_raw(&report).await {
        tracing::warn!("Failed to send diagnostics notification: {}", e);
    }

    cfg.meta.diagnostics_sent = true;
    if let Err(e) = cfg.save() {
        tracing::warn!("Failed to save diagnostics_sent flag: {}", e);
    }
}
