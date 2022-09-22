#[macro_use]
extern crate vapoursynth;

use std::ffi::CStr;
use std::fs::File;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{bail, Error};
use cached_decoder::CachedDecoder;
use ffmpeg_sys::AVFrame;
use vapoursynth::core::CoreRef;
use vapoursynth::format::FormatID;
use vapoursynth::node::Flags;
use vapoursynth::plugins::{Filter, FilterArgument, FrameContext, Metadata};
use vapoursynth::prelude::*;
use vapoursynth::video_info::{Framerate, Resolution, VideoInfo};

mod cached_decoder;
mod clpi;
mod fcache;
mod ffmpeg_stuff;
mod predict;

struct VSSourceFilter<'core> {
    clpi: crate::clpi::CLPIResult,

    pred: Vec<crate::predict::PredictedKeyFrame>,
    pred_inner: Arc<Mutex<fcache::FileCacheBackend>>,

    cached_decoder: Arc<Mutex<Option<CachedDecoder>>>,
    cached_decoder_info: RwLock<Option<CachedDecoder>>,

    global_lock: Arc<Mutex<bool>>,

    resolution: Resolution,
    framerate: Framerate,
    format_id: FormatID,

    num_frames: usize,

    a: PhantomData<&'core u8>,
}

unsafe impl<'core> Send for VSSourceFilter<'core> {}
unsafe impl<'core> Sync for VSSourceFilter<'core> {}

fn extract_framedata(av_frame: &AVFrame, frame: &mut FrameRefMut) {
    unsafe {
        let y = std::slice::from_raw_parts(
            av_frame.data[0],
            av_frame.linesize[0] as usize * av_frame.height as usize,
        );
        let c = std::slice::from_raw_parts(
            av_frame.data[1],
            av_frame.linesize[1] as usize * (av_frame.height / 2) as usize,
        );
        let b = std::slice::from_raw_parts(
            av_frame.data[2],
            av_frame.linesize[2] as usize * (av_frame.height / 2) as usize,
        );
        {
            let vs_y: &mut [u8] = frame.plane_mut(0).unwrap();
            vs_y.copy_from_slice(y);
        }
        {
            let vs_c: &mut [u8] = frame.plane_mut(1).unwrap();
            vs_c.copy_from_slice(c);
        }
        {
            let vs_b: &mut [u8] = frame.plane_mut(2).unwrap();
            vs_b.copy_from_slice(b);
        }
    }
}

impl<'core> VSSourceFilter<'core> {
    fn avframe_to_vsframe(
        &self,
        core: CoreRef<'core>,
        raw_av_frame: *mut AVFrame,
    ) -> FrameRefMut<'core> {
        unsafe {
            let av_frame = &(*raw_av_frame);

            //println!("data {:?}", av_frame.data);
            //println!("linesz {:?}", av_frame.linesize);

            let format = core.get_format(self.format_id).unwrap();
            let mut frame = FrameRefMut::new_uninitialized(core, None, format, self.resolution);

            extract_framedata(av_frame, &mut frame);
            {
                let mut props = frame.props_mut();
                match av_frame.pict_type {
                    ffmpeg_sys::AVPictureType::AV_PICTURE_TYPE_I => props
                        .append_data("_PictType", "I".to_owned().as_bytes())
                        .unwrap(),
                    ffmpeg_sys::AVPictureType::AV_PICTURE_TYPE_P => props
                        .append_data("_PictType", "P".to_owned().as_bytes())
                        .unwrap(),
                    ffmpeg_sys::AVPictureType::AV_PICTURE_TYPE_B => props
                        .append_data("_PictType", "B".to_owned().as_bytes())
                        .unwrap(),

                    _ => {}
                };
            }

            frame
        }
    }
}

impl<'core> Filter<'core> for VSSourceFilter<'core> {
    fn video_info(&self, _api: API, core: CoreRef<'core>) -> Vec<VideoInfo<'core>> {
        let info = VideoInfo {
            format: core.get_format(self.format_id).unwrap().into(),
            framerate: self.framerate.into(),
            resolution: self.resolution.into(),
            num_frames: self.num_frames.into(),
            flags: Flags::empty(),
        };
        vec![info]
    }

    fn get_frame_initial(
        &self,
        _api: API,
        core: CoreRef<'core>,
        _context: FrameContext,
        n: usize,
    ) -> Result<Option<FrameRef<'core>>, Error> {
        unsafe {
            //force singlethreadeness
            let mut a = self.global_lock.lock().unwrap();
            *a = !*a;

            let ret = predict::get_frame_dump_info(n as _, &self.pred);

            if let Some(c) = {
                //this is extraordinarily stupid, why do i have to do this
                let asd = self.cached_decoder_info.read().unwrap();
                let r = asd.clone();
                drop(asd);
                r
            } {
                let n = n as u64;

                let cache_end = c.current_idx + c.frames_left;
                if n >= c.current_idx && n < cache_end {
                    let mut lck = self.cached_decoder.lock().unwrap();
                    let lll = lck.as_mut().unwrap();
                    let d = lll.decoder.as_mut().unwrap();
                    let mut d = d.lock().unwrap();

                    let frame_offset = n - c.current_idx;

                    //let old = std::time::Instant::now();
                    let av_frame = ffmpeg_stuff::read_nth_frame(&mut d, frame_offset);

                    let frame = self.avframe_to_vsframe(core, av_frame.frame);

                    //let now = std::time::Instant::now();
                    //println!("caacahe frame {}", (now - old).as_millis());

                    let frames_getted = frame_offset + 1;

                    //println!("ge {}", frames_getted);
                    //update
                    if (lll.frames_left as i64 - frames_getted as i64) <= 0 {
                        *self.cached_decoder_info.write().unwrap() = None;
                        drop(d);
                        drop(lll);
                        *lck = None;
                    } else {
                        lll.current_idx += frames_getted;
                        lll.frames_left -= frames_getted;
                        let mut lck = self.cached_decoder_info.write().unwrap();
                        let cc = lck.as_mut().unwrap();
                        cc.current_idx = lll.current_idx;
                        cc.frames_left = lll.frames_left;
                    }
                    //println!("Return");
                    return Ok(Some(frame.into()));
                }
            }

            //let old = std::time::Instant::now();
            let mut new_decoder = ffmpeg_stuff::setup_decoder(
                Arc::new(Mutex::new(fcache::FCached::new(self.pred_inner.clone()))),
                self.pred[ret.0].spn,
                self.clpi.stream_pid,
            )
            .unwrap();

            // let now = std::time::Instant::now();
            // println!("decoder setup took {}", (now - old).as_millis());
            let av_frame = ffmpeg_stuff::read_nth_frame(&mut new_decoder, ret.1);
            let frame = self.avframe_to_vsframe(core, av_frame.frame);

            // let now = std::time::Instant::now();
            // println!("frame took {}", (now - old).as_millis());

            let mut lck = self.cached_decoder.lock().unwrap();

            let aaa = if ret.0 == self.pred.len() {
                self.num_frames as u64 - self.pred[ret.0].number - 1
            } else {
                self.pred[ret.0 + 1].number - self.pred[ret.0].number - 1
            };
            let chc = CachedDecoder::new(
                new_decoder,
                self.pred[ret.0].number,
                self.pred[ret.0].number + ret.1 + 1,
                aaa - ret.1,
            );
            let mut w = self.cached_decoder_info.write().unwrap();

            *w = Some(CachedDecoder {
                decoder: None,
                ..chc.clone()
            });
            *lck = Some(chc);

            Ok(Some(frame.into()))
        }
        //     self.source.request_frame_filter(context, n);
    }

    fn get_frame(
        &self,
        _api: API,
        _core: CoreRef<'core>,
        _context: FrameContext,
        _n: usize,
    ) -> Result<FrameRef<'core>, Error> {
        unreachable!()
    }
}

make_filter_function! {
    SourceFunction, "Source"

    fn create_passthrough<'core>(
        _api: API,
        _core: CoreRef<'core>,
        name: &[u8],
    ) -> Result<Option<Box<dyn Filter<'core> + 'core>>, Error> {
        let name = unsafe { CStr::from_ptr(name.as_ptr() as _) };

        let stream_path = PathBuf::from(name.to_str().unwrap());
        if !stream_path.exists() {
            bail!("Stream does not exists");
        }
        let stream_idx_number = stream_path.file_stem().unwrap();

        let clip_info = stream_path.parent().unwrap().parent().unwrap().join("CLIPINF").join(&format!("{}.clpi",stream_idx_number.to_str().unwrap()));
        if !clip_info.exists() {
            bail!("CLIPINFO does not exists");
        }

        let clpi = clpi::read_clpi(&mut (File::open(clip_info)?)).unwrap();
        let pred = predict::predict_frame_numbers(&clpi.combined);

        let inner = Arc::new(Mutex::new(fcache::FileCacheBackend::new(File::open(&stream_path).unwrap(),&pred)));

        let caached = fcache::FCached::new(inner.clone());
        let caached = Arc::new(Mutex::new(caached));

        //Get format and end frames
        let (setup,framecnt) = unsafe {
            let mut setup = ffmpeg_stuff::setup_decoder(caached.clone(),pred[pred.len()-1].spn,clpi.stream_pid).unwrap();
            let framecnt = ffmpeg_stuff::analyse_end(&mut setup);
            (setup,framecnt)
        };


        Ok(Some(Box::new(VSSourceFilter {
            framerate: Framerate {
                numerator: setup.frame_rate_n as _,
                denominator: setup.frame_rate_d as _,
            },
            resolution: Resolution {
                width: framecnt.width as usize,
                height: framecnt.height as usize
            }.into(),
            num_frames: pred[pred.len()-1].number as usize + framecnt.last_packet_frame_cnt as usize,
            format_id: PresetFormat::YUV420P8.into(),
            pred_inner: inner,
            pred: pred,
            a: Default::default(),
            clpi: clpi,
            cached_decoder:      Arc::new(Mutex::new(None)),
            cached_decoder_info: RwLock::new(None),
            global_lock: Arc::new(Mutex::new(false)),
        })))
    }
}

export_vapoursynth_plugin! {
    Metadata {
        identifier: "com.example.bdngsp",
        namespace: "bdngsp",
        name: "BD source Not Good but should Suffice for Preview",
        read_only: true,
    },
    [SourceFunction::new()]
}
