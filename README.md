📈 OpenMaven

<div align="center">

Real-time Reddit sentiment intelligence, narrative detection, and predictive analytics.

</div>

📖 Overview

OpenMaven tracks Reddit comment streams, scores sentiment in real-time, and surfaces what the data actually signals. Built in Rust for maximum performance, it is designed for prediction market traders, researchers, and analysts who need to read public opinion before it hits the mainstream.

Unlike simple scrapers, OpenMaven extracts "Analyst-style" briefs, identifying not just what people are saying, but what it means for the market.

✨ Key Features

🎯 Raw Sentiment Scoring: Every comment is scored (Positive/Negative/Neutral) as it arrives via the Sylvia stream.

🧠 Narrative Engine: Automatically triggers on meaningful sentiment shifts. It generates briefs explaining the why behind the data.

🔮 Prediction Engine: Generates forward-looking, falsifiable assessments with specific confidence scores.

📚 RAG Memory: Utilizes a vector store to inject past narratives as context, correlating current signals with events from days or weeks ago.

⚡ High Performance: Leverages Rust's safety and speed to handle massive data throughput with minimal latency.

🛠️ Tech Stack

Language: Rust (1.75+)

Database: SQLite (Local storage at ~/.openmaven/data.db)

Data Source: Sylvia API

AI Providers: Groq, Anthropic, OpenAI, HuggingFace, Ollama (OpenAI-compatible)

🚀 Quick Start

Prerequisites

Rust Toolchain: Install Rust

Sylvia API Key: Get one at sylvia-api.com

AI API Key: (Groq, OpenAI, Anthropic, etc.)

Installation & Setup

Clone and Run

git clone [https://github.com/glassheadclown/openmaven](https://github.com/glassheadclown/openmaven)
cd openmaven
cargo run


Setup Wizard: On the first run, the interactive wizard will launch.

Simple Mode: Choose a known provider (Groq, OpenAI, etc.). Limits and batch sizes are auto-calculated.

Advanced Mode: Manually set Base URLs, TPM limits, and context windows.

Cost Estimate: The wizard displays estimated Sylvia costs based on your polling intervals before you commit.

💻 CLI Usage

Command

Description

cargo run

Launch the daemon (or wizard on first run).

cargo run -- dashboard

Open the live TUI dashboard.

cargo run -- setup

Re-run the configuration wizard.

cargo run -- backfill

e.g., --subreddit wallstreetbets --limit 10

cargo run -- export

Export data (e.g., --format markdown).

cargo run -- stats

View local database and performance stats.

⚙️ Configuration

Configuration is stored at ~/.openmaven/config.toml. You can edit this file directly to bypass the wizard.

[sylvia]
api_key = "syl_..."
max_comments_per_poll = 100

[ai]
provider_type = "openai"
sentiment_model = "llama-3.1-8b-instant" # Fast/Cheap for scoring
narrative_model = "llama-3.3-70b-versatile" # Smart for analysis
tpm_limit = 6000

[[tracking.subreddits]]
name = "wallstreetbets"
keywords = ["GME", "puts", "calls"]
poll_interval_secs = 60


📊 Narrative Output Example

OpenMaven doesn't just provide raw numbers; it generates actionable intelligence:

Signal: Public sentiment on [Topic] is deteriorating sharply across r/worldnews, suggesting the situation is escalating...

🔮 Assessment — 85% confidence | Next 2-4 hours
Sustained negative sentiment at this velocity historically precedes major diplomatic statements. Watch for NATO response announcements.

💰 Cost Transparency (Sylvia API)

Interval

Daily Cost (Approx)

15s

~$864

60s

~$216

5 min

~$43

15 min

~$14

AI costs vary by provider. OpenMaven uses batching to respect your TPM/Rate limits.

🤝 Contributing

Fork the repository.

Create a feature branch (git checkout -b feature/AmazingFeature).

Run cargo fmt and cargo clippy before committing.

Open a Pull Request.

📄 License

Distributed under the MIT License. See LICENSE for more information.

<div align="center">

⭐ Star this repo if you find it helpful!
Made with ❤️ by glassheadclown

</div>
