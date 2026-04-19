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

    pub fn log(&mut self, msg: &str) -> std::io::Result<()> {
        writeln!(self.out, "{}", msg)
    }

    pub fn log_with_progress<F>(&mut self, f: F) -> std::io::Result<()>
    where
        F: FnOnce(&str) -> String,
    {
        let width = self.steps_count.to_string().len();
        let progress = format!(
            "[{:>width$}/{}]",
            self.current_step,
            self.steps_count,
            width = width
        );

        let msg = f(&progress);
        writeln!(self.out, "{}", msg)
    }
}
