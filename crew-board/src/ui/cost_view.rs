use crate::app::App;
use crate::ui::styles;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    // Store content rect for mouse scroll
    *app.content_rect.borrow_mut() = Some(area);

    let mut lines: Vec<Line> = Vec::new();
    let bold = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim = styles::dim_style();
    let value_style = Style::default().fg(Color::White);
    let cost_style = Style::default().fg(Color::Green);

    lines.push(Line::from(Span::styled("Cost Summary", bold)));
    lines.push(Line::from(""));

    // -- Live session costs from hook state --
    let mut has_live_data = false;
    if let Some(ref mgr) = app.terminal_manager {
        let terminals_with_cost: Vec<_> = mgr.terminals.iter()
            .filter(|t| t.hook_state.as_ref().is_some_and(|h| h.total_cost_usd > 0.0 || h.total_input_tokens > 0))
            .collect();
        if !terminals_with_cost.is_empty() {
            has_live_data = true;
            lines.push(Line::from(Span::styled("Live Session Costs", header_style)));
            lines.push(Line::from(""));
            // Header row
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<20}", "Terminal"), dim),
                Span::styled(format!("{:>12}", "Input Tok"), dim),
                Span::styled(format!("{:>12}", "Output Tok"), dim),
                Span::styled(format!("{:>10}", "Cost"), dim),
            ]));
            let mut total_cost = 0.0f64;
            let mut total_input = 0u64;
            let mut total_output = 0u64;
            for term in &terminals_with_cost {
                let hs = term.hook_state.as_ref().unwrap();
                let short_id = term.id.strip_prefix("TASK_").unwrap_or(&term.id);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<20}", short_id), value_style),
                    Span::styled(format!("{:>12}", format_tokens(hs.total_input_tokens)), value_style),
                    Span::styled(format!("{:>12}", format_tokens(hs.total_output_tokens)), value_style),
                    Span::styled(format!("{:>10}", format!("${:.4}", hs.total_cost_usd)), cost_style),
                ]));
                total_cost += hs.total_cost_usd;
                total_input += hs.total_input_tokens;
                total_output += hs.total_output_tokens;
            }
            // Totals row
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<20}", "TOTAL"), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:>12}", format_tokens(total_input)), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:>12}", format_tokens(total_output)), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:>10}", format!("${:.4}", total_cost)), cost_style.add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(""));
        }
    }

    // -- Per-task cost data from state.json --
    let mut has_task_data = false;
    if let Some(repo) = app.current_repo() {
        for loaded in &repo.tasks {
            let task = &loaded.state;

            // Estimated cost from workflow mode
            if let Some(ref mode) = task.workflow_mode {
                if !mode.estimated_cost.is_empty() {
                    has_task_data = true;
                    lines.push(Line::from(vec![
                        Span::styled(format!("{}: ", task.task_id), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(mode.effective.as_str(), Style::default().fg(Color::Yellow)),
                        Span::raw(" mode, est. "),
                        Span::styled(mode.estimated_cost.as_str(), cost_style),
                    ]));
                }
            }

            // Structured cost data
            if let Some(ref cost) = task.cost_summary {
                has_task_data = true;

                // By-agent table
                if let Some(by_agent) = cost.get("by_agent").and_then(|v| v.as_object()) {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("  {} - By Agent", task.task_id),
                        header_style,
                    )));
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<16}", "Agent"), dim),
                        Span::styled(format!("{:>12}", "Input Tok"), dim),
                        Span::styled(format!("{:>12}", "Output Tok"), dim),
                        Span::styled(format!("{:>10}", "Cost"), dim),
                    ]));
                    let mut agent_total_cost = 0.0f64;
                    let mut agent_total_input = 0u64;
                    let mut agent_total_output = 0u64;
                    for (agent, data) in by_agent {
                        let inp = data.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let out = data.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let c = data.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        agent_total_input += inp;
                        agent_total_output += out;
                        agent_total_cost += c;
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {:<16}", agent), value_style),
                            Span::styled(format!("{:>12}", format_tokens(inp)), value_style),
                            Span::styled(format!("{:>12}", format_tokens(out)), value_style),
                            Span::styled(format!("{:>10}", format!("${:.4}", c)), cost_style),
                        ]));
                    }
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<16}", "TOTAL"), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!("{:>12}", format_tokens(agent_total_input)), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!("{:>12}", format_tokens(agent_total_output)), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!("{:>10}", format!("${:.4}", agent_total_cost)), cost_style.add_modifier(Modifier::BOLD)),
                    ]));
                }

                // By-model table
                if let Some(by_model) = cost.get("by_model").and_then(|v| v.as_object()) {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("  {} - By Model", task.task_id),
                        header_style,
                    )));
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<20}", "Model"), dim),
                        Span::styled(format!("{:>12}", "Input Tok"), dim),
                        Span::styled(format!("{:>12}", "Output Tok"), dim),
                        Span::styled(format!("{:>6}", "Runs"), dim),
                        Span::styled(format!("{:>10}", "Cost"), dim),
                    ]));
                    let mut model_total_cost = 0.0f64;
                    for (model, data) in by_model {
                        let inp = data.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let out = data.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let runs = data.get("runs").and_then(|v| v.as_u64()).unwrap_or(0);
                        let c = data.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        model_total_cost += c;
                        let short_model = if model.len() > 20 { &model[..20] } else { model.as_str() };
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {:<20}", short_model), value_style),
                            Span::styled(format!("{:>12}", format_tokens(inp)), value_style),
                            Span::styled(format!("{:>12}", format_tokens(out)), value_style),
                            Span::styled(format!("{:>6}", runs), value_style),
                            Span::styled(format!("{:>10}", format!("${:.4}", c)), cost_style),
                        ]));
                    }
                    if by_model.len() > 1 {
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {:<20}", ""), dim),
                            Span::raw(""),
                            Span::raw(""),
                            Span::raw(""),
                            Span::styled(format!("{:>10}", format!("${:.4}", model_total_cost)), cost_style.add_modifier(Modifier::BOLD)),
                        ]));
                    }
                }

                // Fallback: show total_cost if no breakdown available
                if cost.get("by_agent").is_none() && cost.get("by_model").is_none() {
                    if let Some(total) = cost.get("total_cost").and_then(|v| v.as_f64()) {
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {} actual: ", task.task_id), dim),
                            Span::styled(format!("${:.4}", total), cost_style),
                        ]));
                    }
                }
            }
        }
    }

    if !has_live_data && !has_task_data {
        lines.push(Line::from(Span::styled("No cost data available yet.", dim)));
        lines.push(Line::from(""));
        lines.push(Line::from("Cost data is recorded during workflow execution."));
    }

    let text = Text::from(lines);
    let block = Block::default()
        .title(" Cost Summary ")
        .borders(Borders::ALL);
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Format token counts with K/M suffix for readability.
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}
