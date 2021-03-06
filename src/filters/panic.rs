use super::prelude::*;

struct Proc;

#[allow(dead_code)]
pub struct PanicFilter {
  params: Vec<Param>,
  proc: Arc<Proc>,
}

impl PanicFilter {
  #[allow(dead_code)]
  pub fn new() -> Self {
    Self {
      params: Vec::new(),
      proc: Arc::new(Proc),
    }
  }
}

impl Filter for PanicFilter {
  fn name(&self) -> &str { "PANIC" }

  fn params(&self) -> &Vec<Param> { &self.params }

  fn proc(&self) -> ArcProc { self.proc.clone() as ArcProc }
}

impl RenderProc for Proc {
  // Heaven forbid we ignore a cancel request while we're panicking!
  fn process_tile(&self, _: &Tile, _: &CancelTok) {
    panic!("debug panic");
  }
}
