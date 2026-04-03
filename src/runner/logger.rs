use std::io::Write;

pub struct Logger<'a> {
    pub current_step: usize,
    pub steps_count: usize,
    pub out: &'a mut dyn Write,
}

impl<'a> Logger<'a> {
    pub fn new(step_count: usize, out: &'a mut dyn Write) -> Self {
        Logger {
            current_step: 0,
            steps_count: step_count,
            out,
        }
    }

    pub fn log(&mut self, str: &str) -> std::io::Result<()> {
        let width = self.steps_count.to_string().len();
        let progress = format!(
            "[{:>width$}/{}]",
            self.current_step,
            self.steps_count,
            width = width
        );

        writeln!(self.out, "{}", str.replace("PROGRESS", &progress))
    }
}
