use super::clpi::*;

#[derive(Clone)]
pub struct PredictedKeyFrame {
    pub number: u64,
    pub pts: u64,
    pub spn: u32,
}

pub fn predict_frame_numbers(a: &[Combined]) -> Vec<PredictedKeyFrame> {
    let frate = 24000.0 / 1001.0;

    let frame_0 = a[0].pts;

    a.iter()
        .map(|e| PredictedKeyFrame {
            number: (((e.pts - frame_0) as f64 / 45_000.0) * frate).round() as _,
            pts: e.pts,
            spn: e.spn,
        })
        .collect()
}

pub fn get_frame_dump_info(num: u64, prd: &[PredictedKeyFrame]) -> (usize, u64) {
    let mut closest = 0;
    for (i, a) in prd.iter().enumerate() {
        if a.number > num {
            closest = i - 1;
            break;
        }
    }
    let frames_offset = num - prd[closest].number;
    (closest, frames_offset)
}
/*
pub fn dump_frame(
    num: u64,
    stream_file: Arc<Mutex<super::fcache::FCached>>,
    clpi: &CLPIResult,
    prd: &[PredictedKeyFrame],
    out_path: &str,
) {
    let mut closest = 0;
    for (i, a) in prd.iter().enumerate() {
        if a.number > num {
            closest = i - 1;
            break;
        }
    }
    dbg!(num, prd[closest].number);
    let frames_offset = num - prd[closest].number;
    super::ffmpeg_stuff::do_stuff(
        stream_file,
        clpi.stream_pid,
        prd[closest].spn,
        frames_offset,
        out_path,
    )
    .unwrap();
}
*/
