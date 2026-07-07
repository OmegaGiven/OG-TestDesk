use iced::widget::{column, text, text_input};
use iced::{Element, Task};

use og_testdesk_core::calc;

#[derive(Debug, Clone)]
pub enum CalculatorMessage {
    ExprChanged(String),
}

pub struct CalculatorTab {
    expr: String,
    result: Result<f64, String>,
}

impl CalculatorTab {
    pub fn new() -> (Self, Task<CalculatorMessage>) {
        (
            Self {
                expr: String::new(),
                result: Ok(0.0),
            },
            Task::none(),
        )
    }

    pub fn update(&mut self, message: CalculatorMessage) -> Task<CalculatorMessage> {
        match message {
            CalculatorMessage::ExprChanged(v) => {
                self.result = if v.trim().is_empty() {
                    Ok(0.0)
                } else {
                    calc::evaluate(&v)
                };
                self.expr = v;
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, CalculatorMessage> {
        let display = match &self.result {
            Ok(value) => text(format!("= {value}")),
            Err(err) => text(format!("Error: {err}")),
        };

        column![
            text("Calculator").size(16),
            text_input("2 + 2 * 3", &self.expr).on_input(CalculatorMessage::ExprChanged),
            display,
        ]
        .spacing(8)
        .padding(16)
        .into()
    }
}
