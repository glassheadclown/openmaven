use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;

use crate::config::*;

const ACCENT: Color = Color::Rgb(255, 69, 69);
const DIM: Color = Color::Rgb(90, 90, 110);
const BRIGHT: Color = Color::Rgb(240, 240, 248);
const BG: Color = Color::Rgb(10, 10, 14);
const YELLOW: Color = Color::Rgb(255, 200, 80);

#[derive(Debug, Clone, PartialEq)]
enum WizardStep {
    Welcome,
    SetupMode,        // simple or advanced
    // Data source
    SylviaKey,
    // AI — simple path
    SimpleProvider,
    SimpleApiKey,
    // AI — advanced path
    AdvancedBaseUrl,
    AdvancedApiKey,
    AdvancedSentimentModel,
    AdvancedNarrativeModel,
    AdvancedTpm,
    AdvancedContext,
    AdvancedDailyCap,
    // Shared
    Subreddits,
    Keywords,
    PollInterval,
    AlertThreshold,
    AnalysisMode,
    Notifications,
    TelegramSetup,
    DiscordSetup,
    WebUi,
    Summary,
    Done,
}

#[derive(Debug, Clone, PartialEq)]
enum SetupMode { Simple, Advanced }

pub struct Wizard {
    step: WizardStep,
    setup_mode: SetupMode,
    input: String,
    error_msg: Option<String>,

    // sylvia
    sylvia_key: String,

    // ai — simple
    simple_provider_idx: usize,
    simple_api_key: String,

    // ai — advanced
    adv_base_url: String,
    adv_api_key: String,
    adv_sentiment_model: String,
    adv_narrative_model: String,
    adv_tpm: String,
    adv_context: String,
    adv_daily_cap: String,

    // tracking
    subreddits: Vec<String>,
    sub_input: String,
    keywords_map: std::collections::HashMap<String, Vec<String>>,
    kw_input: String,
    kw_sub_idx: usize,
    poll_interval: u64,
    alert_threshold: f32,

    // analysis
    analysis_mode_idx: usize,

    // notifications
    notif_idx: usize,
    tg_token: String,
    tg_chat: String,
    discord_url: String,
    tg_phase: usize, // 0 = token, 1 = chat_id

    // web
    web_idx: usize,
    web_port: u16,

    // wizard mode cursor
    mode_idx: usize,
}

impl Wizard {
    pub fn new() -> Self {
        Self {
            step: WizardStep::Welcome,
            setup_mode: SetupMode::Simple,
            input: String::new(),
            error_msg: None,
            sylvia_key: String::new(),
            simple_provider_idx: 0,
            simple_api_key: String::new(),
            adv_base_url: String::new(),
            adv_api_key: String::new(),
            adv_sentiment_model: String::new(),
            adv_narrative_model: String::new(),
            adv_tpm: String::new(),
            adv_context: String::new(),
            adv_daily_cap: String::new(),
            subreddits: vec![],
            sub_input: String::new(),
            keywords_map: std::collections::HashMap::new(),
            kw_input: String::new(),
            kw_sub_idx: 0,
            poll_interval: 60,
            alert_threshold: 0.85,
            analysis_mode_idx: 2, // Both
            notif_idx: 0,
            tg_token: String::new(),
            tg_chat: String::new(),
            discord_url: String::new(),
            tg_phase: 0,
            web_idx: 0,
            web_port: 7860,
            mode_idx: 0,
        }
    }

    pub fn run(&mut self) -> Result<Option<Config>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let result = self.event_loop(&mut terminal);
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<Option<Config>> {
        loop {
            terminal.draw(|f| self.render(f))?;

            if self.step == WizardStep::Done {
                return Ok(Some(self.build_config()));
            }

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match key.code {
                    KeyCode::Esc => {
                        if self.step == WizardStep::Welcome { return Ok(None); }
                        self.go_back();
                    }
                    KeyCode::Enter => self.handle_enter(),
                    KeyCode::Char(c) => self.handle_char(c),
                    KeyCode::Backspace => self.handle_backspace(),
                    KeyCode::Up => self.handle_up(),
                    KeyCode::Down => self.handle_down(),
                    KeyCode::Tab => {
                        if self.step == WizardStep::Subreddits && !self.subreddits.is_empty() {
                            self.subreddits.pop();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_char(&mut self, c: char) {
        match self.step {
            WizardStep::SetupMode | WizardStep::SimpleProvider |
            WizardStep::AnalysisMode | WizardStep::Notifications |
            WizardStep::WebUi => {}
            WizardStep::Subreddits => self.sub_input.push(c),
            WizardStep::Keywords => self.kw_input.push(c),
            _ => self.input.push(c),
        }
    }

    fn handle_backspace(&mut self) {
        match self.step {
            WizardStep::Subreddits => { self.sub_input.pop(); }
            WizardStep::Keywords => { self.kw_input.pop(); }
            _ => { self.input.pop(); }
        }
    }

    fn handle_up(&mut self) {
        match self.step {
            WizardStep::SetupMode => { if self.mode_idx > 0 { self.mode_idx -= 1; } }
            WizardStep::SimpleProvider => { if self.simple_provider_idx > 0 { self.simple_provider_idx -= 1; } }
            WizardStep::AnalysisMode => { if self.analysis_mode_idx > 0 { self.analysis_mode_idx -= 1; } }
            WizardStep::Notifications => { if self.notif_idx > 0 { self.notif_idx -= 1; } }
            WizardStep::WebUi => { if self.web_idx > 0 { self.web_idx -= 1; } }
            _ => {}
        }
    }

    fn handle_down(&mut self) {
        match self.step {
            WizardStep::SetupMode => { if self.mode_idx < 1 { self.mode_idx += 1; } }
            WizardStep::SimpleProvider => {
                let max = KnownProvider::simple_list().len() - 1;
                if self.simple_provider_idx < max { self.simple_provider_idx += 1; }
            }
            WizardStep::AnalysisMode => { if self.analysis_mode_idx < 2 { self.analysis_mode_idx += 1; } }
            WizardStep::Notifications => { if self.notif_idx < 3 { self.notif_idx += 1; } }
            WizardStep::WebUi => { if self.web_idx < 1 { self.web_idx += 1; } }
            _ => {}
        }
    }

    fn handle_enter(&mut self) {
        self.error_msg = None;
        match self.step.clone() {
            WizardStep::Welcome => self.step = WizardStep::SetupMode,

            WizardStep::SetupMode => {
                self.setup_mode = if self.mode_idx == 0 { SetupMode::Simple } else { SetupMode::Advanced };
                self.input.clear();
                self.step = WizardStep::SylviaKey;
            }

            WizardStep::SylviaKey => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some("Sylvia API key required. Get one at sylvia-api.com".into());
                    return;
                }
                self.sylvia_key = v;
                self.input.clear();
                self.step = match self.setup_mode {
                    SetupMode::Simple => WizardStep::SimpleProvider,
                    SetupMode::Advanced => WizardStep::AdvancedBaseUrl,
                };
            }

            // ── Simple path ──────────────────────────────────────────────────

            WizardStep::SimpleProvider => {
                let providers = KnownProvider::simple_list();
                let preset = providers[self.simple_provider_idx].preset();
                self.adv_base_url = preset.base_url.to_string();
                self.adv_sentiment_model = preset.default_sentiment_model.to_string();
                self.adv_narrative_model = preset.default_narrative_model.to_string();
                self.adv_tpm = preset.tpm_limit.to_string();
                self.adv_context = preset.context_window.to_string();
                self.adv_daily_cap = preset.daily_token_limit.map(|v| v.to_string()).unwrap_or_default();
                self.input.clear();
                // Ollama doesn't need an API key
                if providers[self.simple_provider_idx] == KnownProvider::Ollama {
                    self.simple_api_key = String::new();
                    self.step = WizardStep::Subreddits;
                } else {
                    self.step = WizardStep::SimpleApiKey;
                }
            }

            WizardStep::SimpleApiKey => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some("API key required for this provider.".into());
                    return;
                }
                self.simple_api_key = v;
                self.input.clear();
                self.step = WizardStep::Subreddits;
            }

            // ── Advanced path ────────────────────────────────────────────────

            WizardStep::AdvancedBaseUrl => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some("Base URL required (e.g. https://api.openai.com)".into());
                    return;
                }
                self.adv_base_url = v;
                self.input.clear();
                self.step = WizardStep::AdvancedApiKey;
            }

            WizardStep::AdvancedApiKey => {
                self.adv_api_key = self.input.trim().to_string();
                self.input.clear();
                self.step = WizardStep::AdvancedSentimentModel;
            }

            WizardStep::AdvancedSentimentModel => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some("Sentiment model name required".into());
                    return;
                }
                self.adv_sentiment_model = v;
                self.input.clear();
                self.step = WizardStep::AdvancedNarrativeModel;
            }

            WizardStep::AdvancedNarrativeModel => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some("Narrative model name required (can be same as sentiment)".into());
                    return;
                }
                self.adv_narrative_model = v;
                self.input.clear();
                self.step = WizardStep::AdvancedTpm;
            }

            WizardStep::AdvancedTpm => {
                let v = self.input.trim().to_string();
                self.adv_tpm = if v.is_empty() { "10000".to_string() } else { v };
                self.input.clear();
                self.step = WizardStep::AdvancedContext;
            }

            WizardStep::AdvancedContext => {
                let v = self.input.trim().to_string();
                self.adv_context = if v.is_empty() { "32000".to_string() } else { v };
                self.input.clear();
                self.step = WizardStep::AdvancedDailyCap;
            }

            WizardStep::AdvancedDailyCap => {
                self.adv_daily_cap = self.input.trim().to_string();
                self.input.clear();
                self.step = WizardStep::Subreddits;
            }

            // ── Shared steps ─────────────────────────────────────────────────

            WizardStep::Subreddits => {
                let sub = self.sub_input.trim().to_lowercase();
                let sub = sub.trim_start_matches("r/").to_string();
                if !sub.is_empty() {
                    if !self.subreddits.contains(&sub) {
                        self.subreddits.push(sub.clone());
                        self.keywords_map.insert(sub, vec![]);
                    }
                    self.sub_input.clear();
                } else if !self.subreddits.is_empty() {
                    self.kw_sub_idx = 0;
                    self.kw_input.clear();
                    self.step = WizardStep::Keywords;
                } else {
                    self.error_msg = Some("Add at least one subreddit.".into());
                }
            }

            WizardStep::Keywords => {
                let kw = self.kw_input.trim().to_string();
                let sub = self.subreddits[self.kw_sub_idx].clone();
                if !kw.is_empty() {
                    self.keywords_map.entry(sub).or_default().push(kw);
                    self.kw_input.clear();
                } else {
                    self.kw_sub_idx += 1;
                    self.kw_input.clear();
                    if self.kw_sub_idx >= self.subreddits.len() {
                        self.input = self.poll_interval.to_string();
                        self.step = WizardStep::PollInterval;
                    }
                }
            }

            WizardStep::PollInterval => {
                match self.input.trim().parse::<u64>() {
                    Ok(v) if v >= 5 => {
                        self.poll_interval = v;
                        self.input = format!("{:.2}", self.alert_threshold);
                        self.step = WizardStep::AlertThreshold;
                    }
                    _ => self.error_msg = Some("Enter a number >= 5 (seconds)".into()),
                }
            }

            WizardStep::AlertThreshold => {
                match self.input.trim().parse::<f32>() {
                    Ok(v) if (0.0..=1.0).contains(&v) => {
                        self.alert_threshold = v;
                        self.step = WizardStep::AnalysisMode;
                    }
                    _ => self.error_msg = Some("Enter a value between 0.0 and 1.0".into()),
                }
            }

            WizardStep::AnalysisMode => {
                self.step = WizardStep::Notifications;
            }

            WizardStep::Notifications => {
                match self.notif_idx {
                    0 => self.step = WizardStep::WebUi,
                    1 => { self.input.clear(); self.tg_phase = 0; self.step = WizardStep::TelegramSetup; }
                    2 => { self.input.clear(); self.step = WizardStep::DiscordSetup; }
                    3 => { self.input.clear(); self.tg_phase = 0; self.step = WizardStep::TelegramSetup; }
                    _ => {}
                }
            }

            WizardStep::TelegramSetup => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some(if self.tg_phase == 0 { "Bot token required" } else { "Chat ID required" }.into());
                    return;
                }
                if self.tg_phase == 0 {
                    self.tg_token = v;
                    self.tg_phase = 1;
                    self.input.clear();
                } else {
                    self.tg_chat = v;
                    self.input.clear();
                    if self.notif_idx == 3 {
                        self.step = WizardStep::DiscordSetup;
                    } else {
                        self.step = WizardStep::WebUi;
                    }
                }
            }

            WizardStep::DiscordSetup => {
                let v = self.input.trim().to_string();
                if v.is_empty() {
                    self.error_msg = Some("Discord webhook URL required".into());
                    return;
                }
                self.discord_url = v;
                self.input.clear();
                self.step = WizardStep::WebUi;
            }

            WizardStep::WebUi => {
                self.step = WizardStep::Summary;
            }

            WizardStep::Summary => {
                self.step = WizardStep::Done;
            }

            WizardStep::Done => {}
        }
    }

    fn go_back(&mut self) {
        self.error_msg = None;
        self.step = match &self.step {
            WizardStep::SetupMode => WizardStep::Welcome,
            WizardStep::SylviaKey => WizardStep::SetupMode,
            WizardStep::SimpleProvider | WizardStep::AdvancedBaseUrl => WizardStep::SylviaKey,
            WizardStep::SimpleApiKey => WizardStep::SimpleProvider,
            WizardStep::AdvancedApiKey => WizardStep::AdvancedBaseUrl,
            WizardStep::AdvancedSentimentModel => WizardStep::AdvancedApiKey,
            WizardStep::AdvancedNarrativeModel => WizardStep::AdvancedSentimentModel,
            WizardStep::AdvancedTpm => WizardStep::AdvancedNarrativeModel,
            WizardStep::AdvancedContext => WizardStep::AdvancedTpm,
            WizardStep::AdvancedDailyCap => WizardStep::AdvancedContext,
            WizardStep::Subreddits => WizardStep::SylviaKey,
            WizardStep::Keywords => WizardStep::Subreddits,
            WizardStep::PollInterval => WizardStep::Keywords,
            WizardStep::AlertThreshold => WizardStep::PollInterval,
            WizardStep::AnalysisMode => WizardStep::AlertThreshold,
            WizardStep::Notifications => WizardStep::AnalysisMode,
            WizardStep::TelegramSetup | WizardStep::DiscordSetup => WizardStep::Notifications,
            WizardStep::WebUi => WizardStep::Notifications,
            WizardStep::Summary => WizardStep::WebUi,
            _ => return,
        };
    }

    fn build_config(&self) -> Config {
        let api_key = if self.setup_mode == SetupMode::Simple {
            if self.simple_api_key.is_empty() { None } else { Some(self.simple_api_key.clone()) }
        } else {
            if self.adv_api_key.is_empty() { None } else { Some(self.adv_api_key.clone()) }
        };

        let tpm: u32 = self.adv_tpm.parse().unwrap_or(10_000);
        let context: u32 = self.adv_context.parse().unwrap_or(32_000);
        let daily_cap: Option<u32> = self.adv_daily_cap.parse().ok();

        let provider_type = if self.adv_base_url.contains("anthropic") {
            AiProviderType::Anthropic
        } else if self.adv_base_url.contains("localhost") || self.adv_base_url.contains("11434") {
            AiProviderType::Ollama
        } else {
            AiProviderType::OpenAI
        };

        let ai = AiConfig {
            provider_type,
            api_key,
            base_url: self.adv_base_url.clone(),
            sentiment_model: self.adv_sentiment_model.clone(),
            narrative_model: self.adv_narrative_model.clone(),
            tpm_limit: if tpm == 0 { u32::MAX } else { tpm },
            context_window: context,
            daily_token_limit: daily_cap,
            batch_size: 0, // auto-calculate
        };

        let _num_subs = self.subreddits.len();
        let subreddits = self.subreddits.iter().map(|sub| {
            SubredditConfig {
                name: sub.clone(),
                keywords: self.keywords_map.get(sub).cloned().unwrap_or_default(),
                poll_interval_secs: self.poll_interval,
                sentiment_alert_threshold: self.alert_threshold,
                narrative_interval_secs: 0, // auto-calculated from TPM
            }
        }).collect();

        let analysis_mode = match self.analysis_mode_idx {
            0 => AnalysisMode::Raw,
            1 => AnalysisMode::Narrative,
            _ => AnalysisMode::Both,
        };

        Config {
            sylvia: SylviaConfig {
                api_key: self.sylvia_key.clone(),
                base_url: "https://api.sylvia-api.com".into(),
                use_subreddit_endpoints: true,
                cost_per_request: 0.05,
                max_comments_per_poll: 100,
            },
            ai,
            tracking: TrackingConfig {
                subreddits,
                default_poll_interval_secs: self.poll_interval,
                default_alert_threshold: self.alert_threshold,
            },
            notifications: NotificationConfig {
                telegram: if !self.tg_token.is_empty() {
                    Some(TelegramConfig { bot_token: self.tg_token.clone(), chat_id: self.tg_chat.clone() })
                } else { None },
                discord: if !self.discord_url.is_empty() {
                    Some(DiscordConfig { webhook_url: self.discord_url.clone() })
                } else { None },
            },
            web: WebConfig {
                enabled: self.web_idx == 1,
                port: self.web_port,
            },
            analysis: AnalysisConfig {
                mode: analysis_mode,
                ..AnalysisConfig::default()
            },
            meta: MetaConfig::default(),
        }
    }

    // ── Rendering ─────────────────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        let area = f.area();
        f.render_widget(Block::default().style(Style::default().bg(BG)), area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(2)])
            .split(area);

        self.render_header(f, chunks[0]);
        self.render_body(f, chunks[1]);
        self.render_footer(f, chunks[2]);

        if let Some(err) = &self.error_msg.clone() {
            let err_area = Rect {
                x: area.x + 2,
                y: area.height.saturating_sub(3),
                width: area.width.saturating_sub(4),
                height: 1,
            };
            f.render_widget(
                Paragraph::new(format!(" ⚠  {} ", err))
                    .style(Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)),
                err_area,
            );
        }
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let mode_indicator = match self.setup_mode {
            SetupMode::Simple => Span::styled(" simple ", Style::default().fg(Color::Black).bg(YELLOW)),
            SetupMode::Advanced => Span::styled(" advanced ", Style::default().fg(Color::Black).bg(ACCENT)),
        };
        let line = if self.step == WizardStep::Welcome || self.step == WizardStep::SetupMode {
            Line::from(vec![
                Span::styled("OPEN", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("MAVEN", Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
                Span::styled("  setup wizard", Style::default().fg(DIM)),
            ])
        } else {
            Line::from(vec![
                Span::styled("OPEN", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("MAVEN", Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
                Span::styled("  ", Style::default()),
                mode_indicator,
            ])
        };
        let w = Paragraph::new(line).block(
            Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(DIM))
        );
        f.render_widget(w, area.inner(Margin { vertical: 0, horizontal: 2 }));
    }

    fn render_footer(&self, f: &mut Frame, area: Rect) {
        let hint = match self.step {
            WizardStep::Welcome | WizardStep::SetupMode | WizardStep::SimpleProvider |
            WizardStep::AnalysisMode | WizardStep::Notifications | WizardStep::WebUi
                => "[↑↓] select  [enter] confirm  [esc] back",
            WizardStep::Subreddits => "[enter] add  [enter on empty] done  [tab] remove last",
            WizardStep::Keywords => "[enter] add  [enter on empty] next subreddit",
            WizardStep::Summary => "[enter] save & launch  [esc] back",
            _ => "[enter] confirm  [esc] back",
        };
        f.render_widget(
            Paragraph::new(hint).style(Style::default().fg(DIM)),
            area.inner(Margin { vertical: 0, horizontal: 2 }),
        );
    }

    fn render_body(&mut self, f: &mut Frame, area: Rect) {
        let padded = area.inner(Margin { vertical: 1, horizontal: 3 });
        match self.step.clone() {
            WizardStep::Welcome => self.render_welcome(f, padded),
            WizardStep::SetupMode => self.render_setup_mode(f, padded),
            WizardStep::SylviaKey => self.render_text_input(f, padded,
                "Data Source — Sylvia API",
                "Get your key at  sylvia-api.com  •  starts with syl_",
                true, &self.input.clone()),
            WizardStep::SimpleProvider => self.render_simple_provider(f, padded),
            WizardStep::SimpleApiKey => {
                let hint = format!("API key for {}", KnownProvider::simple_list()[self.simple_provider_idx].preset().name);
                self.render_text_input(f, padded, "AI Provider — API Key", &hint, true, &self.input.clone())
            }
            WizardStep::AdvancedBaseUrl => self.render_text_input(f, padded,
                "Advanced — Base URL",
                "e.g. https://api.openai.com  or  http://localhost:11434",
                false, &self.input.clone()),
            WizardStep::AdvancedApiKey => self.render_text_input(f, padded,
                "Advanced — API Key",
                "Leave empty for Ollama / local endpoints",
                true, &self.input.clone()),
            WizardStep::AdvancedSentimentModel => self.render_text_input(f, padded,
                "Advanced — Sentiment Model",
                "Used for per-comment scoring. Use a fast, cheap model. e.g. llama-3.1-8b-instant",
                false, &self.input.clone()),
            WizardStep::AdvancedNarrativeModel => self.render_text_input(f, padded,
                "Advanced — Narrative Model",
                "Used for narrative + predictions. Use a smarter model. e.g. llama-3.3-70b-versatile",
                false, &self.input.clone()),
            WizardStep::AdvancedTpm => self.render_text_input(f, padded,
                "Advanced — Tokens Per Minute Limit",
                "Check your provider dashboard. Enter 0 for unlimited (Ollama). e.g. 12000",
                false, &self.input.clone()),
            WizardStep::AdvancedContext => self.render_text_input(f, padded,
                "Advanced — Context Window (tokens)",
                "Max tokens your model can process at once. e.g. 128000",
                false, &self.input.clone()),
            WizardStep::AdvancedDailyCap => self.render_text_input(f, padded,
                "Advanced — Daily Token Cap",
                "Leave empty if no daily limit. e.g. Groq free = 100000",
                false, &self.input.clone()),
            WizardStep::Subreddits => self.render_subreddits(f, padded),
            WizardStep::Keywords => self.render_keywords(f, padded),
            WizardStep::PollInterval => self.render_text_input(f, padded,
                "Poll Interval",
                "How often to check each subreddit (seconds, min 5). 60s recommended. Lower = more signal, higher Sylvia cost.",
                false, &self.input.clone()),
            WizardStep::AlertThreshold => self.render_text_input(f, padded,
                "Alert Threshold",
                "Negative sentiment score to trigger raw alerts (0.0–1.0). 0.85 is a good default.",
                false, &self.input.clone()),
            WizardStep::AnalysisMode => self.render_analysis_mode(f, padded),
            WizardStep::Notifications => self.render_notifications(f, padded),
            WizardStep::TelegramSetup => {
                let (title, hint) = if self.tg_phase == 0 {
                    ("Telegram — Bot Token", "From @BotFather")
                } else {
                    ("Telegram — Chat ID", "Use @userinfobot to find yours")
                };
                self.render_text_input(f, padded, title, hint, false, &self.input.clone())
            }
            WizardStep::DiscordSetup => self.render_text_input(f, padded,
                "Discord — Webhook URL",
                "https://discord.com/api/webhooks/...",
                false, &self.input.clone()),
            WizardStep::WebUi => self.render_webui(f, padded),
            WizardStep::Summary => self.render_summary(f, padded),
            WizardStep::Done => {}
        }
    }

    fn render_welcome(&self, f: &mut Frame, area: Rect) {
        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled("Welcome to openmaven.", Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD))]),
            Line::from(""),
            Line::from(vec![Span::styled("Reddit intelligence platform — real-time sentiment, narratives, and predictions.", Style::default().fg(DIM))]),
            Line::from(""),
            Line::from(vec![Span::styled("You will need:", Style::default().fg(BRIGHT))]),
            Line::from(vec![Span::styled("  • A Sylvia API key    →  sylvia-api.com", Style::default().fg(DIM))]),
            Line::from(vec![Span::styled("  • An AI API key      →  Groq, Anthropic, OpenAI, HuggingFace, or local Ollama", Style::default().fg(DIM))]),
            Line::from(vec![Span::styled("  • Subreddits to track", Style::default().fg(DIM))]),
            Line::from(""),
            Line::from(vec![Span::styled("Setup takes about 2 minutes.", Style::default().fg(DIM))]),
        ];
        f.render_widget(Paragraph::new(lines).block(panel("")).wrap(Wrap { trim: false }), area);
    }

    fn render_setup_mode(&mut self, f: &mut Frame, area: Rect) {
        let options = vec![
            ("  Simple    ", "Pick from a known provider. Everything configured automatically."),
            ("  Advanced  ", "Set your own base URL, models, context window, TPM limits."),
        ];
        let items: Vec<ListItem> = options.iter().enumerate().map(|(i, (label, desc))| {
            if i == self.mode_idx {
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled("▶ ", Style::default().fg(ACCENT)),
                        Span::styled(*label, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(*desc, Style::default().fg(BRIGHT)),
                    ]),
                ])
            } else {
                ListItem::new(vec![
                    Line::from(vec![Span::styled(format!("  {}", label), Style::default().fg(DIM))]),
                ])
            }
        }).collect();
        f.render_widget(List::new(items).block(panel("Setup Mode")), area);
    }

    fn render_simple_provider(&mut self, f: &mut Frame, area: Rect) {
        let providers = KnownProvider::simple_list();
        let items: Vec<ListItem> = providers.iter().enumerate().map(|(i, p)| {
            let preset = p.preset();
            if i == self.simple_provider_idx {
                let tpm_str = if preset.tpm_limit == u32::MAX {
                    "unlimited".to_string()
                } else {
                    format!("{} TPM", preset.tpm_limit)
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled("▶ ", Style::default().fg(ACCENT)),
                        Span::styled(preset.name, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("  sentiment: {}  |  narrative: {}  |  {}", 
                            preset.default_sentiment_model,
                            preset.default_narrative_model,
                            tpm_str,
                        ), Style::default().fg(BRIGHT)),
                    ]),
                ])
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {}", preset.name), Style::default().fg(DIM)),
                ]))
            }
        }).collect();
        f.render_widget(List::new(items).block(panel("AI Provider")), area);
    }

    fn render_text_input(&self, f: &mut Frame, area: Rect, title: &str, hint: &str, masked: bool, value: &str) {
        let display = if masked && !value.is_empty() { "•".repeat(value.len()) } else { value.to_string() };
        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(hint, Style::default().fg(DIM))]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(format!("{}_", display), Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
            ]),
        ];
        f.render_widget(Paragraph::new(lines).block(panel(title)).wrap(Wrap { trim: false }), area);
    }

    fn render_subreddits(&self, f: &mut Frame, area: Rect) {
        let mut lines = vec![
            Line::from(vec![Span::styled("Subreddits to track:", Style::default().fg(DIM))]),
            Line::from(""),
        ];
        for sub in &self.subreddits {
            lines.push(Line::from(vec![
                Span::styled("  ✓ r/", Style::default().fg(ACCENT)),
                Span::styled(sub.clone(), Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
            ]));
        }
        if self.subreddits.is_empty() {
            lines.push(Line::from(vec![Span::styled("  (none yet)", Style::default().fg(DIM))]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("  r/{}_ ", self.sub_input), Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
        ]));
        f.render_widget(Paragraph::new(lines).block(panel("Subreddits")).wrap(Wrap { trim: false }), area);
    }

    fn render_keywords(&self, f: &mut Frame, area: Rect) {
        let sub = self.subreddits.get(self.kw_sub_idx).map(|s| s.as_str()).unwrap_or("?");
        let existing = self.keywords_map.get(sub).cloned().unwrap_or_default();
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Keywords for r/", Style::default().fg(DIM)),
                Span::styled(sub, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  ({}/{})", self.kw_sub_idx + 1, self.subreddits.len()), Style::default().fg(DIM)),
            ]),
            Line::from(vec![Span::styled("Empty enter = skip / capture all comments", Style::default().fg(DIM))]),
            Line::from(""),
        ];
        for kw in &existing {
            lines.push(Line::from(vec![
                Span::styled("  # ", Style::default().fg(ACCENT)),
                Span::styled(kw.clone(), Style::default().fg(BRIGHT)),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("  {}_ ", self.kw_input), Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
        ]));
        f.render_widget(Paragraph::new(lines).block(panel("Keywords")).wrap(Wrap { trim: false }), area);
    }

    fn render_analysis_mode(&mut self, f: &mut Frame, area: Rect) {
        let options = vec![
            ("  Raw only      ", "Per-comment alerts when sentiment crosses your threshold. Low token usage."),
            ("  Narrative only", "Clustered topic summaries + predictions. No per-comment alerts."),
            ("  Both          ", "Raw alerts + narrative summaries + predictions. Recommended."),
        ];
        let items: Vec<ListItem> = options.iter().enumerate().map(|(i, (label, desc))| {
            if i == self.analysis_mode_idx {
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled("▶ ", Style::default().fg(ACCENT)),
                        Span::styled(*label, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("  {}", desc), Style::default().fg(BRIGHT)),
                    ]),
                ])
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {}", label), Style::default().fg(DIM)),
                ]))
            }
        }).collect();
        f.render_widget(List::new(items).block(panel("Analysis Mode")), area);
    }

    fn render_notifications(&mut self, f: &mut Frame, area: Rect) {
        let options = ["  None", "  Telegram", "  Discord", "  Both Telegram & Discord"];
        let items: Vec<ListItem> = options.iter().enumerate().map(|(i, o)| {
            if i == self.notif_idx {
                ListItem::new(format!("▶ {}", o.trim()))
                    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            } else {
                ListItem::new(o.to_string()).style(Style::default().fg(DIM))
            }
        }).collect();
        f.render_widget(List::new(items).block(panel("Notifications")), area);
    }

    fn render_webui(&mut self, f: &mut Frame, area: Rect) {
        let options = [
            "  No   (CLI / notifications only)",
            "  Yes  (local dashboard at localhost:7860)",
        ];
        let items: Vec<ListItem> = options.iter().enumerate().map(|(i, o)| {
            if i == self.web_idx {
                ListItem::new(format!("▶ {}", o.trim()))
                    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            } else {
                ListItem::new(o.to_string()).style(Style::default().fg(DIM))
            }
        }).collect();
        f.render_widget(List::new(items).block(panel("Web Dashboard")), area);
    }

    fn render_summary(&self, f: &mut Frame, area: Rect) {
        let cfg = self.build_config();
        let num_subs = cfg.tracking.subreddits.len();
        let narrative_interval = cfg.ai.narrative_interval_secs(num_subs);
        let batch_size = cfg.ai.effective_batch_size();
        let daily_tokens = cfg.ai.estimate_daily_tokens(num_subs, cfg.tracking.default_poll_interval_secs);
        let daily_sylvia = cfg.ai.estimate_daily_sylvia_cost(num_subs, cfg.tracking.default_poll_interval_secs, cfg.sylvia.cost_per_request);

        let cap_warn = if let Some(cap) = cfg.ai.daily_token_limit {
            if daily_tokens > cap as u64 {
                format!("⚠  Daily cap: {} — est. {} tokens/day. Consider higher poll interval.", cap, daily_tokens)
            } else {
                format!("✓  Est. {} tokens/day — within daily cap of {}", daily_tokens, cap)
            }
        } else {
            format!("   Est. {} tokens/day (no daily cap)", daily_tokens)
        };

        let mode_str = match cfg.analysis.mode {
            AnalysisMode::Raw => "raw alerts only",
            AnalysisMode::Narrative => "narrative + predictions only",
            AnalysisMode::Both => "raw + narrative + predictions",
        };

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled("Ready to launch. Here's your setup:", Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD))]),
            Line::from(""),
            Line::from(vec![Span::styled(format!("  Sylvia key       {}***", &cfg.sylvia.api_key[..8.min(cfg.sylvia.api_key.len())]), Style::default().fg(DIM))]),
            Line::from(vec![Span::styled(format!("  Sentiment model  {}", cfg.ai.sentiment_model), Style::default().fg(BRIGHT))]),
            Line::from(vec![Span::styled(format!("  Narrative model  {}", cfg.ai.narrative_model), Style::default().fg(BRIGHT))]),
            Line::from(vec![Span::styled(format!("  Batch size       {} (auto)", batch_size), Style::default().fg(DIM))]),
            Line::from(vec![Span::styled(format!("  Narrative every  ~{}s (auto from TPM)", narrative_interval), Style::default().fg(DIM))]),
            Line::from(""),
            Line::from(vec![Span::styled(format!("  Tracking {} subreddits every {}s", num_subs, cfg.tracking.default_poll_interval_secs), Style::default().fg(BRIGHT))]),
            Line::from(vec![Span::styled(format!("  Analysis         {}", mode_str), Style::default().fg(DIM))]),
            Line::from(vec![Span::styled(
                format!("  Sylvia cost      ~${:.2}/day  (~${:.0}/month)",
                    daily_sylvia, daily_sylvia * 30.0),
                Style::default().fg(if daily_sylvia > 50.0 { ACCENT } else { DIM })
            )]),
            Line::from(vec![Span::styled(
                format!("  → At 60s interval: ~${:.2}/day  |  5min: ~${:.2}/day  |  15min: ~${:.2}/day",
                    cfg.ai.estimate_daily_sylvia_cost(num_subs, 60, cfg.sylvia.cost_per_request),
                    cfg.ai.estimate_daily_sylvia_cost(num_subs, 300, cfg.sylvia.cost_per_request),
                    cfg.ai.estimate_daily_sylvia_cost(num_subs, 900, cfg.sylvia.cost_per_request),
                ),
                Style::default().fg(DIM)
            )]),
            Line::from(vec![Span::styled(format!("  {}", cap_warn), Style::default().fg(if cap_warn.starts_with('⚠') { ACCENT } else { DIM }))]),
            Line::from(""),
            Line::from(vec![Span::styled("A full diagnostics report will be sent to your notification channel on first launch.", Style::default().fg(DIM))]),
            Line::from(""),
            Line::from(vec![Span::styled("Press enter to save and start.", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))]),
        ];
        f.render_widget(Paragraph::new(lines).block(panel("Summary")).wrap(Wrap { trim: false }), area);
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .style(Style::default().bg(BG))
}
