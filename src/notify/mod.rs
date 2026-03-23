use anyhow::Result;
use crate::config::NotificationConfig;
use crate::sentiment::SentimentResult;

pub struct Notifier {
    client: reqwest::Client,
    config: NotificationConfig,
}

impl Notifier {
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub async fn alert(&self, subreddit: &str, result: &SentimentResult, body: &str) -> Result<()> {
        let msg = format!(
            "🔴 *openmaven alert*\n\n\
            *r/{}* — sentiment spike detected\n\
            Score: `{:.2}` ({})\n\
            Confidence: `{:.0}%`\n\n\
            _{}_\n\n\
            > {}",
            subreddit,
            result.score,
            result.label,
            result.confidence * 100.0,
            result.summary,
            truncate(body, 200)
        );

        let mut tasks = vec![];

        if let Some(tg) = &self.config.telegram {
            let client = self.client.clone();
            let token = tg.bot_token.clone();
            let chat_id = tg.chat_id.clone();
            let msg_clone = msg.clone();
            tasks.push(tokio::spawn(async move {
                send_telegram(&client, &token, &chat_id, &msg_clone).await
            }));
        }

        if let Some(dc) = &self.config.discord {
            let client = self.client.clone();
            let url = dc.webhook_url.clone();
            let msg_clone = msg.clone();
            tasks.push(tokio::spawn(async move {
                send_discord(&client, &url, &msg_clone).await
            }));
        }

        for task in tasks {
            if let Err(e) = task.await {
                tracing::warn!("Notification task failed: {}", e);
            }
        }

        Ok(())
    }
}

async fn send_telegram(client: &reqwest::Client, token: &str, chat_id: &str, text: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        }))
        .send()
        .await?;
    Ok(())
}

async fn send_discord(client: &reqwest::Client, webhook_url: &str, text: &str) -> Result<()> {
    // Discord doesn't support Markdown the same way, strip formatting
    let clean = text
        .replace("*", "**")
        .replace("`", "`");
    client
        .post(webhook_url)
        .json(&serde_json::json!({
            "content": clean,
            "username": "openmaven"
        }))
        .send()
        .await?;
    Ok(())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

impl Notifier {
    /// Send a pre-formatted message to all configured channels.
    /// Used by the narrative engine.
    pub async fn send_raw(&self, msg: &str) -> Result<()> {
        let mut tasks = vec![];

        if let Some(tg) = &self.config.telegram {
            let client = self.client.clone();
            let token = tg.bot_token.clone();
            let chat_id = tg.chat_id.clone();
            let m = msg.to_string();
            tasks.push(tokio::spawn(async move {
                send_telegram(&client, &token, &chat_id, &m).await
            }));
        }

        if let Some(dc) = &self.config.discord {
            let client = self.client.clone();
            let url = dc.webhook_url.clone();
            let m = msg.to_string();
            tasks.push(tokio::spawn(async move {
                send_discord(&client, &url, &m).await
            }));
        }

        for task in tasks {
            if let Err(e) = task.await {
                tracing::warn!("send_raw task error: {}", e);
            }
        }

        Ok(())
    }
}
