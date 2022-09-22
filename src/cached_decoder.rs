use std::sync::*;

use crate::ffmpeg_stuff::DecoderSetup;

#[derive(Clone)]
pub struct CachedDecoder {
    pub decoder: Option<Arc<Mutex<DecoderSetup>>>,
    pub start_idx: u64,
    pub current_idx: u64,
    pub frames_left: u64,
    pub frames_to_serve_total: u64,
}

impl CachedDecoder {
    pub fn new(
        decoder: DecoderSetup,
        start_idx: u64,
        current_idx: u64,
        frames_to_serve: u64,
    ) -> CachedDecoder {
        CachedDecoder {
            decoder: Some(Arc::new(Mutex::new(decoder))),
            start_idx,
            current_idx,
            frames_left: frames_to_serve,
            frames_to_serve_total: frames_to_serve,
        }
    }
}
