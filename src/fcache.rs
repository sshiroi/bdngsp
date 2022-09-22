use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
};

pub struct FileCacheBackend {
    pub inner: Vec<(Option<Vec<u8>>, super::predict::PredictedKeyFrame)>,
    pub f: File,
    pub file_size: u64,
}

impl FileCacheBackend {
    pub fn new(f: File, pred: &[super::predict::PredictedKeyFrame]) -> FileCacheBackend {
        FileCacheBackend {
            file_size: f.metadata().unwrap().len(),
            f,
            inner: pred.iter().map(|e| (None, e.clone())).collect(),
        }
    }
}

pub struct FCached {
    pred: Arc<Mutex<FileCacheBackend>>,
    seek_head: u64,
}

impl FCached {
    pub fn new(backend: Arc<Mutex<FileCacheBackend>>) -> FCached {
        FCached {
            pred: backend,
            seek_head: 0,
        }
    }

    pub fn seek(&mut self, a: u64) {
        self.seek_head = a;
        //    self.f.seek(SeekFrom::Start(self.seek_head)).unwrap();
    }

    pub fn read(&mut self, b: &mut [u8]) -> u64 {
        // let mut lck = self.pred.lock().unwrap();
        // lck.f.seek(SeekFrom::Start(self.seek_head)).unwrap();
        // let rr =  lck.f.read(b).unwrap();
        // self.seek_head += rr as u64;
        // return rr as u64;
        let mut lck = self.pred.lock().unwrap();
        let mut read_sum = 0;

        for i in 0..lck.inner.len() {
            let pos = if i == 0 {
                0
            } else {
                lck.inner[i].1.spn as u64 * 192
            };

            let next_pos = if i == lck.inner.len() - 1 {
                lck.file_size
            } else {
                lck.inner[i + 1].1.spn as u64 * 192
            };
            if self.seek_head >= pos && self.seek_head < next_pos {
                if lck.inner[i].0.is_none() {
                    let mut buf = vec![0u8; (next_pos - pos) as usize];

                    lck.f.seek(SeekFrom::Start(pos)).unwrap();
                    lck.f.read_exact(&mut buf).unwrap();
                    lck.inner[i].0 = Some(buf);
                }
                let cached = lck.inner[i].0.as_ref().unwrap();
                let cached_offset = self.seek_head - pos;

                let to_read = (b.len() - read_sum).min(cached.len() - cached_offset as usize);

                b[read_sum..read_sum + to_read as usize].copy_from_slice(
                    &cached[cached_offset as usize..cached_offset as usize + to_read as usize],
                );
                read_sum += to_read;

                self.seek_head += to_read as u64;

                if b.len() == read_sum {
                    return read_sum as u64;
                }
            }
        }
        read_sum as u64
    }
}
