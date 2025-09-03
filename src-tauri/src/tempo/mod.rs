use crate::bpm::{BpmEstimate, SimpleBackend};

pub trait TempoBackend: Send {
	fn process(&mut self, frames: &[f32]) -> Option<BpmEstimate>;
}

pub fn make_backend(sample_rate: u32) -> Box<dyn TempoBackend> { Box::new(SimpleTempo::new(sample_rate)) }

struct SimpleTempo { inner: SimpleBackend }
impl SimpleTempo { fn new(sr: u32) -> Self { Self { inner: SimpleBackend::new(sr) } } }
impl TempoBackend for SimpleTempo {
	fn process(&mut self, frames: &[f32]) -> Option<BpmEstimate> { self.inner.process_frames(frames) }
}
