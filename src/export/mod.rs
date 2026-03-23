use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;

use crate::store::{StoredResult, SubredditStats};
use crate::trends::TrendSummary;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ExportFormat {
    Csv,
    Json,
    Markdown,
}

pub struct Exporter;

impl Exporter {
    /// Export recent results to a file. Returns the path written.
    pub fn export_results(
        results: &[StoredResult],
        stats: &[SubredditStats],
        trends: &[TrendSummary],
        format: ExportFormat,
        output_dir: &PathBuf,
    ) -> Result<PathBuf> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = match format {
            ExportFormat::Csv => format!("redpulse_export_{}.csv", timestamp),
            ExportFormat::Json => format!("redpulse_export_{}.json", timestamp),
            ExportFormat::Markdown => format!("redpulse_report_{}.md", timestamp),
        };

        std::fs::create_dir_all(output_dir)?;
        let path = output_dir.join(&filename);

        let content = match format {
            ExportFormat::Csv => Self::to_csv(results),
            ExportFormat::Json => Self::to_json(results, stats, trends)?,
            ExportFormat::Markdown => Self::to_markdown(results, stats, trends),
        };

        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn to_csv(results: &[StoredResult]) -> String {
        let mut out = String::from(
            "comment_id,subreddit,author,score,label,confidence,summary,created_at,permalink\n"
        );
        for r in results {
            out.push_str(&format!(
                "{},{},{},{:.4},{},{:.4},{},{},{}\n",
                csv_escape(&r.comment_id),
                csv_escape(&r.subreddit),
                csv_escape(&r.author),
                r.score,
                csv_escape(&r.label),
                r.confidence,
                csv_escape(&r.summary),
                csv_escape(&r.created_at),
                csv_escape(&r.permalink),
            ));
        }
        out
    }

    fn to_json(
        results: &[StoredResult],
        stats: &[SubredditStats],
        trends: &[TrendSummary],
    ) -> Result<String> {
        let payload = serde_json::json!({
            "exported_at": Utc::now().to_rfc3339(),
            "total_comments": results.len(),
            "stats": stats,
            "trends": trends,
            "results": results,
        });
        Ok(serde_json::to_string_pretty(&payload)?)
    }

    fn to_markdown(
        results: &[StoredResult],
        stats: &[SubredditStats],
        trends: &[TrendSummary],
    ) -> String {
        let mut md = String::new();

        md.push_str("# redpulse Sentiment Report\n\n");
        md.push_str(&format!("Generated: `{}`\n\n", Utc::now().format("%Y-%m-%d %H:%M UTC")));

        // Summary table
        md.push_str("## Subreddit Summary\n\n");
        md.push_str("| Subreddit | Total | Avg Score | Positive | Negative | Neutral |\n");
        md.push_str("|---|---|---|---|---|---|\n");
        for s in stats {
            let total = s.positive_count + s.negative_count + s.neutral_count;
            md.push_str(&format!(
                "| r/{} | {} | {:.2} | {} ({:.0}%) | {} ({:.0}%) | {} ({:.0}%) |\n",
                s.subreddit,
                s.total,
                s.avg_score,
                s.positive_count,
                if total > 0 { s.positive_count as f64 / total as f64 * 100.0 } else { 0.0 },
                s.negative_count,
                if total > 0 { s.negative_count as f64 / total as f64 * 100.0 } else { 0.0 },
                s.neutral_count,
                if total > 0 { s.neutral_count as f64 / total as f64 * 100.0 } else { 0.0 },
            ));
        }

        // Trend section
        if !trends.is_empty() {
            md.push_str("\n## Keyword Trends\n\n");
            md.push_str("| Subreddit | Keyword | Current Score | Direction | Volume |\n");
            md.push_str("|---|---|---|---|---|\n");
            for t in trends {
                md.push_str(&format!(
                    "| r/{} | `{}` | {} | {} | {} |\n",
                    t.subreddit,
                    t.keyword,
                    t.current_score.map(|s| format!("{:.2}", s)).unwrap_or_else(|| "—".into()),
                    t.direction,
                    t.volume,
                ));
            }
        }

        // Notable comments (top negative and top positive)
        let mut sorted = results.to_vec();
        sorted.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));

        md.push_str("\n## Most Negative Comments\n\n");
        for r in sorted.iter().take(5) {
            md.push_str(&format!(
                "**r/{}** | `{:.2}` | u/{}\n\n> {}\n\n*{}*\n\n---\n\n",
                r.subreddit, r.score, r.author,
                r.body.lines().next().unwrap_or("").chars().take(300).collect::<String>(),
                r.summary
            ));
        }

        md.push_str("## Most Positive Comments\n\n");
        for r in sorted.iter().rev().take(5) {
            md.push_str(&format!(
                "**r/{}** | `{:.2}` | u/{}\n\n> {}\n\n*{}*\n\n---\n\n",
                r.subreddit, r.score, r.author,
                r.body.lines().next().unwrap_or("").chars().take(300).collect::<String>(),
                r.summary
            ));
        }

        md
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
