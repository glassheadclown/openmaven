# 📈 OpenMaven

<div align="center">
  <!-- TODO: Add project logo if available. Consider an icon related to market trends or data intelligence. -->

[![GitHub stars](https://img.shields.io/github/stars/glassheadclown/openmaven?style=for-the-badge)](https://github.com/glassheadclown/openmaven/stargazers)
[![GitHub forks](https://img.shields.io/github/forks/glassheadclown/openmaven?style=for-the-badge)](https://github.com/glassheadclown/openmaven/network)
[![GitHub issues](https://img.shields.io/github/issues/glassheadclown/openmaven?style=for-the-badge)](https://github.com/glassheadclown/openmaven/issues)
[![License](https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-important?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)

## Real-time Reddit sentiment analysis, trend detection, and predictive market signals.
</div>
<img width="1216" height="330" alt="image" src="https://github.com/user-attachments/assets/b47c9f4d-13b3-4401-909f-6441d817c5b9" />
<img width="1893" height="1010" alt="image" src="https://github.com/user-attachments/assets/8d4b7aa7-409e-4102-89d2-1f9f3d3c379d" />
<img width="1854" height="1465" alt="image" src="https://github.com/user-attachments/assets/97ffea1b-ed65-498b-869d-05593e390b1b" />


## What is OpenMaven?

OpenMaven monitors Reddit comment streams in real-time and turns raw community noise into actionable intelligence. Built in **Rust** for speed and reliability, it's designed for traders, analysts, and anyone who needs to identify emerging trends before they hit the mainstream.

It goes beyond basic sentiment scraping—OpenMaven generates analyst-style summaries that explain *why* the mood is shifting and what that likely means for the market.

## Features

* 🎯 **Live Sentiment Scoring** — Every incoming comment is classified (Positive / Negative / Neutral) as it arrives via the Sylvia stream.
* 🧠 **Mood Shift Detection** — Automatically triggers when sentiment changes significantly, generating a concise brief on what's driving it.
* 🔮 **Predictive Signals** — Produces forward-looking predictions with confidence scores, so you know how much weight to put on them.
* 📚 **Contextual Memory** — Uses a vector store to retain historical data, allowing it to connect current events to past patterns.
* ⚡ **High Performance** — Rust keeps things fast and memory-safe, handling large data volumes without issue.

## Tech Stack

* **Language:** Rust (1.75+)
* **Database:** SQLite — stored at `~/.openmaven/data.db`
* **Data Source:** Sylvia API
* **AI Providers:** Groq, Anthropic, OpenAI, HuggingFace, Ollama (your choice)

## Getting Started

**Prerequisites:**
1. **Rust** — Install from [rust-lang.org](https://www.rust-lang.org/tools/install)
2. **Sylvia API key** — Get one at [sylvia-api.com](https://sylvia-api.com)
3. **AI provider key** — OpenAI, Groq, or similar

```bash
git clone https://github.com/glassheadclown/openmaven
cd openmaven
cargo run
```

On first run, a setup wizard walks you through configuration. You have two options:

* **Easy setup** — Pick a major provider (Groq, OpenAI, etc.) and let the wizard configure defaults.
* **Manual setup** — Set your own API URLs, rate limits, and polling windows.

The wizard also shows an estimated daily cost for the Sylvia API before you commit.

## CLI Reference

| Command | Description |
| :--- | :--- |
| `cargo run` | Start OpenMaven (or launch the setup wizard on first run) |
| `cargo run -- dashboard` | Open the TUI dashboard |
| `cargo run -- setup` | Re-run configuration |
| `cargo run -- backfill` | Fetch historical data (e.g., `--subreddit wallstreetbets --limit 10`) |
| `cargo run -- export` | Export your data (e.g., `--format markdown`) |
| `cargo run -- stats` | View database stats |

## Configuration

Settings are stored at `~/.openmaven/config.toml` and can be edited directly if you prefer not to use the wizard.
```toml
[sylvia]
api_key = "syl_..."
max_comments_per_poll = 100

[ai]
provider_type = "openai"
sentiment_model = "llama-3.1-8b-instant"      # Fast/cheap for high-volume classification
narrative_model = "llama-3.3-70b-versatile"   # More capable model for analysis
tpm_limit = 6000

[[tracking.subreddits]]
name = "wallstreetbets"
keywords = ["GME", "puts", "calls"]
poll_interval_secs = 60
```

## Example Output

> **Signal:** Sentiment around [Topic] in r/worldnews is shifting rapidly.
> 🔮 **High Confidence** — 85% | Timeframe: next 2–4 hours
> *Historically, this pattern precedes an official government statement. Watch for movement from NATO.*

## Sylvia API Cost Estimates (scanning 3 subreddits)

| Poll Interval | Est. Daily Cost(24H) |
| :--- | :--- |
| 15 seconds | ~$864 |
| 60 seconds | ~$216 |
| 5 minutes | ~$43 |
| 15 minutes | ~$14 |

*AI inference costs vary by provider. OpenMaven batches requests to stay within rate limits.*

## To Do List
- Implement global keywords that work across all selected subreddits
- Implement a chat feature for web/tui dashboard
- Build a better web dashboard
- Implement trend reporting in Tui dashboard
- Implement setting daily budget setup  

## Contributing

1. Fork the repo
2. Create a branch (`git checkout -b feature/YourFeature`)
3. Run `cargo fmt` and `cargo clippy` before submitting
4. Open a pull request

## License

MIT — see the `LICENSE` file for details.

<div align="center">

⭐ If this is useful, a star goes a long way!

Made with ❤️ by glassheadclown with help from claude code
</div>
