//! Terminal User Interface for Saorsa WebRTC CLI

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::{
    io::{self, Stdout},
    sync::Arc,
    time::{Duration, Instant},
};

use saorsa_webrtc_core::{prelude::*, types::CallId};

/// Display mode for video
#[derive(Debug, Clone, Copy)]
pub enum DisplayMode {
    /// Sixel graphics (best quality)
    Sixel,
    /// ASCII art fallback
    Ascii,
    /// No video display
    None,
}

/// Terminal UI state
pub struct TerminalUI {
    display_mode: DisplayMode,
    terminal: Terminal<CrosstermBackend<Stdout>>,
    start_time: Instant,
    stats: ConnectionStats,
    muted: bool,
    video_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    pub rtt_ms: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub fps: Option<u32>,
    pub packets_lost: Option<u32>,
    pub packets_sent: Option<u32>,
}

/// Static UI drawing function for closures
fn draw_ui_static(
    f: &mut Frame,
    display_mode: DisplayMode,
    stats: ConnectionStats,
    muted: bool,
    video_enabled: bool,
    start_time: Instant,
) {
    let size = f.size();

    // Split the screen vertically
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Video area
            Constraint::Length(3), // Stats
            Constraint::Length(3), // Controls
        ])
        .split(size);

    // Video display area
    draw_video_area_static(f, chunks[0], display_mode);

    // Statistics area
    draw_stats_area_static(f, chunks[1], stats, start_time);

    // Controls area
    draw_controls_area_static(f, chunks[2], muted, video_enabled);
}

/// Draw the video display area (static)
fn draw_video_area_static(f: &mut Frame, area: Rect, display_mode: DisplayMode) {
    let block = Block::default()
        .title("🎥 Video Call")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let content = match display_mode {
        DisplayMode::Sixel => {
            // TODO: Implement Sixel rendering
            vec![Line::from(vec![
                Span::styled("Sixel video display", Style::default().fg(Color::Green)),
                Span::raw(" (placeholder)"),
            ])]
        }
        DisplayMode::Ascii => {
            // TODO: Implement ASCII art rendering
            vec![
                Line::from("   .-\"\"\"-.   "),
                Line::from("  /       \\  "),
                Line::from(" |         | "),
                Line::from("  \\       /  "),
                Line::from("   '-----'   "),
                Line::from("    (ᵔᴥᵔ)     "),
            ]
        }
        DisplayMode::None => {
            vec![Line::from(vec![Span::styled(
                "Video disabled",
                Style::default().fg(Color::Yellow),
            )])]
        }
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(paragraph, area);
}

/// Draw the statistics area (static)
fn draw_stats_area_static(f: &mut Frame, area: Rect, stats: ConnectionStats, start_time: Instant) {
    let block = Block::default()
        .title("📊 Statistics")
        .borders(Borders::ALL);

    let stats_text = vec![
        Line::from(format!(
            "RTT: {}ms | Bitrate: {}kbps | FPS: {}",
            stats.rtt_ms.unwrap_or(0),
            stats.bitrate_kbps.unwrap_or(0),
            stats.fps.unwrap_or(0)
        )),
        Line::from(format!(
            "Packets: Sent {} | Lost {}",
            stats.packets_sent.unwrap_or(0),
            stats.packets_lost.unwrap_or(0)
        )),
        Line::from(format!(
            "Duration: {:.1}s",
            start_time.elapsed().as_secs_f32()
        )),
    ];

    let paragraph = Paragraph::new(stats_text).block(block);
    f.render_widget(paragraph, area);
}

/// Draw the controls area (static)
fn draw_controls_area_static(f: &mut Frame, area: Rect, muted: bool, video_enabled: bool) {
    let block = Block::default().title("🎮 Controls").borders(Borders::ALL);

    let controls = vec![Line::from(vec![
        Span::styled(
            "(q/Esc)",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Quit | "),
        Span::styled(
            "(m)",
            Style::default().fg(if muted { Color::Red } else { Color::Green }),
        ),
        Span::raw(" Mute | "),
        Span::styled(
            "(v)",
            Style::default().fg(if video_enabled {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
        Span::raw(" Video | "),
        Span::styled("(s)", Style::default().fg(Color::Blue)),
        Span::raw(" Stats | "),
        Span::styled("(h)", Style::default().fg(Color::Blue)),
        Span::raw(" Help"),
    ])];

    let paragraph = Paragraph::new(controls).block(block);
    f.render_widget(paragraph, area);
}

impl TerminalUI {
    /// Create a new terminal UI
    pub fn new(display_mode: DisplayMode) -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(Self {
            display_mode,
            terminal,
            start_time: Instant::now(),
            stats: ConnectionStats::default(),
            muted: false,
            video_enabled: true,
        })
    }

    /// Run the terminal UI main loop
    pub async fn run(
        &mut self,
        _service: Arc<WebRtcService<PeerIdentityString, AntQuicTransport>>,
        _call_id: CallId,
    ) -> Result<()> {
        loop {
            // Handle input
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('m') => {
                            self.muted = !self.muted;
                            // TODO: service.toggle_audio_mute(&call_id).await?;
                        }
                        KeyCode::Char('v') => {
                            self.video_enabled = !self.video_enabled;
                            // TODO: service.toggle_video(&call_id).await?;
                        }
                        KeyCode::Char('s') => {
                            // Show detailed stats
                        }
                        KeyCode::Char('h') => {
                            // Show help
                        }
                        _ => {}
                    }
                }
            }

            // Update stats
            self.update_stats().await;

            // Render UI
            let stats = self.stats.clone();
            let muted = self.muted;
            let video_enabled = self.video_enabled;
            let start_time = self.start_time;
            let display_mode = self.display_mode;
            self.terminal.draw(|f| {
                draw_ui_static(
                    f,
                    display_mode,
                    stats.clone(),
                    muted,
                    video_enabled,
                    start_time,
                )
            })?;

            // Small delay to prevent excessive CPU usage
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Ok(())
    }

    /// Update connection statistics
    async fn update_stats(&mut self) {
        // TODO: Get real stats from the service
        // For now, simulate some stats
        let elapsed = self.start_time.elapsed().as_secs();

        self.stats = ConnectionStats {
            rtt_ms: Some(23 + (elapsed % 10) as u32),
            bitrate_kbps: Some(1500 + (elapsed % 500) as u32),
            fps: Some(30),
            packets_lost: Some((elapsed / 10) as u32),
            packets_sent: Some((elapsed * 100) as u32),
        };
    }

    /// Draw the main UI
    #[allow(dead_code)]
    fn draw_ui(&self, f: &mut Frame) {
        self.draw_ui_with_state(
            f,
            self.stats.clone(),
            self.muted,
            self.video_enabled,
            self.start_time,
        );
    }

    /// Draw the main UI with provided state
    #[allow(dead_code)]
    fn draw_ui_with_state(
        &self,
        f: &mut Frame,
        stats: ConnectionStats,
        muted: bool,
        video_enabled: bool,
        start_time: Instant,
    ) {
        let size = f.size();

        // Split the screen vertically
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),   // Video area
                Constraint::Length(3), // Stats
                Constraint::Length(3), // Controls
            ])
            .split(size);

        // Video display area
        self.draw_video_area(f, chunks[0]);

        // Statistics area
        self.draw_stats_area_with_state(f, chunks[1], stats, start_time);

        // Controls area
        self.draw_controls_area_with_state(f, chunks[2], muted, video_enabled);
    }

    /// Draw the video display area
    fn draw_video_area(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("🎥 Video Call")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let content = match self.display_mode {
            DisplayMode::Sixel => {
                // TODO: Implement Sixel rendering
                vec![Line::from(vec![
                    Span::styled("Sixel video display", Style::default().fg(Color::Green)),
                    Span::raw(" (placeholder)"),
                ])]
            }
            DisplayMode::Ascii => {
                // TODO: Implement ASCII art rendering
                vec![
                    Line::from("   .-\"\"\"-.   "),
                    Line::from("  /       \\  "),
                    Line::from(" |         | "),
                    Line::from("  \\       /  "),
                    Line::from("   '-----'   "),
                    Line::from("    (ᵔᴥᵔ)     "),
                ]
            }
            DisplayMode::None => {
                vec![Line::from(vec![Span::styled(
                    "Video disabled",
                    Style::default().fg(Color::Yellow),
                )])]
            }
        };

        let paragraph = Paragraph::new(content)
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);

        f.render_widget(paragraph, area);
    }

    /// Draw the statistics area
    #[allow(dead_code)]
    fn draw_stats_area(&self, f: &mut Frame, area: Rect) {
        self.draw_stats_area_with_state(f, area, self.stats.clone(), self.start_time);
    }

    /// Draw the statistics area with provided state
    fn draw_stats_area_with_state(
        &self,
        f: &mut Frame,
        area: Rect,
        stats: ConnectionStats,
        start_time: Instant,
    ) {
        let block = Block::default()
            .title("📊 Statistics")
            .borders(Borders::ALL);

        let stats_text = vec![
            Line::from(format!(
                "RTT: {}ms | Bitrate: {}kbps | FPS: {}",
                stats.rtt_ms.unwrap_or(0),
                stats.bitrate_kbps.unwrap_or(0),
                stats.fps.unwrap_or(0)
            )),
            Line::from(format!(
                "Packets: Sent {} | Lost {}",
                stats.packets_sent.unwrap_or(0),
                stats.packets_lost.unwrap_or(0)
            )),
            Line::from(format!(
                "Duration: {:.1}s",
                start_time.elapsed().as_secs_f32()
            )),
        ];

        let paragraph = Paragraph::new(stats_text).block(block);
        f.render_widget(paragraph, area);
    }

    /// Draw the controls area
    #[allow(dead_code)]
    fn draw_controls_area(&self, f: &mut Frame, area: Rect) {
        self.draw_controls_area_with_state(f, area, self.muted, self.video_enabled);
    }

    /// Draw the controls area with provided state
    fn draw_controls_area_with_state(
        &self,
        f: &mut Frame,
        area: Rect,
        muted: bool,
        video_enabled: bool,
    ) {
        let block = Block::default().title("🎮 Controls").borders(Borders::ALL);

        let controls = vec![Line::from(vec![
            Span::styled(
                "(q/Esc)",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Quit | "),
            Span::styled(
                "(m)",
                Style::default().fg(if muted { Color::Red } else { Color::Green }),
            ),
            Span::raw(" Mute | "),
            Span::styled(
                "(v)",
                Style::default().fg(if video_enabled {
                    Color::Green
                } else {
                    Color::Yellow
                }),
            ),
            Span::raw(" Video | "),
            Span::styled("(s)", Style::default().fg(Color::Blue)),
            Span::raw(" Stats | "),
            Span::styled("(h)", Style::default().fg(Color::Blue)),
            Span::raw(" Help"),
        ])];

        let paragraph = Paragraph::new(controls).block(block);
        f.render_widget(paragraph, area);
    }

    /// Display a video frame
    #[allow(dead_code)]
    pub fn display_frame(&mut self, frame_data: &[u8]) -> Result<()> {
        match self.display_mode {
            DisplayMode::Sixel => {
                // For Sixel, frame_data should be raw RGB/YUV
                // In production, we'd convert to image and render with sixel protocol
                // For now, just validate the data
                if frame_data.is_empty() {
                    return Err(anyhow::anyhow!("Empty frame data"));
                }
                Ok(())
            }
            DisplayMode::Ascii => {
                // For ASCII, we'd downsample and convert to characters
                if frame_data.is_empty() {
                    return Err(anyhow::anyhow!("Empty frame data"));
                }
                Ok(())
            }
            DisplayMode::None => Ok(()),
        }
    }

    /// Show help dialog
    #[allow(dead_code)]
    pub fn show_help(&self) {
        // TODO: Implement help overlay
    }
}

impl Drop for TerminalUI {
    fn drop(&mut self) {
        // Restore terminal state
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

/// Display mode enum (re-exported for CLI)
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CliDisplayMode {
    /// Sixel graphics (best quality)
    Sixel,
    /// ASCII art
    Ascii,
    /// No video display
    None,
}

impl From<CliDisplayMode> for DisplayMode {
    fn from(mode: CliDisplayMode) -> Self {
        match mode {
            CliDisplayMode::Sixel => DisplayMode::Sixel,
            CliDisplayMode::Ascii => DisplayMode::Ascii,
            CliDisplayMode::None => DisplayMode::None,
        }
    }
}
