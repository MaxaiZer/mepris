use std::io::Write;

pub struct Logger<'a, W: Write> {
    pub current_step: usize,
    pub steps_count: usize,
    pub out: &'a mut W,
}

impl<'a, W: Write> Logger<'a, W> {
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
