use std::{
    error::Error,
    ffi::{c_void, CString},
    sync::{Arc, Mutex},
};

use ffmpeg_sys::{AVCodec, AVCodecContext, AVFormatContext, AVFrame, AVPixelFormat};

pub use ffmpeg_sys;
use ffmpeg_sys::AVCodecID::*;
use ffmpeg_sys::*;

pub struct CustomFileContext {
    pub buffer: [u8; 32 * 1024],
    pub the_file: Arc<Mutex<super::fcache::FCached>>,
}

pub struct DecoderSetup {
    pub incodec: *mut AVCodec,
    pub avctx: *mut AVCodecContext,
    pub fctx: *mut AVFormatContext,
    pub inpkt: *mut AVPacket,
    pub stream_index: i32,
    pub frame_rate_d: u32,
    pub frame_rate_n: u32,
    pub rc: Arc<CustomFileContext>,
}

unsafe impl Sync for DecoderSetup {}

impl Drop for DecoderSetup {
    fn drop(&mut self) {
        unsafe {
            avcodec_free_context(&mut self.avctx);
            avformat_free_context(self.fctx);
        }
    }
}

pub unsafe fn setup_decoder(
    stream_file: Arc<Mutex<super::fcache::FCached>>,
    start_spn: u32,
    stream_pid: u16,
) -> Result<DecoderSetup, Box<dyn Error>> {
    unsafe extern "C" fn file_seek(opaque: *mut c_void, offset: i64, whence: i32) -> i64 {
        let refa = opaque as *mut CustomFileContext;
        let refa = &mut (*refa);
        if whence == SEEK_SET {
            let mut ff = refa.the_file.lock().unwrap();

            ff.seek(offset as _);
            0
        } else if whence == AVSEEK_SIZE {
            0
        } else {
            panic!("Invalid seek");
        }
    }

    unsafe extern "C" fn read_packet(opaque: *mut c_void, buf: *mut u8, size: i32) -> i32 {
        // println!("read_packet {}",size);
        let refa = opaque as *mut CustomFileContext;
        let refa = &mut (*refa);
        let slc = std::slice::from_raw_parts_mut(buf, size as usize);
        let mut ff = refa.the_file.lock().unwrap();

        let ree = ff.read(slc);
        ree as _
    }

    let incodec = avcodec_find_decoder(AV_CODEC_ID_H264);
    let avctx = avcodec_alloc_context3(incodec);
    let in_thing = avcodec_open2(avctx, incodec, std::ptr::null_mut::<*mut AVDictionary>());

    if in_thing != 0 {
        return Err(Box::new(simple_error::simple_error!(
            "avcodec_open2: {}",
            in_thing
        )));
    }
    let mut fctx = avformat_alloc_context();

    (*fctx).iformat = av_find_input_format(CString::new("mpegts")?.as_ptr());

    let rc = Arc::new(CustomFileContext {
        buffer: [0u8; 32 * 1024],
        the_file: stream_file,
    });
    (*fctx).pb = avio_alloc_context(
        rc.buffer.as_ptr() as _,
        rc.buffer.len() as _,
        0,
        (rc.as_ref() as *const CustomFileContext) as *mut c_void,
        Some(read_packet),
        None,
        Some(file_seek),
    );
    let seek_dst = start_spn as i64 * 192;
    let rr = avio_seek((*fctx).pb, seek_dst, SEEK_SET);
    if rr != seek_dst {
        return Err(Box::new(simple_error::simple_error!("avio_seek: {}", rr)));
    }
    let av_ret = avformat_open_input(
        &mut fctx,
        CString::new("dunno")?.as_ptr(),
        std::ptr::null_mut() as _,
        std::ptr::null_mut() as _,
    );
    if av_ret != 0 {
        return Err(Box::new(simple_error::simple_error!(
            "avformat_open_input: {}",
            av_ret
        )));
    }

    avcodec_flush_buffers(avctx);
    avformat_find_stream_info(fctx, std::ptr::null_mut() as _);

    let mut stream_index = 0;

    let mut frame_rate_d = 0;
    let mut frame_rate_n = 0;
    for i in 0..(*fctx).nb_streams {
        let strm = (*(*fctx).streams).offset(i as _);
        if (*strm).id == stream_pid as _ {
            stream_index = i as i32;
            let asd = (*strm).r_frame_rate;
            frame_rate_d = asd.den as u32;
            frame_rate_n = asd.num as u32;
        }
    }
    let inpkt = av_packet_alloc();

    Ok(DecoderSetup {
        avctx,
        fctx,
        incodec,
        stream_index,
        rc,
        frame_rate_d,
        frame_rate_n,
        inpkt,
    })
}

pub struct AutoFreeFrame {
    pub frame: *mut AVFrame,
}

impl Drop for AutoFreeFrame {
    fn drop(&mut self) {
        unsafe {
            ffmpeg_sys::av_frame_free(&mut self.frame);
        }
    }
}

pub unsafe fn read_nth_frame(our_shit: &mut DecoderSetup, frame_offset: u64) -> AutoFreeFrame {
    use ffmpeg_sys::*;
    //println!("{}", frame_offset);
    // let inpkt = av_packet_alloc();
    let inpkt = our_shit.inpkt;

    let fctx = our_shit.fctx;
    let avctx = our_shit.avctx;

    if (*inpkt).pts < 0 {
        //println!("Did read");
        av_read_frame(fctx, inpkt);
    }

    while (*inpkt).stream_index != our_shit.stream_index {
        av_packet_unref(inpkt);
        av_read_frame(fctx, inpkt);
    }

    let out = av_frame_alloc();
    // println!("iffset {}", frame_offset);
    for xx in 0..(frame_offset + 1) {
        while avcodec_receive_frame(avctx, out) == AVERROR(EAGAIN) {
            avcodec_send_packet(avctx, inpkt);

            loop {
                av_packet_unref(inpkt);
                av_read_frame(fctx, inpkt);
                if (*inpkt).stream_index == our_shit.stream_index {
                    break;
                }
            }
        }
        if xx != frame_offset {
            av_frame_unref(out);
        }
    }
    AutoFreeFrame { frame: out }
}

#[derive(Debug)]
pub struct Analisys {
    pub width: u64,
    pub height: u64,
    pub format: AVPixelFormat,

    pub last_packet_frame_cnt: u64,
}

pub unsafe fn analyse_end(our_shit: &mut DecoderSetup) -> Analisys {
    use ffmpeg_sys::*;
    let mut inpkt = av_packet_alloc();

    let fctx = our_shit.fctx;
    let avctx = our_shit.avctx;

    av_read_frame(fctx, inpkt);

    while (*inpkt).stream_index != our_shit.stream_index {
        av_packet_unref(inpkt);
        av_read_frame(fctx, inpkt);
    }
    let mut rett = Analisys {
        width: 0,
        height: 0,
        format: AVPixelFormat::AV_PIX_FMT_YUV420P,
        last_packet_frame_cnt: 0,
    };

    let mut out = av_frame_alloc();
    'asd: loop {
        while avcodec_receive_frame(avctx, out) == AVERROR(EAGAIN) {
            avcodec_send_packet(avctx, inpkt);

            loop {
                av_packet_unref(inpkt);
                let read_frame_ret = av_read_frame(fctx, inpkt);
                if read_frame_ret != 0 {
                    break 'asd;
                }

                if (*inpkt).stream_index == our_shit.stream_index {
                    break;
                }
            }
        }
        if rett.last_packet_frame_cnt == 0 {
            rett.width = (*out).width as u64;
            rett.height = (*out).height as u64;
            const FMT_420: i32 = AVPixelFormat::AV_PIX_FMT_YUV420P as i32;

            rett.format = match (*out).format {
                FMT_420 => AVPixelFormat::AV_PIX_FMT_YUV420P,
                _ => panic!("Unsopported pixel fmt"),
            };
        }

        rett.last_packet_frame_cnt += 1;
    }
    av_packet_free((&mut inpkt) as _);
    av_frame_free((&mut out) as _);
    rett
}

/*
fn ouput_y4m(f: &AVFrame, out_path: &str) {
    let mut file = File::create(out_path).unwrap();
    let mut header = String::new();
    header += "YUV4MPEG2 ";
    header += &format!("W{} ", f.width);
    header += &format!("H{} ", f.height);
    header += "F24000:1001 ";
    header += "C420";
    header += "\n";
    file.write(header.as_bytes()).unwrap();

    dbg!(f.linesize);
    dbg!(f.data);

    unsafe {
        let y = std::slice::from_raw_parts(f.data[0], f.linesize[0] as usize * f.height as usize);
        let c =
            std::slice::from_raw_parts(f.data[1], f.linesize[1] as usize * (f.height / 2) as usize);
        let b =
            std::slice::from_raw_parts(f.data[2], f.linesize[2] as usize * (f.height / 2) as usize);
        // let r = std::slice::from_raw_parts(f.data[3] , f.linesize[3] as usize * f.height as usize);

        file.write("FRAME\n".to_owned().as_bytes()).unwrap();
        file.write(y).unwrap();
        file.write(c).unwrap();
        file.write(b).unwrap();
        //file.write(r).unwrap();
    }

    drop(file);
}
*/
