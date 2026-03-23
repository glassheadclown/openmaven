use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Chart, Dataset, GraphType,
        List, ListItem, ListState, Paragraph, Tabs, Wrap,
    },
    Frame, Terminal,
};
use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use crate::store::{Store, StoredResult};
use crate::trends::{TrendSummary, TrendTracker};

const ACCENT: Color = Color::Rgb(255, 69, 69);
const GREEN: Color = Color::Rgb(61, 220, 132);
const YELLOW: Color = Color::Rgb(255, 209, 102);
const DIM: Color = Color::Rgb(80, 80, 100);
const BRIGHT: Color = Color::Rgb(240, 240, 248);
const BG: Color = Color::Rgb(10, 10, 14);
const PANEL: Color = Color::Rgb(16, 16, 22);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Live,
    Trends,
    Stats,
}

impl Tab {
    fn titles() -> Vec<&'static str> {
        vec!["  Live Feed  ", "  Trends  ", "  Stats  "]
    }
    fn index(&self) -> usize {
        match self { Tab::Live => 0, Tab::Trends => 1, Tab::Stats => 2 }
    }
}

pub struct LiveDashboard {
    store: Arc<Store>,
    trends: Arc<TrendTracker>,
    results: Arc<RwLock<VecDeque<StoredResult>>>,
    trend_summaries: Arc<RwLock<Vec<TrendSummary>>>,
    active_tab: Tab,
    list_state: ListState,
    #[allow(dead_code)]
    selected_sub: Option<String>,
    paused: bool,
}

impl LiveDashboard {
    pub fn new(store: Arc<Store>, trends: Arc<TrendTracker>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            store,
            trends,
            results: Arc::new(RwLock::new(VecDeque::with_capacity(500))),
            trend_summaries: Arc::new(RwLock::new(vec![])),
            active_tab: Tab::Live,
            list_state,
            selected_sub: None,
            paused: false,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Spawn background refresh task
        let results_ref = Arc::clone(&self.results);
        let trend_ref = Arc::clone(&self.trend_summaries);
        let store_ref = Arc::clone(&self.store);
        let trends_src = Arc::clone(&self.trends);

        tokio::spawn(async move {
            loop {
                // Fetch latest from DB
                if let Ok(fresh) = store_ref.recent(200).await {
                    let mut lock = results_ref.write().await;
                    lock.clear();
                    for r in fresh {
                        lock.push_back(r);
                    }
                }
                // Refresh trends
                trends_src.snapshot_all().await;
                let summaries = trends_src.summary().await;
                let mut t = trend_ref.write().await;
                *t = summaries;

                sleep(Duration::from_secs(3)).await;
            }
        });

        loop {
            let results_snapshot = {
                let lock = self.results.read().await;
                lock.iter().cloned().collect::<Vec<_>>()
            };
            let trends_snapshot = {
                let lock = self.trend_summaries.read().await;
                lock.clone()
            };

            terminal.draw(|f| self.render(f, &results_snapshot, &trends_snapshot))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press { continue; }
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char(' ') => self.paused = !self.paused,
                        KeyCode::Tab => {
                            self.active_tab = match self.active_tab {
                                Tab::Live => Tab::Trends,
                                Tab::Trends => Tab::Stats,
                                Tab::Stats => Tab::Live,
                            };
                        }
                        KeyCode::BackTab => {
                            self.active_tab = match self.active_tab {
                                Tab::Live => Tab::Stats,
                                Tab::Trends => Tab::Live,
                                Tab::Stats => Tab::Trends,
                            };
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let max = results_snapshot.len().saturating_sub(1);
                            let i = self.list_state.selected().unwrap_or(0);
                            self.list_state.select(Some((i + 1).min(max)));
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let i = self.list_state.selected().unwrap_or(0);
                            self.list_state.select(Some(i.saturating_sub(1)));
                        }
                        _ => {}
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }

    fn render(&mut self, f: &mut Frame, results: &[StoredResult], trends: &[TrendSummary]) {
        let area = f.area();
        f.render_widget(Block::default().style(Style::default().bg(BG)), area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // header + tabs
                Constraint::Min(0),     // content
                Constraint::Length(1),  // status bar
            ])
            .split(area);

        self.render_header(f, chunks[0]);

        match self.active_tab {
            Tab::Live => self.render_live(f, chunks[1], results),
            Tab::Trends => self.render_trends(f, chunks[1], trends),
            Tab::Stats => self.render_stats(f, chunks[1], results),
        }

        self.render_statusbar(f, chunks[2], results);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(16), Constraint::Min(0)])
            .split(area);

        let logo = Paragraph::new(Line::from(vec![
            Span::styled("OPEN", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("MAVEN", Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
        ])).alignment(Alignment::Center)
          .block(Block::default().borders(Borders::RIGHT).border_style(Style::default().fg(DIM)));
        f.render_widget(logo, chunks[0]);

        let tab_titles: Vec<Line> = Tab::titles().iter().map(|t| Line::from(*t)).collect();
        let tabs = Tabs::new(tab_titles)
            .select(self.active_tab.index())
            .style(Style::default().fg(DIM))
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .divider(Span::styled("|", Style::default().fg(DIM)));
        f.render_widget(tabs, chunks[1]);
    }

    fn render_live(&mut self, f: &mut Frame, area: Rect, results: &[StoredResult]) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(32)])
            .split(area);

        // Feed list
        let items: Vec<ListItem> = results.iter().map(|r| {
            let score_color = if r.score > 0.2 { GREEN }
                             else if r.score < -0.2 { ACCENT }
                             else { YELLOW };
            let label_char = match r.label.as_str() {
                "positive" => "▲",
                "negative" => "▼",
                _ => "●",
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{} ", label_char), Style::default().fg(score_color)),
                    Span::styled(format!("r/{:<18}", r.subreddit), Style::default().fg(ACCENT)),
                    Span::styled(format!(" {:>6.2}  ", r.score), Style::default().fg(score_color).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("u/{}", r.author), Style::default().fg(DIM)),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        r.body.chars().take(80).collect::<String>().replace('\n', " "),
                        Style::default().fg(BRIGHT),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(format!("  ↳ {}", r.summary), Style::default().fg(DIM)),
                ]),
            ])
        }).collect();

        let list = List::new(items)
            .block(panel_block("Live Comments"))
            .highlight_style(Style::default().bg(PANEL).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, cols[0], &mut self.list_state);

        // Detail panel for selected item
        if let Some(idx) = self.list_state.selected() {
            if let Some(r) = results.get(idx) {
                self.render_detail(f, cols[1], r);
            }
        }
    }

    fn render_detail(&self, f: &mut Frame, area: Rect, r: &StoredResult) {
        let score_color = if r.score > 0.2 { GREEN }
                         else if r.score < -0.2 { ACCENT }
                         else { YELLOW };

        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("r/{}", r.subreddit), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled(format!("u/{}", r.author), Style::default().fg(DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Score   ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{:+.3}", r.score),
                    Style::default().fg(score_color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Label   ", Style::default().fg(DIM)),
                Span::styled(&r.label, Style::default().fg(score_color)),
            ]),
            Line::from(vec![
                Span::styled(format!("Conf    {:.0}%", r.confidence * 100.0), Style::default().fg(DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Summary", Style::default().fg(DIM)),
            ]),
            Line::from(vec![
                Span::styled(&r.summary, Style::default().fg(BRIGHT)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Body", Style::default().fg(DIM)),
            ]),
            Line::from(vec![
                Span::styled(
                    r.body.chars().take(400).collect::<String>(),
                    Style::default().fg(BRIGHT),
                ),
            ]),
        ];

        let w = Paragraph::new(text)
            .block(panel_block("Detail"))
            .wrap(Wrap { trim: false });
        f.render_widget(w, area);
    }

    fn render_trends(&self, f: &mut Frame, area: Rect, trends: &[TrendSummary]) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                trends.iter().map(|_| Constraint::Ratio(1, trends.len().max(1) as u32)).collect::<Vec<_>>()
            )
            .split(area);

        for (i, trend) in trends.iter().enumerate() {
            if i >= chunks.len() { break; }

            // Build chart data from snapshots
            let data: Vec<(f64, f64)> = trend.snapshots.iter().enumerate()
                .map(|(i, p)| (i as f64, p.score as f64))
                .collect();

            if data.len() < 2 {
                let placeholder = Paragraph::new(format!(
                    "  r/{} / {}  —  not enough data yet (need 2+ snapshots)",
                    trend.subreddit, trend.keyword
                )).style(Style::default().fg(DIM))
                  .block(panel_block(""));
                f.render_widget(placeholder, chunks[i]);
                continue;
            }

            let score_color = trend.current_score.map(|s| {
                if s > 0.15 { GREEN } else if s < -0.15 { ACCENT } else { YELLOW }
            }).unwrap_or(DIM);

            let dataset = Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(score_color))
                .data(&data);

            let y_min = data.iter().map(|(_, y)| y).cloned().fold(f64::INFINITY, f64::min).max(-1.0) - 0.1;
            let y_max = data.iter().map(|(_, y)| y).cloned().fold(f64::NEG_INFINITY, f64::max).min(1.0) + 0.1;

            let title = format!(
                " r/{} / {}  {}  score: {}  vol: {} ",
                trend.subreddit,
                trend.keyword,
                trend.direction,
                trend.current_score.map(|s| format!("{:+.2}", s)).unwrap_or_else(|| "—".into()),
                trend.volume,
            );

            let chart = Chart::new(vec![dataset])
                .block(panel_block(&title))
                .x_axis(Axis::default()
                    .style(Style::default().fg(DIM))
                    .bounds([0.0, data.len() as f64]))
                .y_axis(Axis::default()
                    .style(Style::default().fg(DIM))
                    .bounds([y_min, y_max])
                    .labels(vec![
                        Span::styled(format!("{:.1}", y_min), Style::default().fg(DIM)),
                        Span::styled("0.0", Style::default().fg(DIM)),
                        Span::styled(format!("{:.1}", y_max), Style::default().fg(DIM)),
                    ]));

            f.render_widget(chart, chunks[i]);
        }

        if trends.is_empty() {
            let w = Paragraph::new("  No trend data yet. Trends populate as comments are analyzed.")
                .style(Style::default().fg(DIM))
                .block(panel_block("Keyword Trends"));
            f.render_widget(w, area);
        }
    }

    fn render_stats(&self, f: &mut Frame, area: Rect, results: &[StoredResult]) {
        let total = results.len();
        let pos = results.iter().filter(|r| r.label == "positive").count();
        let neg = results.iter().filter(|r| r.label == "negative").count();
        let neu = results.iter().filter(|r| r.label == "neutral").count();
        let avg = if total > 0 {
            results.iter().map(|r| r.score as f64).sum::<f64>() / total as f64
        } else { 0.0 };

        // Per-subreddit breakdown
        let mut by_sub: std::collections::HashMap<&str, (usize, f64)> = std::collections::HashMap::new();
        for r in results {
            let e = by_sub.entry(&r.subreddit).or_insert((0, 0.0));
            e.0 += 1;
            e.1 += r.score as f64;
        }
        let mut sub_stats: Vec<(&str, usize, f64)> = by_sub.iter()
            .map(|(s, (n, sum))| (*s, *n, sum / *n as f64))
            .collect();
        sub_stats.sort_by(|a, b| b.1.cmp(&a.1));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(0)])
            .split(area);

        // Global stats
        let global = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("  Total analyzed   "), Style::default().fg(DIM)),
                Span::styled(format!("{}", total), Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled(format!("  Average score    "), Style::default().fg(DIM)),
                Span::styled(
                    format!("{:+.3}", avg),
                    Style::default().fg(if avg > 0.1 { GREEN } else if avg < -0.1 { ACCENT } else { YELLOW })
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(format!("  Positive         "), Style::default().fg(DIM)),
                Span::styled(
                    format!("{}  ({:.1}%)", pos, if total > 0 { pos as f64 / total as f64 * 100.0 } else { 0.0 }),
                    Style::default().fg(GREEN),
                ),
            ]),
            Line::from(vec![
                Span::styled(format!("  Negative         "), Style::default().fg(DIM)),
                Span::styled(
                    format!("{}  ({:.1}%)", neg, if total > 0 { neg as f64 / total as f64 * 100.0 } else { 0.0 }),
                    Style::default().fg(ACCENT),
                ),
            ]),
            Line::from(vec![
                Span::styled(format!("  Neutral          "), Style::default().fg(DIM)),
                Span::styled(
                    format!("{}  ({:.1}%)", neu, if total > 0 { neu as f64 / total as f64 * 100.0 } else { 0.0 }),
                    Style::default().fg(YELLOW),
                ),
            ]),
        ]).block(panel_block("Global"));
        f.render_widget(global, chunks[0]);

        // Per-subreddit
        let items: Vec<ListItem> = sub_stats.iter().map(|(sub, n, avg)| {
            let color = if *avg > 0.1 { GREEN } else if *avg < -0.1 { ACCENT } else { YELLOW };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  r/{:<25}", sub), Style::default().fg(ACCENT)),
                Span::styled(format!("{:>5} comments  ", n), Style::default().fg(DIM)),
                Span::styled(format!("avg {:+.3}", avg), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            ]))
        }).collect();

        let list = List::new(items).block(panel_block("By Subreddit"));
        f.render_widget(list, chunks[1]);
    }

    fn render_statusbar(&self, f: &mut Frame, area: Rect, results: &[StoredResult]) {
        let status = if self.paused { "PAUSED" } else { "LIVE" };
        let status_color = if self.paused { YELLOW } else { GREEN };
        let w = Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", status), Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("  {}  comments", results.len()),
                Style::default().fg(DIM),
            ),
            Span::styled(
                "  [tab] switch  [↑↓/jk] scroll  [space] pause  [q] quit",
                Style::default().fg(DIM),
            ),
        ])).style(Style::default().bg(PANEL));
        f.render_widget(w, area);
    }
}

fn panel_block(title: &str) -> Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(Color::Rgb(180, 180, 200)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .style(Style::default().bg(BG))
}
