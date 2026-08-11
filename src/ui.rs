use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};

use crate::{
    model::{Snapshot, UsageRow},
    usage,
};

const BG: Color = Color::Reset;
const SURFACE: Color = Color::Rgb(35, 33, 54);
const OVERLAY: Color = Color::Rgb(57, 53, 82);
const MUTED: Color = Color::Rgb(110, 106, 134);
const SUBTLE: Color = Color::Rgb(144, 140, 170);
const TEXT: Color = Color::Rgb(224, 222, 244);
const IRIS: Color = Color::Rgb(196, 167, 231);
const FOAM: Color = Color::Rgb(156, 207, 216);

pub fn run(mut snapshots: Vec<Snapshot>) -> Result<()> {
    let mut terminal = ratatui::init();
    terminal.clear()?;
    let result = event_loop(&mut terminal, &mut snapshots);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, snapshots: &mut Vec<Snapshot>) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, snapshots))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('r') => {
                    let providers = snapshots
                        .iter()
                        .map(|snapshot| snapshot.provider)
                        .collect::<Vec<_>>();
                    *snapshots = usage::collect_all(&providers);
                }
                _ => {}
            }
        }
    }
}

fn draw(frame: &mut Frame, snapshots: &[Snapshot]) {
    frame.render_widget(
        Block::default().style(Style::default().bg(BG)),
        frame.area(),
    );
    for (area, snapshot) in panel_areas(frame.area(), snapshots.len())
        .into_iter()
        .zip(snapshots)
    {
        draw_snapshot(frame, area, snapshot);
    }
}

fn draw_snapshot(frame: &mut Frame, outer: Rect, snapshot: &Snapshot) {
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .style(Style::default().bg(BG));
    frame.render_widget(panel, outer);
    let inner = outer.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    if inner.width < 28 || inner.height < 10 {
        frame.render_widget(
            Paragraph::new("terminal too small\nneed at least 32 x 12")
                .style(Style::default().fg(IRIS))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    if inner.height < 22 {
        draw_compact(frame, inner, snapshot);
        return;
    }

    let limit_height = if snapshot.limits.is_empty() {
        2
    } else {
        snapshot.limits.len() as u16 * 3 + 2
    };
    let model_height = (snapshot.models.len().max(1) as u16) + 2;
    let constraints = if inner.height >= 30 {
        vec![
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(limit_height),
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(1),
            Constraint::Length(model_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(limit_height),
            Constraint::Length(1),
            Constraint::Min(7),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    draw_header(frame, chunks[0], snapshot);
    draw_rule(frame, chunks[1]);
    draw_limits(frame, chunks[2], snapshot);
    draw_rule(frame, chunks[3]);
    draw_rows(frame, chunks[4], "TOKENS / DAY", &snapshot.days, false);
    draw_rule(frame, chunks[5]);
    if inner.height >= 30 {
        draw_rows(frame, chunks[6], "TOKENS / MODEL", &snapshot.models, true);
        let status = chunks[8];
        draw_status(frame, status, snapshot);
    } else {
        draw_status(frame, chunks[6], snapshot);
    }
}

fn panel_areas(area: Rect, count: usize) -> Vec<Rect> {
    if count <= 1 {
        return vec![centered_panel(area)];
    }
    let canvas = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let columns = if canvas.width >= 82 { 2 } else { 1 };
    let rows = count.div_ceil(columns);
    let row_areas = Layout::vertical(vec![Constraint::Ratio(1, rows as u32); rows])
        .spacing(1)
        .split(canvas);
    let mut areas = Vec::with_capacity(count);
    for row in row_areas.iter() {
        let in_row = (count - areas.len()).min(columns);
        let columns = Layout::horizontal(vec![Constraint::Ratio(1, in_row as u32); in_row])
            .spacing(1)
            .split(*row);
        areas.extend(columns.iter().copied());
    }
    areas.truncate(count);
    areas
}

fn draw_compact(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let limit_height = 1 + snapshot.limits.len().max(1) as u16 * 2;
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(limit_height),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);
    draw_header(frame, chunks[0], snapshot);
    draw_rule(frame, chunks[1]);
    draw_compact_limits(frame, chunks[2], snapshot);
    draw_rule(frame, chunks[3]);
    frame.render_widget(
        section_title("TOKENS / TODAY"),
        Rect {
            height: 1,
            ..chunks[4]
        },
    );
    if let Some(today) = snapshot.days.last() {
        let peak = snapshot
            .days
            .iter()
            .map(|row| row.tokens)
            .max()
            .unwrap_or(1)
            .max(1);
        draw_bar_line(
            frame,
            Rect {
                y: chunks[4].y + 2,
                height: 1,
                ..chunks[4]
            },
            today,
            peak,
            false,
        );
    }
    draw_rule(frame, chunks[5]);
    draw_rows(frame, chunks[6], "TOKENS / MODEL", &snapshot.models, true);
    draw_status(frame, chunks[7], snapshot);
}

fn draw_compact_limits(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    frame.render_widget(section_title("LIMITS"), Rect { height: 1, ..area });
    if snapshot.limits.is_empty() {
        frame.render_widget(
            Paragraph::new("not exposed").style(Style::default().fg(MUTED)),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
        return;
    }
    for (index, limit) in snapshot.limits.iter().enumerate() {
        let y = area.y + 1 + index as u16 * 2;
        let reset = limit
            .resets
            .as_ref()
            .map(|value| format!("  reset {value}"))
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(&limit.label, Style::default().fg(TEXT)),
                Span::styled(
                    format!("  {}%", limit.percent),
                    Style::default().fg(IRIS).add_modifier(Modifier::BOLD),
                ),
                Span::styled(reset, Style::default().fg(MUTED)),
            ])),
            Rect {
                y,
                height: 1,
                ..area
            },
        );
        frame.render_widget(
            Gauge::default()
                .ratio(f64::from(limit.percent) / 100.0)
                .gauge_style(Style::default().fg(SUBTLE).bg(OVERLAY))
                .label(""),
            Rect {
                y: y + 1,
                height: 1,
                ..area
            },
        );
    }
}

fn draw_bar_line(frame: &mut Frame, line: Rect, row: &UsageRow, peak: u64, blocks: bool) {
    let label_width = if blocks {
        (line.width / 2).clamp(12, 28)
    } else {
        7
    };
    let value_width = 9.min(line.width.saturating_sub(label_width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(label_width),
            Constraint::Min(4),
            Constraint::Length(value_width),
        ])
        .split(line);
    let today = row.label == "Today";
    frame.render_widget(
        Paragraph::new(row.label.as_str()).style(
            Style::default()
                .fg(if today { TEXT } else { SUBTLE })
                .add_modifier(if today {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        columns[0],
    );
    frame.render_widget(
        Gauge::default()
            .ratio(row.tokens as f64 / peak.max(1) as f64)
            .gauge_style(
                Style::default()
                    .fg(if blocks { IRIS } else { SUBTLE })
                    .bg(SURFACE),
            )
            .label(""),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(human_tokens(row.tokens))
            .alignment(Alignment::Right)
            .style(Style::default().fg(if today { TEXT } else { SUBTLE })),
        columns[2],
    );
}

fn centered_panel(area: Rect) -> Rect {
    let width = area.width.clamp(1, 72);
    let height = area.height.clamp(1, 44);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn draw_header(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let plan = snapshot
        .plan
        .as_deref()
        .unwrap_or("LOCAL")
        .to_ascii_uppercase();
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}  ", snapshot.provider.mark()),
                Style::default().fg(FOAM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                snapshot.provider.name(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::styled(format!("    {plan}"), Style::default().fg(MUTED)),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_rule(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(OVERLAY)),
        area,
    );
}

fn draw_limits(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            std::iter::once(Constraint::Length(1))
                .chain(snapshot.limits.iter().map(|_| Constraint::Length(3)))
                .collect::<Vec<_>>(),
        )
        .split(area);
    frame.render_widget(section_title("LIMITS"), rows[0]);
    if snapshot.limits.is_empty() {
        frame.render_widget(
            Paragraph::new("not exposed by this agent").style(Style::default().fg(MUTED)),
            area.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
        );
        return;
    }
    for (index, limit) in snapshot.limits.iter().enumerate() {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(rows[index + 1]);
        let reset = limit
            .resets
            .as_ref()
            .map(|value| format!("reset {value}"))
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(&limit.label, Style::default().fg(TEXT)),
                Span::raw("  "),
                Span::styled(
                    format!("{}%", limit.percent),
                    Style::default().fg(IRIS).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {reset}"), Style::default().fg(MUTED)),
            ])),
            parts[0],
        );
        frame.render_widget(
            Gauge::default()
                .ratio(f64::from(limit.percent) / 100.0)
                .gauge_style(Style::default().fg(SUBTLE).bg(OVERLAY))
                .label(""),
            Rect {
                height: 1,
                ..parts[1]
            },
        );
    }
}

fn draw_rows(frame: &mut Frame, area: Rect, title: &str, rows: &[UsageRow], blocks: bool) {
    frame.render_widget(section_title(title), Rect { height: 1, ..area });
    let available = area.height.saturating_sub(2) as usize;
    let shown = rows.len().min(available);
    if shown == 0 {
        frame.render_widget(
            Paragraph::new("no local usage found").style(Style::default().fg(MUTED)),
            Rect {
                y: area.y + 2,
                height: 1,
                ..area
            },
        );
        return;
    }
    let peak = rows.iter().map(|row| row.tokens).max().unwrap_or(1).max(1);
    for (index, row) in rows.iter().take(shown).enumerate() {
        let y = area.y + index as u16 + 2;
        let line = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        draw_bar_line(frame, line, row, peak, blocks);
    }
}

fn draw_status(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let note = snapshot.note.as_deref().unwrap_or("r refresh  q quit");
    let right = snapshot.updated_at.format("%H:%M:%S").to_string();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(8), Constraint::Length(10)])
        .split(area);
    frame.render_widget(
        Paragraph::new(note)
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: true }),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(right)
            .alignment(Alignment::Right)
            .style(Style::default().fg(MUTED)),
        columns[1],
    );
}

fn section_title(title: &str) -> Paragraph<'_> {
    Paragraph::new(title).style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
}

fn human_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000_000 {
        format!("{:.1}B", tokens as f64 / 1_000_000_000.0)
    } else if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Local;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::model::{Limit, Provider};

    fn snapshot(provider: Provider) -> Snapshot {
        Snapshot {
            provider,
            plan: Some("Plus".into()),
            limits: vec![Limit {
                label: "Weekly".into(),
                percent: 50,
                resets: Some("3d 2h".into()),
            }],
            days: vec![UsageRow {
                label: "Today".into(),
                tokens: 5_000,
            }],
            models: vec![UsageRow {
                label: "gpt-test".into(),
                tokens: 5_000,
            }],
            note: None,
            updated_at: Local::now(),
        }
    }

    #[test]
    fn compact_vertical_cards_keep_gauges() {
        let backend = TestBackend::new(70, 44);
        let mut terminal = Terminal::new(backend).unwrap();
        let snapshots = [snapshot(Provider::Claude), snapshot(Provider::OpenCode)];
        terminal.draw(|frame| draw(frame, &snapshots)).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert_eq!(rendered.matches("Weekly").count(), 2);
        assert!(rendered.contains('█'));
        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
    }
}
