use std::{
    error::Error,
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
};

use byteorder::{ReadBytesExt, BE};

struct Coarse {
    fine_id: u32,
    pts: u16,
    spn: u32,
}
struct Fine {
    //reserved: u8,
    //endpos: u8,
    pts: u16,
    spn: u32,
}

pub struct Combined {
    pub pts: u64,
    pub spn: u32,
}

pub struct CLPIResult {
    pub stream_pid: u16,
    pub combined: Vec<Combined>,
}

fn parse_cpi(buf: &[u8]) -> Result<CLPIResult, Box<dyn Error>> {
    let stream_pid;
    let mut b = Cursor::new(buf);

    //junk
    b.read(&mut [0u8; 3])?;

    let num_stream_pid = b.read_u8()?;

    //println!("num_stream_pid: {}", num_stream_pid);

    for _ in 0..num_stream_pid {
        let mut blob = [0u8; 12];
        b.read(&mut blob)?;
        let mut bread = bitreader::BitReader::new(&blob);
        stream_pid = bread.read_u16(16)?;
        bread.skip(10)?; //Resredev
        let _stream_type = bread.read_u8(4)?;
        let coarse_entries = bread.read_u16(16)?;
        let fine_entries = bread.read_u32(18)?;
        let epmap_addr = bread.read_u32(32)?;

        //println!("  stream_pid: {}", stream_pid);
        //println!("  stream_type: {}", stream_type);
        //println!("  coarse_entries: {}", coarse_entries);
        //println!("  fine_entries: {}", fine_entries);
        //println!("  epmap_addr: {}", epmap_addr);

        let real_addr = epmap_addr + 2; // start right after type
        b.seek(SeekFrom::Start(real_addr as _))?;
        let fine_table_addr = b.read_u32::<BE>()?;
        //println!("fine: {}", fine_table_addr);

        let mut coarse = Vec::new();
        let mut fines = Vec::new();

        coarse.reserve(coarse_entries as usize);
        fines.reserve(fine_entries as usize);
        for xx in 0..coarse_entries {
            let mut blob = [0u8; 8];
            b.read(&mut blob)?;
            let mut bread = bitreader::BitReader::new(&blob);

            let fine_id_ref = bread.read_u32(18)?;
            let pts = bread.read_u16(14)?;
            let spn = bread.read_u32(32)?;
            //  if xx < 10 || xx  > coarse_entries  -10{
            //      println!("{} {} {}",fine_id_ref,pts,spn);
            //  }
            let _ = xx;
            coarse.push(Coarse {
                fine_id: fine_id_ref,
                pts,
                spn,
            });
        }

        let fine_real_addr = real_addr + fine_table_addr;
        b.seek(SeekFrom::Start(fine_real_addr as _))?;

        //println!("");
        //println!("FINE");
        //println!("");
        for xx in 0..fine_entries {
            let mut blob = [0u8; 4];
            b.read(&mut blob)?;
            let mut bread = bitreader::BitReader::new(&blob);
            let _reserved = bread.read_u8(1)?;
            let _end_pos_offset = bread.read_u8(3)?;
            let pts_fine = bread.read_u16(11)?;
            let spn_fine = bread.read_u32(17)?;

            //if xx < 10 || xx  > fine_entries  -10{
            //    println!("{} {}",pts_fine,spn_fine);
            //}
            let _ = xx;

            //println!("{}",spn_fine);
            fines.push(Fine {
                //reserved: reserved,
                // endpos: end_pos_offset,
                pts: pts_fine,
                spn: spn_fine,
            })
        }

        let mut combined = Vec::with_capacity(fine_entries as usize);

        for (ii, c) in coarse.iter().enumerate() {
            let start = c.fine_id;
            let end_fine = if ii != coarse_entries as usize - 1 {
                coarse[ii + 1].fine_id
            } else {
                fine_entries
            };
            let coarse_spn = c.spn as u32 & (!0x1FFFF);
            let coarse_pts = (c.pts as u64 & (!(0x01 as u64))) << 18;

            for fine in start..end_fine {
                let fine_entry = &fines[fine as usize];

                let spn = coarse_spn + fine_entry.spn;
                let pts = coarse_pts + ((fine_entry.pts as u64) << 8);
                combined.push(Combined { pts, spn })
            }
        }
        //for i in 0..50 {
        //    let c = &combined[i];
        //    println!("{} {} {}", c.spn, c.pts, c.pts as f32 / 45_000.0);
        //}
        //combined.sort_by(|a,b| a.spn.partial_cmp(&b.spn).unwrap());
        return Ok(CLPIResult {
            stream_pid,
            combined,
        });
    }
    Err(Box::new(simple_error::simple_error!("Errr")))
}

pub fn read_clpi(clip_file: &mut File) -> Result<CLPIResult, Box<dyn Error>> {
    clip_file.seek(SeekFrom::Start(0))?;
    let mut asd = [0u8; 4];
    //TypeIndecators
    clip_file.read(&mut asd)?;
    clip_file.read(&mut asd)?;

    let _seq_info_addr = clip_file.read_u32::<BE>()?;
    let _prog_info_addr = clip_file.read_u32::<BE>()?;
    let cpi_info_addr = clip_file.read_u32::<BE>()?;
    let _clip_mark_info_addr = clip_file.read_u32::<BE>()?;

    //println!(
    //    "seq_info_addr {} prog_info_addr {}",
    //    seq_info_addr, prog_info_addr
    //);
    //println!(
    //    "cpi_info_addr {} clip_mark_info_addr {}",
    //    cpi_info_addr, clip_mark_info_addr
    //);

    clip_file.seek(SeekFrom::Start(cpi_info_addr as u64))?;

    let cpi_length = clip_file.read_u32::<BE>()?;
    //println!("cpi len {}", cpi_length);
    //println!(
    //    "{}",
    //    cpi_length as u64 + (clip_file.stream_position()?) as u64
    //);
    let mut cpi = vec![0u8; cpi_length as usize];
    clip_file.read(&mut cpi)?;
    //println!("Reda");

    parse_cpi(&cpi)
}
