use crate::logging::CustomLayer;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;

pub struct TraceBuffer(pub Arc<Mutex<Vec<u8>>>);

impl Write for TraceBuffer {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

impl TraceBuffer {
    pub fn as_string(&self) -> String {
        let buf = self.0.lock().unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }
}

pub fn run_with_tracing(debug: bool, f: impl FnOnce()) -> TraceBuffer {
    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    let mut buf = TraceBuffer(Arc::new(Mutex::new(Vec::new())));

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(CustomLayer::new(TraceBuffer(buf.0.clone())));

    tracing::subscriber::with_default(subscriber, || {
        f();
    });

    _ = buf.flush();
    buf
}
