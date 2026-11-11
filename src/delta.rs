// Delta frames of the ffmpeg overlay (`/ffmpeg/frames?…&delta=1`): instead of
// the whole yuva420p frame at every output frame (1080p: 3 MB, at 50 fps
// 150 MB/s through the sidecar, the loopback and `vizrs-ffmpeg`), each record
// carries only the rows that changed since the frame before. A standing
// graphic is 8 bytes a frame, a crawl the rows of its band. `vizrs-ffmpeg`
// keeps the frame, patches it and hands ffmpeg the whole frame as before
// (rawvideo needs every frame).
//
// A record: `VZD1`, the number of spans (u32 LE), each span's offset and
// length in the frame (u32 LE each), then the bytes of the spans one after
// another. The first record of a stream is the whole frame (one span); no
// span = the frame before again. Spans are runs of whole rows of one plane
// (Y, U, V, A), runs closer than `GAP` rows joined, at most `MAX_SPANS`.

use std::io::{self, Read, Write};

pub const MAGIC: &[u8; 4] = b"VZD1";
/// The record of a frame the same as the one before.
pub const UNCHANGED: [u8; 8] = *b"VZD1\0\0\0\0";
/// Changed rows of a plane this close together go as one span.
const GAP: usize = 8;
/// Most spans in a record; more are joined into fewer, larger ones.
const MAX_SPANS: usize = 64;

/// The planes of a yuva420p frame: (offset, bytes per row, rows).
fn planes(width: u32, height: u32) -> [(usize, usize, usize); 4] {
    let (w, h) = (width as usize, height as usize);
    let (w2, h2) = (w.div_ceil(2), h.div_ceil(2));
    [(0, w, h), (w * h, w2, h2), (w * h + w2 * h2, w2, h2), (w * h + 2 * w2 * h2, w, h)]
}

/// The spans (offset, length) of `new` that differ from `old`, frames of
/// `width` x `height`; `old` of another length: the whole frame.
pub fn spans(old: &[u8], new: &[u8], width: u32, height: u32) -> Vec<(usize, usize)> {
    if old.len() != new.len() {
        return vec![(0, new.len())];
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (offset, stride, rows) in planes(width, height) {
        if stride == 0 {
            continue;
        }
        let mut run: Option<(usize, usize)> = None;
        for row in 0..rows {
            let at = offset + row * stride;
            if old[at..at + stride] == new[at..at + stride] {
                continue;
            }
            run = match run {
                Some((first, last)) if row - last <= GAP => Some((first, row)),
                Some((first, last)) => {
                    out.push((offset + first * stride, (last + 1 - first) * stride));
                    Some((row, row))
                }
                None => Some((row, row)),
            };
        }
        if let Some((first, last)) = run {
            out.push((offset + first * stride, (last + 1 - first) * stride));
        }
    }
    // Too many spans: the closest neighbours joined until few enough.
    while out.len() > MAX_SPANS {
        let i = (1..out.len()).min_by_key(|&i| out[i].0 - (out[i - 1].0 + out[i - 1].1)).expect("several spans");
        let (start, _) = out[i - 1];
        let end = out[i].0 + out[i].1;
        out[i - 1] = (start, end - start);
        out.remove(i);
    }
    out
}

/// Writes the record of `new` after `old` (`None` = the first of the stream: the whole frame).
pub fn write_record(out: &mut dyn Write, old: Option<&[u8]>, new: &[u8], width: u32, height: u32) -> io::Result<()> {
    let spans = match old {
        Some(old) => spans(old, new, width, height),
        None => vec![(0, new.len())],
    };
    let mut head = Vec::with_capacity(8 + spans.len() * 8);
    head.extend_from_slice(MAGIC);
    head.extend_from_slice(&(spans.len() as u32).to_le_bytes());
    for &(offset, length) in &spans {
        head.extend_from_slice(&(offset as u32).to_le_bytes());
        head.extend_from_slice(&(length as u32).to_le_bytes());
    }
    out.write_all(&head)?;
    for (offset, length) in spans {
        out.write_all(&new[offset..offset + length])?;
    }
    Ok(())
}

/// Reads one record into `frame` (the frame before, patched in place).
pub fn read_record(source: &mut dyn Read, frame: &mut [u8]) -> io::Result<()> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, format!("delta frame: {what}"));
    let mut head = [0u8; 8];
    source.read_exact(&mut head)?;
    if &head[..4] != MAGIC {
        return Err(bad("no record"));
    }
    let count = u32::from_le_bytes(head[4..8].try_into().expect("4 bytes")) as usize;
    if count > MAX_SPANS.max(4) * 2 {
        return Err(bad("too many spans"));
    }
    let mut table = vec![0u8; count * 8];
    source.read_exact(&mut table)?;
    for entry in table.chunks_exact(8) {
        let offset = u32::from_le_bytes(entry[..4].try_into().expect("4 bytes")) as usize;
        let length = u32::from_le_bytes(entry[4..].try_into().expect("4 bytes")) as usize;
        let end = offset.checked_add(length).filter(|&end| end <= frame.len()).ok_or_else(|| bad("span outside the frame"))?;
        source.read_exact(&mut frame[offset..end])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 frame: Y 16 bytes, U 4, V 4, A 16.
    fn frame(fill: u8) -> Vec<u8> {
        vec![fill; 40]
    }

    #[test]
    fn records_carry_the_changed_rows_and_rebuild_the_frame() {
        let first = frame(0);
        let mut second = first.clone();
        second[4] = 7; // Y row 1
        second[37] = 9; // A row 3
        assert_eq!(spans(&first, &second, 4, 4), [(4, 4), (36, 4)]);
        // rows close together are one span
        let mut near = first.clone();
        near[0] = 1;
        near[12] = 1;
        assert_eq!(spans(&first, &near, 4, 4), [(0, 16)]);

        let mut stream = Vec::new();
        write_record(&mut stream, None, &first, 4, 4).unwrap();
        write_record(&mut stream, Some(&first), &second, 4, 4).unwrap();
        write_record(&mut stream, Some(&second), &second, 4, 4).unwrap();
        assert_eq!(stream[stream.len() - 8..], UNCHANGED);
        // whole frame, two rows, nothing
        assert_eq!(stream.len(), (8 + 8 + 40) + (8 + 16 + 8) + 8);

        let mut shown = frame(99);
        let mut source = &stream[..];
        read_record(&mut source, &mut shown).unwrap();
        assert_eq!(shown, first);
        read_record(&mut source, &mut shown).unwrap();
        assert_eq!(shown, second);
        read_record(&mut source, &mut shown).unwrap();
        assert_eq!(shown, second);
        assert!(source.is_empty());
    }

    #[test]
    fn many_changes_join_into_few_spans_and_bad_records_are_refused() {
        // every tenth row of Y and A changed, too far apart to join: 80 runs, joined to the limit
        let (w, h) = (2u32, 400u32);
        let old = vec![0u8; 2 * 400 * 2 + 2 * 200];
        let mut new = old.clone();
        for row in (0..400).step_by(10) {
            new[row * 2] = 1;
            new[1200 + row * 2] = 1;
        }
        let spans = spans(&old, &new, w, h);
        assert_eq!(spans.len(), MAX_SPANS);
        let mut rebuilt = old.clone();
        for (offset, length) in spans {
            rebuilt[offset..offset + length].copy_from_slice(&new[offset..offset + length]);
        }
        assert_eq!(rebuilt, new);

        let mut shown = frame(0);
        let outside = [&MAGIC[..], &1u32.to_le_bytes(), &36u32.to_le_bytes(), &8u32.to_le_bytes()].concat();
        assert!(read_record(&mut &outside[..], &mut shown).is_err());
        assert!(read_record(&mut &b"RAW!\0\0\0\0"[..], &mut shown).is_err());
    }
}
