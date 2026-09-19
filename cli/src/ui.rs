// Rendering for the `rojom` dashboard: header, project sidebar, log console,
// key-hint footer, and the scan / confirm popups.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::core::{DiscoveredProject, LogLine, Project};
use crate::{App, Mode, Status};

const SIDEBAR_WIDTH: u16 = 34;
const ACCENT: Color = Color::LightRed; // Rojo's brand red

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let [side, main] =
        Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(20)]).areas(body);

    draw_header(frame, app, header);
    draw_sidebar(frame, app, side);
    draw_logs(frame, app, main);
    draw_footer(frame, app, footer);

    match &app.mode {
        Mode::Normal => {}
        Mode::ScanInput { input } => draw_scan_input(frame, input),
        Mode::ScanPick {
            items,
            checked,
            cursor,
        } => draw_scan_pick(frame, items, checked, *cursor),
        Mode::ConfirmDelete => draw_confirm(frame, app),
        Mode::ConfirmQuit => draw_confirm_quit(frame, app),
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let running = app.running_count();
    let title = Line::from(vec![
        " ◆ ".fg(ACCENT).bold(),
        "Rojo Manager".bold(),
        "  terminal".dim(),
    ]);
    let stats = Line::from(vec![
        Span::styled(
            format!("{running} running"),
            if running > 0 {
                Style::new().green().bold()
            } else {
                Style::new().dim()
            },
        ),
        format!(" · {} projects ", app.projects.len()).dim(),
    ])
    .right_aligned();
    frame.render_widget(Paragraph::new(title), area);
    frame.render_widget(Paragraph::new(stats), area);
}

fn status_dot(status: &Status) -> Span<'static> {
    match status {
        Status::Running => "●".green(),
        Status::Stopped => "○".dark_gray(),
        Status::Error(_) => "●".red(),
    }
}

fn status_badge(status: &Status) -> Span<'static> {
    match status {
        Status::Running => " running ".black().on_green().bold(),
        Status::Stopped => " stopped ".dim().reversed(),
        Status::Error(_) => " error ".white().on_red().bold(),
    }
}

/// Truncate to `width` columns (chars, not bytes) with an ellipsis.
fn clip(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let keep = width.saturating_sub(1);
    format!("{}…", s.chars().take(keep).collect::<String>())
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().dim())
        .title_top(Line::from(" Projects ").bold());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.projects.is_empty() {
        let hint = Text::from(vec![
            Line::from(""),
            Line::from("No projects yet.").dim().centered(),
            Line::from(vec![
                "Press ".dim(),
                "a".bold().fg(ACCENT),
                " to scan a folder.".dim(),
            ])
            .centered(),
        ]);
        frame.render_widget(Paragraph::new(hint), inner);
        return;
    }

    // "› ● name              :port" — name fills whatever the port leaves free.
    let usable = inner.width.saturating_sub(2) as usize; // minus highlight symbol
    let items: Vec<ListItem> = app
        .projects
        .iter()
        .map(|p| {
            let port = format!(":{}", p.port);
            let name_w = usable.saturating_sub(2 + port.len() + 1);
            let name = clip(&p.name, name_w);
            let pad = " ".repeat(name_w.saturating_sub(name.chars().count()) + 1);
            ListItem::new(Line::from(vec![
                status_dot(&app.status_of(&p.id)),
                " ".into(),
                name.into(),
                pad.into(),
                port.dim(),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(list, inner, &mut state);
}

fn log_line(entry: &LogLine) -> Line<'static> {
    let ts = chrono::DateTime::from_timestamp_millis(entry.ts)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default();
    let (bar, text): (Span<'static>, Span<'static>) = match entry.stream.as_str() {
        "system" => ("▏".magenta(), entry.line.clone().dim().italic()),
        "stderr" => ("▏".yellow(), colour_by_level(&entry.line)),
        _ => ("▏".cyan(), colour_by_level(&entry.line)),
    };
    Line::from(vec![ts.dim(), " ".into(), bar, " ".into(), text])
}

/// Rojo's own log level markers decide the colour; everything else stays plain.
fn colour_by_level(line: &str) -> Span<'static> {
    let owned = line.to_string();
    if line.contains("ERROR") || line.starts_with("error") {
        owned.red()
    } else if line.contains("WARN") {
        owned.yellow()
    } else {
        owned.into()
    }
}

fn draw_logs(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(project) = app.selected_project().cloned() else {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().dim())
            .title_top(Line::from(" Logs ").bold());
        frame.render_widget(block, area);
        return;
    };
    let status = app.status_of(&project.id);

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(match status {
            Status::Running => Style::new().green(),
            Status::Error(_) => Style::new().red(),
            Status::Stopped => Style::new().dim(),
        })
        .title_top(Line::from(vec![
            " ".into(),
            project.name.clone().bold(),
            format!("  :{} ", project.port).dim(),
        ]))
        .title_top(Line::from(vec![status_badge(&status), " ".into()]).right_aligned())
        .title_bottom(Line::from(serve_command(&project)).dim());
    if let Status::Error(msg) = &status {
        block = block.title_bottom(Line::from(format!(" {msg} ").red()).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = app
        .logs
        .get(&project.id)
        .map(|buf| buf.iter().map(log_line).collect())
        .unwrap_or_default();
    if lines.is_empty() {
        let hint = Line::from(vec![
            "No output yet — press ".dim(),
            "Enter".bold().fg(ACCENT),
            " to start.".dim(),
        ])
        .centered();
        frame.render_widget(Paragraph::new(hint), inner);
        return;
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total = paragraph.line_count(inner.width);
    let max_scroll = total.saturating_sub(inner.height as usize);
    app.log_scroll = app.log_scroll.min(max_scroll);
    let offset = max_scroll - app.log_scroll;
    frame.render_widget(paragraph.scroll((offset as u16, 0)), inner);

    if app.log_scroll > 0 {
        let tag = format!(" ↓ {} more ", app.log_scroll);
        let w = tag.chars().count() as u16;
        let x = area.x + area.width.saturating_sub(w + 2);
        let y = area.y + area.height - 1;
        frame.render_widget(
            Paragraph::new(tag.black().on_yellow()),
            Rect::new(x, y, w, 1),
        );
    }
}

fn serve_command(p: &Project) -> String {
    let extra = if p.args.is_empty() {
        String::new()
    } else {
        format!(" {}", p.args.join(" "))
    };
    format!(" rojo serve {} --port {}{extra} ", p.project_file, p.port)
}

fn key_hints(pairs: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans: Vec<Span> = vec![" ".into()];
    for (i, (key, what)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push("  ".into());
        }
        spans.push((*key).bold().fg(ACCENT));
        spans.push(" ".into());
        spans.push((*what).dim());
    }
    Line::from(spans)
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(t) = &app.toast {
        let style = if t.is_error {
            Style::new().white().on_red().bold()
        } else {
            Style::new().black().on_green()
        };
        frame.render_widget(Paragraph::new(format!(" {} ", t.text)).style(style), area);
        return;
    }
    let hints = match app.mode {
        Mode::Normal => key_hints(&[
            ("↑↓", "select"),
            ("Enter", "start/stop"),
            ("S", "start all"),
            ("X", "stop all"),
            ("a", "add"),
            ("d", "delete"),
            ("r", "reload"),
            ("c", "clear"),
            ("PgUp/PgDn", "scroll"),
            ("q", "quit"),
        ]),
        Mode::ScanInput { .. } => {
            key_hints(&[("Enter", "scan"), ("Ctrl+U", "clear"), ("Esc", "cancel")])
        }
        Mode::ScanPick { .. } => key_hints(&[
            ("↑↓", "move"),
            ("Space", "toggle"),
            ("a", "all/none"),
            ("Enter", "add selected"),
            ("Esc", "cancel"),
        ]),
        Mode::ConfirmDelete => key_hints(&[("y", "delete"), ("any other key", "cancel")]),
        Mode::ConfirmQuit => key_hints(&[("y", "quit"), ("any other key", "cancel")]),
    };
    frame.render_widget(Paragraph::new(hints), area);
}

// ---- popups ----------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn popup(frame: &mut Frame, title: &str, width: u16, height: u16) -> Rect {
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title_top(Line::from(format!(" {title} ")).bold());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

fn draw_scan_input(frame: &mut Frame, input: &str) {
    let width = (frame.area().width * 3 / 4).clamp(40, 90);
    let inner = popup(frame, "Add projects — scan a folder", width, 5);
    let text = Text::from(vec![
        Line::from("Folder to search recursively for *.project.json:").dim(),
        Line::from(vec![
            "› ".fg(ACCENT).bold(),
            clip(input, inner.width.saturating_sub(3) as usize).into(),
            "▏".fg(ACCENT),
        ]),
        Line::from("Vendored folders (node_modules, Packages, target…) are skipped.").dim(),
    ]);
    frame.render_widget(Paragraph::new(text), inner);
}

fn draw_scan_pick(frame: &mut Frame, items: &[DiscoveredProject], checked: &[bool], cursor: usize) {
    let width = (frame.area().width * 4 / 5).clamp(50, 110);
    let height = (items.len() as u16 + 3).min(frame.area().height.saturating_sub(2));
    let picked = checked.iter().filter(|c| **c).count();
    let inner = popup(
        frame,
        &format!("Found {} project(s) — {picked} selected", items.len()),
        width,
        height,
    );
    let [list_area, hint_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let name_w = 24usize;
    let rows: Vec<ListItem> = items
        .iter()
        .zip(checked)
        .map(|(d, on)| {
            let mark = if *on { "[x]".green() } else { "[ ]".dim() };
            let name = clip(&d.name, name_w);
            let pad = " ".repeat(name_w.saturating_sub(name.chars().count()) + 1);
            let folder_w = (list_area.width as usize).saturating_sub(6 + name_w + 8);
            ListItem::new(Line::from(vec![
                mark,
                " ".into(),
                name.bold(),
                pad.into(),
                clip(&d.folder, folder_w).dim(),
                format!("  :{}", d.port).cyan(),
            ]))
        })
        .collect();
    let list = List::new(rows)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(list, list_area, &mut state);
    frame.render_widget(
        Paragraph::new(Line::from("Ports are assigned to the next free one.").dim()),
        hint_area,
    );
}

fn draw_confirm(frame: &mut Frame, app: &App) {
    let name = app
        .selected_project()
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let inner = popup(frame, "Delete project", 50, 4);
    let text = Text::from(vec![
        Line::from(vec![
            "Remove ".into(),
            name.bold(),
            " from the list?".into(),
        ]),
        Line::from("A running serve is stopped first. Files on disk are untouched.").dim(),
    ]);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

fn draw_confirm_quit(frame: &mut Frame, app: &App) {
    let inner = popup(frame, "Quit", 50, 4);
    let text = Text::from(vec![
        Line::from(vec![
            "Stop ".into(),
            format!("{} running serve(s)", app.running_count()).bold(),
            " and quit?".into(),
        ]),
        Line::from("Serves do not survive the dashboard — unlike the desktop tray.").dim(),
    ]);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn sample_app() -> App {
        let mut app = App::new();
        let mk = |id: &str, name: &str, port: u16| Project {
            id: id.into(),
            name: name.into(),
            folder: "C:/games/x".into(),
            project_file: "default.project.json".into(),
            port,
            args: vec![],
        };
        app.projects = vec![
            mk("a", "GameA", 34872),
            mk("b", "A very long project name that overflows", 34873),
        ];
        app.status
            .insert("a".into(), Status::Error("rojo exited with code 1".into()));
        for i in 0..40 {
            app.logs.entry("a".into()).or_default().push_back(LogLine {
                ts: 0,
                stream: if i % 2 == 0 {
                    "stdout".into()
                } else {
                    "stderr".into()
                },
                line: format!("[ERROR rojo] line {i} {}", "x".repeat(i * 3)),
            });
        }
        app
    }

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_mode_renders_at_normal_and_tiny_sizes_without_panicking() {
        let mut app = sample_app();
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains("GameA"), "{screen}");
        assert!(screen.contains("error"), "{screen}");
        assert!(
            screen.contains("line 39"),
            "tail should be visible:\n{screen}"
        );

        app.log_scroll = usize::MAX;
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains("line 0"), "scrolled to top:\n{screen}");
        assert!(screen.contains("more"), "{screen}");

        let items = vec![DiscoveredProject {
            name: "Found".into(),
            folder: "C:/found".into(),
            project_file: "default.project.json".into(),
            port: 34874,
            reason: String::new(),
        }];
        let modes = [
            Mode::ScanInput {
                input: "C:/games".into(),
            },
            Mode::ScanPick {
                items,
                checked: vec![true],
                cursor: 0,
            },
            Mode::ConfirmDelete,
            Mode::ConfirmQuit,
            Mode::Normal,
        ];
        for mode in modes {
            app.mode = mode;
            for (w, h) in [(100, 30), (40, 8), (20, 3)] {
                render(&mut app, w, h);
            }
        }

        app.projects.clear();
        assert!(render(&mut app, 80, 20).contains("No projects yet"));
    }
}
