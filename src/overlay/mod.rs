// ffmpeg for ErsatzTV that overlays the VizRS graphics itself (binary
// vizrs-ffmpeg, set as the FFmpeg path in ErsatzTV; DOKUMENTACJA §1). Every
// call goes to the real ffmpeg unchanged, except a transcode that reads the
// ErsatzTV graphics canvas (`-i http://localhost:8409/internal/graphics/<channel>/<item>?...`):
//
// 1. the canvas is opened here and drained, so ErsatzTV still starts the VizRS
//    script element with the item's template data (commands, ads, EPG);
// 2. the sidecar takes that script session over (`GET /ffmpeg`): ErsatzTV gets
//    no more packets and its graphics engine waits at no cost;
// 3. ffmpeg reads raw yuva420p frames from the sidecar instead of the canvas
//    (`GET /ffmpeg/frames`), through a socket passed as `pipe:N`.
//
// ErsatzTV's own graphics elements of that item (text, image in the canvas)
// are not drawn then; its watermarks are, by ffmpeg itself, as VizRS Bridge
// says (watermark.rs). Whenever the sidecar does not answer, the original
// command runs: the item plays with ErsatzTV's graphics path.
//
// With VIZRS_FFMPEG_FPS every transcode to HLS runs at that rate (ErsatzTV
// takes the rate from the media): ErsatzTV's conversion of the media's rate
// (`[0:0]fps=25,…`, Normalize Framerate) converts to N instead, the video gets
// `fps=N` before the graphics overlay, so the graphics move at N frames per
// second, and the output `-r N`.
// With VIZRS_FFMPEG_PRESET a software encoder (libx264, libx265) without a
// preset gets `-preset:v <preset>`: ErsatzTV's next streaming engine leaves
// the preset of the FFmpeg profile out, so x264 runs at `medium`.
// With VIZRS_FFMPEG_READRATE_CATCHUP an HLS playlist input read at `-readrate`
// (ErsatzTV's MPEG-TS stream of a channel, read from its own HLS) gets
// `-readrate_catchup <rate>`: without it ffmpeg never makes up a stall at the
// live edge, so the stream falls further behind real time with every one.
// With VIZRS_FFMPEG_FPS_BLEND=1 the frame rate conversion of the media in a
// transcode to HLS (`[0:0]fps=25,…`, ErsatzTV's Normalize Framerate) blends
// neighbouring frames (`framerate=fps=25`): 60 or 30 fps media at 25 fps take
// every 2nd and 3rd frame in turn with `fps`, and motion judders. Frames that
// fall on an output frame (50 or 25 fps media) pass unchanged. Media no faster
// than the output keeps `fps` (its frames repeated, not blended): its rate
// comes from ffprobe, run in the background for the first piece of a media and
// cached for the next ones (through ErsatzTV's proxy a probe takes seconds).
//
//     VIZRS_CG_URL       sidecar, e.g. http://cg:7700 (empty: no overlay)
//     VIZRS_FFMPEG       real ffmpeg (default /usr/local/bin/ffmpeg)
//     VIZRS_FFMPEG_FPS   channel frame rate, e.g. 50 (empty: the media's rate)
//     VIZRS_FFMPEG_PRESET  x264/x265 preset, e.g. veryfast (empty: ErsatzTV's)
//     VIZRS_FFMPEG_X264_PARAMS  libx264 options, e.g. bframes=0:subme=0 (empty: none)
//     VIZRS_FFMPEG_SETTINGS  encoder settings set in VizRS Live (x264 preset, H.264 profile,
//                        x264 options, NVENC and QSV presets, hardware B-frames); each key
//                        the file has instead of the variables
//                        (default /config/vizrs-live/encoder.json, src/encoder.rs)
//     VIZRS_FFMPEG_READRATE_CATCHUP  catch-up rate of HLS inputs, e.g. 2 (empty: none)
//     VIZRS_FFMPEG_FPS_BLEND  1 = blend frames when converting the media's rate
//     VIZRS_FFMPEG_INGEST  1 = live ingest of the sidecar takes the channel over
//     VIZRS_FFMPEG_STANDBY image of a live programme without its source (empty: black)
//     VIZRS_FFMPEG_CUDA  encode = with an NVENC encoder, decoding and filters on the CPU;
//                        overlay = decoding on the CPU, filters and overlay on the card;
//                        auto = overlay while the card has VIZRS_FFMPEG_CUDA_MIN_FREE MiB free, encode below
//     VIZRS_FFMPEG_CUDA_MIN_FREE  free GPU memory in MiB for overlay in auto (default 400)
//     VIZRS_FFMPEG_NVENC_BF  B-frames of hardware encoders (NVENC, QSV, VAAPI), e.g. 0 (empty: ffmpeg's)
//     VIZRS_FFMPEG_SYNC_RAM  1 = with CUDA overlay, media frames wait for the graphics in RAM, not on the card
//     VIZRS_FFMPEG_NO_DISCARDCORRUPT  1 = a stream copy of an HLS playlist keeps the frame at each join of pieces
//
// The graphics of a canvas transcode with `-init_hw_device cuda` go to the
// card through `hwupload` (that device), not `hwupload_cuda`: each
// `hwupload_cuda` opens a CUDA context of its own, about 28 MiB of GPU memory
// per graphics area. With VIZRS_FFMPEG_NVENC_BF an NVENC encoder (`*_nvenc`)
// without `-bf` gets `-bf:v <n>`: ffmpeg's default B-frames hold reference
// frames on the card (Quadro P400, 1080p50: 139 MiB with the default, 99 with
// 1, 79 with 0).
//
// With VIZRS_FFMPEG_CUDA=encode a command that encodes with NVENC (`*_nvenc`)
// loses ErsatzTV's CUDA decoding and filters (`-hwaccel cuda`,
// `-init_hw_device cuda`, `hwupload`, `scale_cuda`, `overlay_cuda`, `pad_cuda`,
// `yadif_cuda`, `bwdif_cuda` become their CPU filters): only the encoder stays
// on the card. A process then takes about half the GPU memory (720p: 118 MiB
// instead of 218 MiB), for some CPU – on a card shared with a desktop, two
// pieces of a channel at once (one ending, the next starting) no longer run
// out of memory (`CUDA_ERROR_OUT_OF_MEMORY`).
//
// With VIZRS_FFMPEG_CUDA=overlay only ErsatzTV's CUDA decoding goes (`-hwaccel
// cuda` and its options): the decoded frames are uploaded (`format=…,hwupload`)
// where ErsatzTV's filters expect them on the card, scaling and the overlay of
// the graphics (`overlay_cuda`) stay there, and the frames for the encoder come
// back to the CPU (`hwdownload`): waiting for the muxer (3 s of `loudnorm`
// lookahead) on the card they ran it out of memory. A canvas transcode then
// reads the media at `-readrate 1` when ErsatzTV gives it no rate
// (`paced_media`), with an initial burst of the graphics the sidecar has
// ready at once (`ahead` of `/ffmpeg`): the graphics come in the channel's
// time anyway, media decoded ahead of them waited on the card in front of
// `overlay_cuda`, and without the burst a piece started late never made up
// the time lost (HLS at 0.93x, the player's buffer ran out). Measured on a Quadro P400
// (720p25 media to 1080p50 with a graphics strip): 0.33 of a core instead of
// 1.40 with `encode`, about the same GPU memory (213 MiB instead of 206).
//
// With VIZRS_FFMPEG_SYNC_RAM=1 and `overlay` (also chosen by `auto`) the
// media frames wait for the graphics of their moment in RAM, before they go
// to the card (`media_waits_in_ram`): `loudnorm` gives its audio about 3 s
// late, so ffmpeg decodes the media that far ahead, and each frame uploaded
// at once waited in front of `overlay_cuda` for graphics that come in the
// channel's time – about 3 MiB a 1080p frame, up to 560 MiB more on the card,
// and a burst ran a Quadro P400 out of memory. Measured (1080p50, loudnorm,
// graphics from a pipe): 91 MiB instead of 667, with or without `-readrate`
// and its burst; the frames take that RAM instead (+430 MB RSS) and the sync
// about 0.09 of a core (PLAN-p400, E9). Such a command gets no `-readrate`:
// the graphics pace the media, and a piece no longer starts behind the
// schedule (its burst never made up the seek's preroll), so the overlay reads
// its frames while the history still holds their pictures – no jump.
//
// With VIZRS_FFMPEG_NO_DISCARDCORRUPT=1 a stream copy of an HLS playlist
// (ErsatzTV's MPEG-TS channel stream reads its own HLS that way) loses
// `discardcorrupt` from its `-fflags`: each piece of the channel is a new
// ffmpeg whose MPEG-TS continuity counters start at 0, so the demuxer flags
// the last video frame of the piece before as corrupt and the flag dropped
// it – a 40 ms freeze at every join (44 s), though the frame decodes intact.
//
// With VIZRS_FFMPEG_CUDA=auto a command encoding with NVENC and CUDA asks
// `nvidia-smi` for the free memory of the card first (at most 1 s): with at
// least VIZRS_FFMPEG_CUDA_MIN_FREE MiB it runs as `overlay`, below as `encode`
// (half the GPU memory, more CPU) – a card shared with a desktop that filled
// it no longer ends a piece with `CUDA_ERROR_OUT_OF_MEMORY` and the
// fallback slate. Without `nvidia-smi`, or when it fails, `overlay`.
//
// With VIZRS_FFMPEG_INGEST=1 a transcode to HLS of a channel whose live
// source is on (sidecar `/ingest/state`) plays that source in its place
// (src/takeover.rs), with the graphics overlaid as on the schedule.
// The same runs widen the graphics zones of `area: auto` in the middle of a
// piece (PLAN-strefy S3b): the overlay follows `/ffmpeg/zones` of its session
// and, when they widen, stops the run (SIGINT) and starts the next from where
// the piece is, with a new frame stream of the session in the new zones.
//
// A static binary, so it runs in the ErsatzTV image of any architecture
// without extra packages. A canvas over https goes to plain ffmpeg.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use regex::Regex;

use crate::area::Area;
use crate::encoder::{self, Encoder};
use crate::http::percent_decode;
use crate::json::{self, Value};
use crate::net::{Url, connect, error, read_head, read_head_text, status_of};
use crate::sys;
use crate::takeover::{self, Job, Sink, SinkControl};
mod args;
mod canvas;
mod cpu;
mod cuda;
mod ram_sync;
mod relay;
mod run;
mod settings;
mod watermark;
mod zones;

pub use args::*;
pub use canvas::*;
pub use cpu::*;
pub use cuda::*;
pub use ram_sync::*;
pub use relay::*;
pub use run::*;
pub use settings::*;
pub use watermark::*;
pub use zones::*;

const DEFAULT_FFMPEG: &str = "/usr/local/bin/ffmpeg";
const CLAIM_WAIT_MS: u32 = 2000;
const SCRIPT_END: Duration = Duration::from_millis(500);
const RECONNECT: Duration = Duration::from_secs(1);
/// A probe without a result this long ago (failed, or still running) is run again.
/// How often the overlay asks for the zones of its session (as `takeover` for the live state).
const ZONES_POLL: Duration = Duration::from_millis(100);
const DEFAULT_CUDA_MIN_FREE: u64 = 400;
const GPU_QUERY_WAIT: Duration = Duration::from_secs(1);
const PROBE_RETRY: Duration = Duration::from_secs(60);
/// Read timeout of a probe of an http input, in microseconds.
const PROBE_TIMEOUT_US: &str = "20000000";
const CODEC: [&str; 3] = ["-vcodec", "-c:v", "-codec:v"];
const PRESET_ENCODERS: [&str; 2] = ["libx264", "libx265"];
const COPY: [(&str, &str); 4] = [("-c", "copy"), ("-c:v", "copy"), ("-vcodec", "copy"), ("-codec:v", "copy")];
// Options of the canvas input that do not apply to the raw frames.
const DROP_WITH_VALUE: [&str; 16] = [
    "-headers",
    "-protocol_whitelist",
    "-reconnect",
    "-reconnect_streamed",
    "-reconnect_delay_max",
    "-reconnect_on_network_error",
    "-rw_timeout",
    "-f",
    "-c:v",
    "-vcodec",
    "-codec:v",
    "-pix_fmt",
    "-video_size",
    "-s",
    "-framerate",
    "-r",
];
const FORWARDED: [i32; 4] = [sys::SIGTERM, sys::SIGINT, sys::SIGHUP, sys::SIGQUIT];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_canvas_names_the_item_for_its_watermarks() {
        let region = args(&["-i", "/m.mp4", "-headers", "x", "-i", "http://etv:8409/internal/graphics/1/77?region=wm&sig=x", "out"]);
        assert!(find_canvas(&region).is_none(), "no VizRS element");
        let (canvas, url) = find_region_canvas(&region).unwrap();
        assert_eq!((canvas.channel.as_str(), canvas.item.as_str(), canvas.index), ("1", "77", 4));
        assert!(url.ends_with("region=wm&sig=x"));
        assert!(
            find_region_canvas(&args(&["-i", "http://etv:8409/internal/graphics/1/77?sig=x", "out"])).is_none(),
            "a VizRS canvas is not one"
        );
        assert!(find_region_canvas(&args(&["-i", "/m.mp4", "out"])).is_none());
    }

    #[test]
    fn rate_of_the_live_graphics_session() {
        assert_eq!(run::rate_number("25"), 25.0);
        assert!((run::rate_number("30000/1001") - 29.97).abs() < 0.01);
        assert_eq!(run::rate_number(""), 25.0);
        assert_eq!(run::rate_number("0/0"), 25.0);
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn cuda_encode_only_moves_decoding_and_filters_to_the_cpu() {
        let real = args(&[
            "-threads",
            "1",
            "-init_hw_device",
            "cuda",
            "-hwaccel",
            "cuda",
            "-hwaccel_output_format",
            "cuda",
            "-i",
            "/m/a.mp4",
            "-f",
            "rawvideo",
            "-i",
            "pipe:6",
            "-filter_complex",
            "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload_cuda[vz_g0];[vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload_cuda[vz_g1];[0:0]hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640[v_o0]",
            "-map",
            "[v_o0]",
            "-vcodec",
            "h264_nvenc",
            "-b:v",
            "2000k",
            "-f",
            "hls",
            "o.m3u8",
        ]);
        let cpu = cuda_encode_only(real);
        assert_eq!(
            cpu,
            args(&[
                "-threads",
                "1",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p[vz_g0];[vz_s1]crop=1280:80:0:320,format=yuva420p[vz_g1];[0:0]fps=23620/1000,setsar=1,format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][vz_g0]overlay=x=112:y=208:format=auto[vz_o0];[vz_o0][vz_g1]overlay=x=0:y=640:format=auto[v_o0]",
                "-map",
                "[v_o0]",
                "-vcodec",
                "h264_nvenc",
                "-b:v",
                "2000k",
                "-f",
                "hls",
                "o.m3u8",
            ])
        );
        // scale with a size, a lone upload, deinterlacing
        let other = cuda_encode_only(args(&[
            "-hwaccel",
            "cuda",
            "-i",
            "a",
            "-vf",
            "[0:0]hwupload_cuda,yadif_cuda,scale_cuda=1920:1080:format=nv12:interp_algo=lanczos[v]",
            "-c:v",
            "hevc_nvenc",
            "o",
        ]));
        assert_eq!(other, args(&["-i", "a", "-vf", "[0:0]yadif,scale=1920:1080,format=nv12[v]", "-c:v", "hevc_nvenc", "o"]));
        assert_eq!(cpu_chain("[a]hwupload_cuda[b]").unwrap(), "[a]null[b]");
        // not NVENC, no CUDA, or an unknown CUDA filter: unchanged
        let x264 = args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "libx264", "o"]);
        assert_eq!(cuda_encode_only(x264.clone()), x264);
        let plain = args(&["-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(cuda_encode_only(plain.clone()), plain);
        let unknown = args(&["-hwaccel", "cuda", "-i", "a", "-vf", "tonemap_cuda=x", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(cuda_encode_only(unknown.clone()), unknown);
    }

    #[test]
    fn cuda_decode_on_cpu_keeps_filters_and_overlay_on_the_card() {
        let graph = "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload_cuda[vz_g0];\
                     [vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload_cuda[vz_g1];\
                     [0:0]hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];\
                     [v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640[v_o0]";
        let real = args(&[
            "-threads",
            "1",
            "-init_hw_device",
            "cuda",
            "-hwaccel",
            "cuda",
            "-hwaccel_output_format",
            "cuda",
            "-i",
            "/m/a.mp4",
            "-f",
            "rawvideo",
            "-i",
            "pipe:6",
            "-filter_complex",
            "[0:1]loudnorm=I=-23,apad[a]",
            "-filter_complex",
            graph,
            "-map",
            "[v_o0]",
            "-map",
            "[a]",
            "-vcodec",
            "h264_nvenc",
            "o.m3u8",
        ]);
        let placed = graph
            .replace("[0:0]hwupload,", &format!("[0:0]{UPLOAD_FORMATS},hwupload,"))
            .replace("overlay_cuda=x=0:y=640[v_o0]", &format!("overlay_cuda=x=0:y=640,hwdownload,{UPLOAD_FORMATS}[v_o0]"));
        assert_eq!(
            cuda_decode_on_cpu(real),
            args(&[
                "-threads",
                "1",
                "-init_hw_device",
                "cuda",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                "[0:1]loudnorm=I=-23,apad[a]",
                "-filter_complex",
                &placed,
                "-map",
                "[v_o0]",
                "-map",
                "[a]",
                "-vcodec",
                "h264_nvenc",
                "o.m3u8",
            ])
        );
        // CUDA filters straight on the decoded frames: an upload before the first, a download at the end
        let direct = args(&[
            "-hwaccel",
            "cuda",
            "-hwaccel_output_format",
            "cuda",
            "-i",
            "a",
            "-vf",
            "yadif_cuda,scale_cuda=1920:1080",
            "-c:v",
            "hevc_nvenc",
            "o",
        ]);
        assert_eq!(
            cuda_decode_on_cpu(direct),
            args(&[
                "-i",
                "a",
                "-vf",
                &format!("{UPLOAD_FORMATS},hwupload,yadif_cuda,scale_cuda=1920:1080,hwdownload,{UPLOAD_FORMATS}"),
                "-c:v",
                "hevc_nvenc",
                "o"
            ])
        );
        let mut card = Vec::new();
        // a download of decoded frames (already on the CPU) dropped, the later upload kept; not for the encoder: stays on the card
        assert_eq!(
            place_frames("[0:0]hwdownload,format=nv12,fps=25,hwupload_cuda[v]", &mut card, &[], false),
            format!("[0:0]format=nv12,fps=25,{UPLOAD_FORMATS},hwupload_cuda[v]")
        );
        assert_eq!(card, ["v"]);
        // the graphics (format before the upload) unchanged; a chain of labels on the card keeps them there
        assert_eq!(place_frames("[1:0]format=yuva420p,hwupload_cuda[g]", &mut card, &[], false), "[1:0]format=yuva420p,hwupload_cuda[g]");
        assert_eq!(place_frames("[v]fps=50[w]", &mut card, &[], false), "[v]fps=50[w]");
        assert_eq!(
            place_frames("[w][g]overlay_cuda=x=0:y=0[o]", &mut card, &["o"], false),
            format!("[w][g]overlay_cuda=x=0:y=0,hwdownload,{UPLOAD_FORMATS}[o]")
        );
        assert!(!card.contains(&"o".to_owned()));
        // not NVENC, or no CUDA decoding (ErsatzTV already uploads): unchanged
        let x264 = args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "libx264", "o"]);
        assert_eq!(cuda_decode_on_cpu(x264.clone()), x264);
        let uploaded =
            args(&["-init_hw_device", "cuda", "-i", "a", "-vf", "hwupload_cuda,scale_cuda=1920:1080", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(cuda_decode_on_cpu(uploaded.clone()), uploaded);
        assert_eq!((Cuda::parse(""), Cuda::parse(" Encode "), Cuda::parse("overlay")), (Cuda::Full, Cuda::Encode, Cuda::Overlay));
        assert_eq!(Cuda::parse("AUTO"), Cuda::Auto);
    }

    #[test]
    fn cuda_auto_falls_back_to_encode_on_a_full_card() {
        assert_eq!(parse_free_memory("439\n"), Some(439));
        assert_eq!(parse_free_memory(" 1200\n80\n"), Some(1200));
        assert_eq!(parse_free_memory("[N/A]\n"), None);
        assert_eq!(parse_free_memory(""), None);
        assert_eq!(auto_cuda(Some(299), 300), Cuda::Encode);
        assert_eq!(auto_cuda(Some(300), 300), Cuda::Overlay);
        assert_eq!(auto_cuda(None, 300), Cuda::Overlay);
        let nvenc = args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert!(nvenc_with_cuda(&nvenc));
        assert!(nvenc_with_cuda(&args(&["-i", "a", "-vf", "scale_cuda=8:4", "-vcodec", "h264_nvenc", "o"])));
        assert!(!nvenc_with_cuda(&args(&["-i", "a", "-c:v", "h264_nvenc", "o"])));
        assert!(!nvenc_with_cuda(&args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "libx264", "o"])));
    }

    #[test]
    fn relay_keeps_whole_frames_and_comes_back() {
        use std::net::TcpListener;
        struct Tap(Arc<Mutex<Vec<u8>>>);
        impl Write for Tap {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                let mut seen = self.0.lock().unwrap();
                // Enough seen: ffmpeg is gone.
                if seen.len() > 4000 && seen.contains(&2) {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                seen.extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let first = TcpStream::connect(address).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        // One and a half frames of 4 bytes, then the sidecar is gone.
        server.write_all(&[1; 6]).unwrap();
        drop(server);
        let reconnect = || {
            let (sender, receiver) = mpsc::channel();
            let socket = TcpStream::connect(address).unwrap();
            let (mut back, _) = listener.accept().unwrap();
            back.write_all(&[2; 4]).unwrap();
            drop(back);
            sender.send(FrameSocket { stream: socket, delta: false }).unwrap();
            receiver
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        relay(FrameSocket { stream: first, delta: false }, Tap(Arc::clone(&seen)), 4, None, Some(&reconnect));
        let seen = seen.lock().unwrap();
        assert_eq!(&seen[..8], &[1, 1, 1, 1, 1, 1, 0, 0], "the broken frame is filled");
        let at = seen.iter().position(|b| *b == 2).unwrap();
        assert_eq!(at % 4, 0, "the frames stay aligned");
    }

    /// Delta records become whole frames for ffmpeg; a record broken off is
    /// not shown, and the stream after a reconnection starts with a whole frame.
    #[test]
    fn relay_rebuilds_whole_frames_from_delta_records() {
        use std::net::TcpListener;
        // a 2x2 frame: Y 4, U 1, V 1, A 4 bytes
        let (one, two) = ([1u8; 10], [1u8, 1, 1, 1, 1, 1, 1, 1, 7, 7]);
        let mut records = Vec::new();
        crate::delta::write_record(&mut records, None, &one, 2, 2).unwrap();
        crate::delta::write_record(&mut records, Some(&one), &two, 2, 2).unwrap();
        records.extend_from_slice(&crate::delta::UNCHANGED);
        records.extend_from_slice(&crate::delta::MAGIC[..]); // broken off
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let first = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.write_all(&records).unwrap();
        drop(server);
        let mut seen = Vec::new();
        relay(FrameSocket { stream: first, delta: true }, &mut seen, 10, None, None);
        assert_eq!(seen, [&one[..], &two, &two].concat());
    }

    #[test]
    fn zones_of_a_session_from_the_sidecar() {
        let zones = parse_zones(
            r#"{"rev":2,"zones":"448x96+1440+32,1920x216+0+864","width":1920,"height":312,
                "areas":[{"x":1440,"y":32,"w":448,"h":96},{"x":0,"y":864,"w":1920,"h":216}],"resume":"channel=1"}"#,
        )
        .unwrap();
        assert_eq!(zones.rev, 2);
        assert_eq!((zones.width, zones.height), (1920, 312));
        // stacked: the second part starts below the first
        assert_eq!(zones.areas[1], (Area { x: 0, y: 864, w: 1920, h: 216 }, 96));
        // the whole channel: no zones, one part at the origin
        let whole = parse_zones(r#"{"rev":3,"zones":null,"width":1920,"height":1080,"areas":[{"x":0,"y":0,"w":1920,"h":1080}]}"#).unwrap();
        assert_eq!((whole.geometry.as_str(), whole.areas.len()), ("", 1));
        assert!(parse_zones(r#"{"rev":1,"width":1,"height":1,"areas":[]}"#).is_err());
    }

    #[test]
    fn frames_declare_their_colour_matrix() {
        let input = args(&["-i", "media", "-i", "http://localhost/internal/graphics/4/1", "-f", "hls", "o"]);
        let canvas = Canvas { start: 2, index: 2, channel: "4".into(), item: "7".into() };
        let mut frames = Frames {
            width: 8,
            height: 4,
            rate: "25".into(),
            matrix: "bt709".into(),
            areas: vec![],
            resume: String::new(),
            session: "1".into(),
            ahead: None,
            zones_rev: None,
        };
        let raw = ["-i", "media", "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "8x4", "-framerate", "25"];
        let tail = ["-i", "pipe:5", "-f", "hls", "o"];
        let declared = [&raw[..], &["-colorspace", "bt709", "-color_range", "tv"], &tail].concat();
        assert_eq!(overlay_args(&input, &canvas, 5, &frames, &Watermarks::default()), args(&declared));
        // An older sidecar names no matrix: nothing is declared.
        frames.matrix.clear();
        assert_eq!(overlay_args(&input, &canvas, 5, &frames, &Watermarks::default()), args(&[&raw[..], &tail].concat()));
    }

    /// ErsatzTV Next's command of a piece with no graphics element, only a watermark (S1, channel 1, 2026-11-26).
    fn piece_with_watermark() -> Vec<OsString> {
        args(&[
            "-threads",
            "0",
            "-nostdin",
            "-readrate",
            "1.0",
            "-i",
            "http://localhost:8409/internal/media/jellyfin/79?exp=1&sig=x",
            "-loop",
            "1",
            "-framerate",
            "25",
            "-t",
            "773340ms",
            "-f",
            "image2",
            "-pixel_format",
            "yuva420p",
            "-video_size",
            "76x76",
            "-i",
            "/config/cache/next/still-images/b_76x76_yuva420p.raw",
            "-filter_complex",
            "[0:1]loudnorm=I=-23:TP=-1.5:LRA=11,aresample=48000,aresample=async=1,apad[a]",
            "-filter_complex",
            "[0:0]fps=25,scale=1600:900:flags=bicubic,setsar=1[v_m0];[v_m0][1:0]overlay=x=1444:y=45:format=0[v_o0]",
            "-map",
            "[v_o0]",
            "-map",
            "[a]",
            "-vcodec",
            "libx264",
            "-t",
            "773340ms",
            "-f",
            "hls",
            "/transcode/1/ffmpeg.m3u8",
        ])
    }

    fn piece_frames(areas: Vec<(Area, u32)>) -> Frames {
        Frames {
            width: 1600,
            height: 900,
            rate: "25".into(),
            matrix: String::new(),
            areas,
            resume: String::new(),
            session: "1".into(),
            ahead: None,
            zones_rev: None,
        }
    }

    #[test]
    fn graphics_go_over_a_piece_without_element_under_its_watermark() {
        let out = piece_overlay_args(&piece_with_watermark(), 7, &piece_frames(vec![])).unwrap();
        let text: Vec<&str> = out.iter().map(|a| a.to_str().unwrap()).collect();
        // The frames are the third input, right after the watermark.
        let at = text.iter().position(|a| *a == "pipe:7").unwrap();
        assert_eq!(text[at - 1], "-i");
        assert_eq!(text[at - 3..at - 1], ["-framerate", "25"]);
        assert_eq!(text[at - 10], "/config/cache/next/still-images/b_76x76_yuva420p.raw");
        // The graphics over the picture, the watermark over them; the sound and the maps as they were.
        assert!(text.contains(&"[0:1]loudnorm=I=-23:TP=-1.5:LRA=11,aresample=48000,aresample=async=1,apad[a]"));
        assert!(text.contains(
            &"[0:0]fps=25,scale=1600:900:flags=bicubic,setsar=1[v_m0];[vz_pv][1:0]overlay=x=1444:y=45:format=0[v_o0];\
              [2:0]format=yuva420p[vz_gfx];[v_m0][vz_gfx]overlay=x=0:y=0:format=auto:shortest=1[vz_pv]"
        ));
        assert_eq!(text.iter().filter(|a| **a == "[v_o0]").count(), 1, "the map of the picture stays");
    }

    #[test]
    fn graphics_go_over_a_piece_without_watermark_and_in_their_zones() {
        let piece = args(&[
            "-i",
            "/m/a.mp4",
            "-filter_complex",
            "[0:0]fps=25,scale=1600:900,setsar=1[v_m0]",
            "-map",
            "[v_m0]",
            "-map",
            "0:a",
            "-t",
            "10s",
            "-f",
            "hls",
            "o.m3u8",
        ]);
        let out = piece_overlay_args(&piece, 4, &piece_frames(vec![])).unwrap();
        let graph = "[0:0]fps=25,scale=1600:900,setsar=1[v_m0];[1:0]format=yuva420p[vz_gfx];[v_m0][vz_gfx]overlay=x=0:y=0:format=auto:shortest=1[vz_pv]";
        assert!(out.iter().any(|a| a == graph));
        assert!(out.windows(2).any(|w| w[0] == "-map" && w[1] == "[vz_pv]"));
        // A zone of the channel: overlaid at its place.
        let zone = vec![(Area { x: 1360, y: 16, w: 144, h: 144 }, 0)];
        let out = piece_overlay_args(&piece, 4, &piece_frames(zone)).unwrap();
        assert!(out.iter().any(|a| a.to_str().unwrap().ends_with("[v_m0][vz_gfx]overlay=format=auto:shortest=1:x=1360:y=16[vz_pv]")));
    }

    #[test]
    fn no_graphics_over_a_piece_they_cannot_go_over() {
        // Hardware frames in the graph.
        let mut cuda = piece_with_watermark();
        let graph = cuda.iter().position(|a| a.to_str().unwrap().starts_with("[0:0]fps")).unwrap();
        cuda[graph] = "[0:0]hwupload_cuda,scale_cuda=1600:900[v_m0];[v_m0][1:0]overlay_cuda=x=1444:y=45[v_o0]".into();
        assert!(piece_overlay_args(&cuda, 7, &piece_frames(vec![])).is_none());
        // No picture of a filter graph mapped out.
        let copied = args(&["-i", "/m/a.mp4", "-map", "0:v", "-t", "10s", "-f", "hls", "o.m3u8"]);
        assert!(piece_overlay_args(&copied, 7, &piece_frames(vec![])).is_none());
    }

    #[test]
    fn canvas_input() {
        let list = args(&["-i", "/m/a.mp4", "-headers", "h", "-i", "http://x:1/internal/graphics/2%2E1/28750?exp=1", "-f", "null", "-"]);
        assert_eq!(find_canvas(&list), Some(Canvas { start: 2, index: 4, channel: "2.1".into(), item: "28750".into() }));
        assert_eq!(find_canvas(&args(&["-i", "http://x/internal/graphics/1/7?region=a"])), None);
        assert!(find_canvas(&args(&["-i", "http://x/internal/graphics/1/7?region="])).is_some());
        assert_eq!(find_canvas(&args(&["-i", "http://x/internal/graphics/1/a"])), None);
        assert_eq!(find_canvas(&args(&["-i", "file:///internal/graphics/1/7"])), None);
        assert_eq!(find_canvas(&args(&["-version"])), None);
    }

    #[test]
    fn headers() {
        let list = args(&["-headers", "a: 1\r\nx-etv-frame-rate:25/1\nbad\n: v\r\n", "-headers", "a:2"]);
        assert_eq!(parse_headers(&list), vec![("a".into(), "2".into()), ("x-etv-frame-rate".into(), "25/1".into())]);
    }

    #[test]
    fn rate_only_on_hls_transcodes() {
        let hls = args(&["-i", "a", "-filter_complex", "[0:0]x[v];[v][1:0]overlay_cuda=x=0[o];[o][2]overlay", "-f", "hls", "o.m3u8"]);
        assert_eq!(
            with_rate(hls.clone(), "50"),
            args(&[
                "-i",
                "a",
                "-filter_complex",
                "[0:0]x[v];[v]fps=50[v_fps];[v_fps][1:0]overlay_cuda=x=0[o];[o][2]overlay",
                "-r",
                "50",
                "-f",
                "hls",
                "o.m3u8"
            ])
        );
        assert_eq!(with_rate(hls.clone(), ""), hls);
        let copy = args(&["-i", "a", "-c:v", "copy", "-f", "hls", "o"]);
        assert_eq!(with_rate(copy.clone(), "50"), copy);
        // ErsatzTV's conversion of the media's rate converts to the channel's.
        let normalized = args(&["-i", "a", "-filter_complex", "[0:0]fps=25,scale=8:4[v];[v][1:0]overlay[o]", "-f", "hls", "o"]);
        assert_eq!(with_rate(normalized, "50")[3], "[0:0]fps=50,scale=8:4[v];[v]fps=50[v_fps];[v_fps][1:0]overlay[o]");
    }

    #[test]
    fn media_rate_and_input() {
        assert_eq!(parse_rate("30000/1001").map(|r| (r * 1000.0).round()), Some(29970.0));
        assert_eq!(parse_rate("50"), Some(50.0));
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("x"), None);
        assert_eq!(probed_rate("r_frame_rate=60/1\navg_frame_rate=60/1\n"), Some(60.0));
        assert_eq!(probed_rate("r_frame_rate=30/1\navg_frame_rate=0/0\n"), Some(30.0));
        assert_eq!(probed_rate(""), None);
        let hls = args(&[
            "-ss",
            "1s",
            "-i",
            "http://m/1?sig=a",
            "-i",
            "pipe:4",
            "-filter_complex",
            "[0:1]loudnorm[a]",
            "-filter_complex",
            "[0:0]fps=25,scale=8:4[v]",
            "-f",
            "hls",
            "o",
        ]);
        assert_eq!(media_input(&hls).as_deref(), Some("http://m/1?sig=a"));
        let second = args(&["-i", "a", "-i", "b", "-filter_complex", "[1:v]fps=25[v]", "-f", "hls", "o"]);
        assert_eq!(media_input(&second).as_deref(), Some("b"));
        assert_eq!(media_input(&args(&["-i", "a", "-filter_complex", "[0:0]fps=25[v]", "-f", "mpegts", "o"])), None);
        assert_eq!(media_input(&args(&["-i", "a", "-filter_complex", "[0:0]scale=8:4[v]", "-f", "hls", "o"])), None);
    }

    #[test]
    fn blend_only_the_media_rate_of_hls_transcodes() {
        let filter = "[0:0]fps=25,scale=1600:900:flags=bicubic,setsar=1[v_m0];[1:0]format=yuva420p[v_s0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][v_s0]overlay=x=0:y=0[v_o0]";
        let hls = args(&["-i", "a", "-filter_complex", "[0:1]loudnorm[a]", "-filter_complex", filter, "-f", "hls", "o.m3u8"]);
        let blended = with_blend(hls.clone(), true, None);
        assert_eq!(blended[3], "[0:1]loudnorm[a]");
        assert_eq!(
            blended[5],
            "[0:0]framerate=fps=25,scale=1600:900:flags=bicubic,setsar=1[v_m0];[1:0]format=yuva420p[v_s0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][v_s0]overlay=x=0:y=0[v_o0]"
        );
        assert_eq!(with_blend(hls.clone(), false, None), hls);
        // Faster media blends; media no faster than the output repeats its frames.
        assert_eq!(with_blend(hls.clone(), true, Some(30.0)), blended);
        assert_eq!(with_blend(hls.clone(), true, Some(25.0)), hls);
        let fifty = args(&["-i", "a", "-filter_complex", "[0:0]fps=50,scale=8:4[v]", "-f", "hls", "o"]);
        assert_eq!(with_blend(fifty.clone(), true, Some(25.0)), fifty);
        assert_eq!(with_blend(fifty.clone(), true, Some(60.0))[3], "[0:0]framerate=fps=50,scale=8:4[v]");
        // Not a transcode to HLS, and an fps with options.
        let other = args(&["-i", "a", "-filter_complex", "[0:0]fps=25,scale=8:4[v]", "-f", "mpegts", "o"]);
        assert_eq!(with_blend(other.clone(), true, None), other);
        let options = args(&["-i", "a", "-filter_complex", "[0:v]fps=25:round=up[v]", "-f", "hls", "o"]);
        assert_eq!(with_blend(options.clone(), true, None), options);
        let label = args(&["-i", "a", "-filter_complex", "[0:v]fps=30000/1001[v]", "-f", "hls", "o"]);
        assert_eq!(with_blend(label.clone(), true, None)[3], "[0:v]framerate=fps=30000/1001[v]");
        assert_eq!(with_blend(label.clone(), true, Some(29.97)), label);
    }

    #[test]
    fn bframes_only_for_hardware_encoders_without_them() {
        let nvenc = args(&["-i", "a", "-vcodec", "h264_nvenc", "-b:v", "6000k", "o"]);
        assert_eq!(with_bframes(nvenc.clone(), "0"), args(&["-i", "a", "-vcodec", "h264_nvenc", "-bf:v", "0", "-b:v", "6000k", "o"]));
        assert_eq!(with_bframes(nvenc.clone(), ""), nvenc);
        assert_eq!(with_bframes(nvenc.clone(), "x"), nvenc);
        let own = args(&["-i", "a", "-c:v", "hevc_nvenc", "-bf", "2", "o"]);
        assert_eq!(with_bframes(own.clone(), "0"), own);
        let x264 = args(&["-i", "a", "-vcodec", "libx264", "o"]);
        assert_eq!(with_bframes(x264.clone(), "0"), x264);
        for hardware in ["h264_qsv", "h264_vaapi", "hevc_vaapi"] {
            assert_eq!(with_bframes(args(&["-c:v", hardware, "o"]), "1"), args(&["-c:v", hardware, "-bf:v", "1", "o"]));
        }
    }

    #[test]
    fn presets_of_each_encoder() {
        let run = |encoder: &str| with_presets(args(&["-i", "a", "-c:v", encoder, "o"]), "veryfast", "p5", "slow");
        assert_eq!(run("libx264"), args(&["-i", "a", "-c:v", "libx264", "-preset:v", "veryfast", "o"]));
        assert_eq!(run("hevc_nvenc"), args(&["-i", "a", "-c:v", "hevc_nvenc", "-preset:v", "p5", "o"]));
        assert_eq!(run("h264_qsv"), args(&["-i", "a", "-c:v", "h264_qsv", "-preset:v", "slow", "o"]));
        assert_eq!(run("h264_vaapi"), args(&["-i", "a", "-c:v", "h264_vaapi", "o"]));
        let nvenc = args(&["-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(with_presets(nvenc.clone(), "veryfast", "", "slow"), nvenc);
        let own = args(&["-i", "a", "-c:v", "h264_nvenc", "-preset", "p7", "o"]);
        assert_eq!(with_presets(own.clone(), "", "p1", ""), own);
    }

    #[test]
    fn shortest_goes_only_with_an_output_duration() {
        let full = args(&["-t", "44000ms", "-i", "m.mp4", "-i", "pipe:6", "-map", "[v]", "-t", "44000ms", "-shortest", "-f", "hls", "o"]);
        let without = args(&["-t", "44000ms", "-i", "m.mp4", "-i", "pipe:6", "-map", "[v]", "-t", "44000ms", "-f", "hls", "o"]);
        assert_eq!(without_shortest(full.clone(), true), without);
        assert_eq!(without_shortest(full.clone(), false), full);
        // No -t of the output (only of an input): -shortest ends it, it stays.
        let open = args(&["-t", "44000ms", "-i", "m.mp4", "-map", "[v]", "-shortest", "-f", "hls", "o"]);
        assert_eq!(without_shortest(open.clone(), true), open);
    }

    #[test]
    fn media_paced_only_in_canvas_transcodes_overlaid_on_the_card() {
        let canvas = |head: &[&str], filter: &str| {
            let mut all: Vec<&str> = head.to_vec();
            all.extend(["-i", "http://m", "-f", "rawvideo", "-i", "pipe:6", "-filter_complex", filter, "-c:v", "h264_nvenc", "o"]);
            args(&all)
        };
        let on_card = "[0:0]hwupload[m];[1:0]hwupload[g];[m][g]overlay_cuda[v]";
        assert_eq!(paced_media(canvas(&["-ss", "5ms"], on_card), None), canvas(&["-ss", "5ms", "-readrate", "1"], on_card));
        // Graphics ready ahead: the media bursts as far (at least ffmpeg's 0.5 s, at most 30 s).
        let burst = |seconds: &str| canvas(&["-readrate", "1", "-readrate_initial_burst", seconds], on_card);
        assert_eq!(paced_media(canvas(&[], on_card), Some(7.46)), burst("7.5"));
        assert_eq!(paced_media(canvas(&[], on_card), Some(-2.0)), burst("0.5"));
        assert_eq!(paced_media(canvas(&[], on_card), Some(99.0)), burst("30.0"));
        // Its own rate, the overlay on the CPU, or no raw graphics: unchanged.
        let own = canvas(&["-readrate", "1.0"], on_card);
        assert_eq!(paced_media(own.clone(), Some(4.0)), own);
        let cpu = canvas(&[], "[0:0][1:0]overlay[v]");
        assert_eq!(paced_media(cpu.clone(), Some(4.0)), cpu);
        let plain = args(&["-i", "http://m", "-filter_complex", on_card, "-c:v", "h264_nvenc", "o"]);
        assert_eq!(paced_media(plain.clone(), Some(4.0)), plain);
    }

    #[test]
    fn media_starts_on_the_frame_grid_of_the_graphics() {
        let cmd = |graph: &str, rate: &str| {
            let mut list = vec!["-ss", "132000ms", "-i", "http://m/a", "-f", "rawvideo"];
            if !rate.is_empty() {
                list.extend(["-framerate", rate]);
            }
            list.extend(["-i", "pipe:6", "-filter_complex", "[0:1]loudnorm,apad[a]", "-filter_complex", graph, "-map", "[v_o0]", "o.m3u8"]);
            args(&list)
        };
        // S1 (CPU overlay, two areas): the media in the channel's rate, filled to frame 0 before the first overlay.
        let cpu = "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=320:112:0:0,format=yuva420p[vz_g0];[vz_s1]crop=144:144:0:112,format=yuva420p[vz_g1];\
                   [0:0]fps=25,scale=1600:900[v_m0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][vz_g0]overlay=x=0:y=80[vz_o0];[vz_o0][vz_g1]overlay=x=1360:y=16[v_o0]";
        let fixed = format!("[v_m0_fps]fps=25:start_time=0[vz_grid];{}", cpu.replacen("[v_m0_fps][vz_g0]", "[vz_grid][vz_g0]", 1));
        assert_eq!(media_on_graphics_grid(cmd(cpu, "25")), cmd(&fixed, "25"));
        // Once only.
        assert_eq!(media_on_graphics_grid(cmd(&fixed, "25")), cmd(&fixed, "25"));
        // Synced in RAM (local stack, CUDA): filled before the sync with the graphics, not before `overlay_cuda`.
        let ram = format!(
            "[1:0]split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];[0:0]fps=50[0:0_fps];[0:0_fps][vz_wt]{RAM_SYNC},hwupload,fps=50[v_m0];\
             [vz_w1]hwupload[v_s0];[v_m0][v_s0]overlay_cuda[v_o0]"
        );
        let ram_fixed = format!("[0:0_fps]fps=50:start_time=0[vz_grid];{}", ram.replacen("[0:0_fps][vz_wt]", "[vz_grid][vz_wt]", 1));
        assert_eq!(media_on_graphics_grid(cmd(&ram, "50")), cmd(&ram_fixed, "50"));
        // No rate of the graphics, no graphics pipe, no overlay: unchanged.
        assert_eq!(media_on_graphics_grid(cmd(cpu, "")), cmd(cpu, ""));
        let plain = args(&["-i", "a.mp4", "-filter_complex", "[0:0]fps=25[v]", "-map", "[v]", "o.ts"]);
        assert_eq!(media_on_graphics_grid(plain.clone()), plain);
        let stacked = "[0:0]scale=10:10[a];[1:0]scale=10:10[b];[a][b]hstack[v_o0]";
        assert_eq!(media_on_graphics_grid(cmd(stacked, "25")), cmd(stacked, "25"));
    }

    #[test]
    fn cpu_overlay_synced_in_ram_reads_the_media_without_its_readrate() {
        let cmd = |pacing: &[&str], graph: &str| {
            let mut list: Vec<&str> = vec!["-threads", "0", "-fflags", "+genpts"];
            list.extend(pacing);
            list.extend(["-i", "http://m/a", "-t", "3177561ms", "-f", "rawvideo", "-framerate", "25", "-i", "pipe:6"]);
            list.extend([
                "-filter_complex",
                "[0:1]loudnorm,apad[a]",
                "-filter_complex",
                graph,
                "-map",
                "[v_o0]",
                "-vcodec",
                "libx264",
                "o.m3u8",
            ]);
            args(&list)
        };
        // S1: ErsatzTV's -readrate 1.0 on the media, the graphics overlaid on the CPU (two areas).
        let s1 = "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=176:176:0:0,format=yuva420p[vz_g0];[vz_s1]crop=1600:84:0:176,format=yuva420p[vz_g1];\
                  [0:0]fps=25,scale=1600:900[v_m0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][vz_g0]overlay=format=0:x=1360:y=16[vz_o0];[vz_o0][vz_g1]overlay=format=0:x=0:y=816[v_o0]";
        let paced = ["-readrate", "1.0", "-readrate_initial_burst", "2", "-readrate_catchup", "2"];
        assert_eq!(cpu_synced_in_ram(cmd(&paced, s1), true), cmd(&[], s1));
        assert_eq!(cpu_synced_in_ram(cmd(&paced, s1), false), cmd(&paced, s1), "off by default");
        // On a card, or no overlay of the graphics: unchanged.
        let cuda = "[0:0]hwupload,scale_cuda=8:4[v_m0];[1:0]format=yuva420p,hwupload[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]";
        assert_eq!(cpu_synced_in_ram(cmd(&paced, cuda), true), cmd(&paced, cuda));
        let vaapi = "[0:0]scale_vaapi=8:4[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=format=0,format=nv12|vaapi,hwupload[x];[x][v_s0]overlay_vaapi[v_o0]";
        assert_eq!(cpu_synced_in_ram(cmd(&paced, vaapi), true), cmd(&paced, vaapi));
        let stacked = "[0:0]scale=8:4[a];[1:0]scale=8:4[b];[a][b]hstack[v_o0]";
        assert_eq!(cpu_synced_in_ram(cmd(&paced, stacked), true), cmd(&paced, stacked));
        // A -readrate of another input stays.
        let other = args(&[
            "-readrate",
            "1",
            "-i",
            "http://m/a",
            "-readrate",
            "1",
            "-f",
            "rawvideo",
            "-i",
            "pipe:6",
            "-filter_complex",
            s1,
            "o.m3u8",
        ]);
        let kept = args(&["-i", "http://m/a", "-readrate", "1", "-f", "rawvideo", "-i", "pipe:6", "-filter_complex", s1, "o.m3u8"]);
        assert_eq!(cpu_synced_in_ram(other, true), kept);
    }

    #[test]
    fn transparent_frames_of_a_lost_sidecar_come_at_the_channel_rate() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let first = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        drop(listener.accept().unwrap()); // the sidecar lost at once
        let seen = Arc::new(Mutex::new(0usize));
        struct Count(Arc<Mutex<usize>>);
        impl Write for Count {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                let mut n = self.0.lock().unwrap();
                *n += data.len();
                if *n >= 4 * 30 { Err(io::ErrorKind::BrokenPipe.into()) } else { Ok(data.len()) }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let never = || mpsc::channel::<FrameSocket>().1;
        let start = std::time::Instant::now();
        relay(FrameSocket { stream: first, delta: false }, Count(Arc::clone(&seen)), 4, Some(Duration::from_millis(10)), Some(&never));
        // 30 frames of 4 bytes, one per 10 ms: not at once.
        assert!(start.elapsed() >= Duration::from_millis(250), "{:?}", start.elapsed());
    }

    #[test]
    fn media_waits_for_the_graphics_in_ram_before_its_upload() {
        let canvas = |filter: &str| {
            args(&[
                "-init_hw_device",
                "cuda",
                "-readrate",
                "1",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                "[0:1]loudnorm=I=-23,apad[a]",
                "-filter_complex",
                filter,
                "-map",
                "[v_o0]",
                "-map",
                "[a]",
                "-vcodec",
                "h264_nvenc",
                "o.m3u8",
            ])
        };
        // ErsatzTV's graph after `cuda_decode_on_cpu` and two graphics areas (`placed_areas`).
        let graph = format!(
            "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload[vz_g0];\
             [vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload[vz_g1];\
             [0:0]{UPLOAD_FORMATS},hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];\
             [v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640,hwdownload,{UPLOAD_FORMATS}[v_o0]"
        );
        let synced = format!(
            "[1:0]split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];\
             [vz_w1]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload[vz_g0];\
             [vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload[vz_g1];\
             [0:0][vz_wt]{RAM_SYNC},{UPLOAD_FORMATS},hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];\
             [v_m0]fps=25[v_m0_fps];\
             [v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640,hwdownload,{UPLOAD_FORMATS}[v_o0]"
        );
        assert_eq!(media_waits_in_ram(canvas(&graph), true), canvas(&synced));
        // Twice: the graph is already synced.
        assert_eq!(media_waits_in_ram(canvas(&synced), true), canvas(&synced));
        // One area, the graphics straight into the overlay.
        let direct = "[0:0]format=yuv420p,hwupload[m];[m][1:v]overlay_cuda=x=0:y=780,hwdownload,format=yuv420p[v_o0]";
        assert_eq!(
            media_waits_in_ram(canvas(direct), true),
            canvas(&format!(
                "[1:v]split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];\
                 [0:0][vz_wt]{RAM_SYNC},format=yuv420p,hwupload[m];[m][vz_w1]overlay_cuda=x=0:y=780,hwdownload,format=yuv420p[v_o0]"
            ))
        );
        // Off, the overlay on the CPU, no graphics pipe, two uploading media chains, graphics used twice: unchanged.
        assert_eq!(media_waits_in_ram(canvas(&graph), false), canvas(&graph));
        let cpu = canvas("[0:0][1:0]overlay=x=0:y=0[v_o0]");
        assert_eq!(media_waits_in_ram(cpu.clone(), true), cpu);
        let no_pipe = args(&["-i", "a", "-i", "b", "-filter_complex", &graph, "-c:v", "h264_nvenc", "o"]);
        assert_eq!(media_waits_in_ram(no_pipe.clone(), true), no_pipe);
        let two = canvas("[0:0]hwupload[m];[2:0]hwupload[n];[m][1:0]overlay_cuda[o];[o][n]overlay_cuda[v_o0]");
        assert_eq!(media_waits_in_ram(two.clone(), true), two);
        let twice = canvas("[0:0]hwupload[m];[1:0]hwupload[g];[m][g]overlay_cuda[o];[o][1:0]overlay_cuda[v_o0]");
        assert_eq!(media_waits_in_ram(twice.clone(), true), twice);
    }

    #[test]
    fn hls_stream_copy_keeps_frames_flagged_corrupt() {
        // ErsatzTV's MPEG-TS channel stream: its own HLS copied.
        let ts = |flags: &str| {
            args(&[
                "-nostdin",
                "-threads",
                "1",
                "-fflags",
                flags,
                "-readrate",
                "1.0",
                "-i",
                "http://localhost:8409/iptv/channel/1.m3u8?mode=segmenter&subtitles=false",
                "-map",
                "0",
                "-c",
                "copy",
                "-f",
                "mpegts",
                "pipe:1",
            ])
        };
        assert_eq!(keeping_corrupt(ts("+genpts+discardcorrupt+igndts"), true), ts("+genpts+igndts"));
        let only = keeping_corrupt(ts("+discardcorrupt"), true);
        assert!(!only.contains(&OsString::from("-fflags")), "nothing left: -fflags goes");
        // Off, no discardcorrupt, flags cleared with '-', a transcode, a file input: unchanged.
        assert_eq!(keeping_corrupt(ts("+genpts+discardcorrupt+igndts"), false), ts("+genpts+discardcorrupt+igndts"));
        assert_eq!(keeping_corrupt(ts("+genpts+igndts"), true), ts("+genpts+igndts"));
        assert_eq!(keeping_corrupt(ts("+genpts-discardcorrupt"), true), ts("+genpts-discardcorrupt"));
        let encode = args(&["-fflags", "+discardcorrupt", "-i", "a.m3u8", "-c:v", "libx264", "o.ts"]);
        assert_eq!(keeping_corrupt(encode.clone(), true), encode);
        let file = args(&["-fflags", "+discardcorrupt", "-i", "a.mp4", "-c", "copy", "o.ts"]);
        assert_eq!(keeping_corrupt(file.clone(), true), file);
        // The flags of another input stay.
        let two =
            args(&["-fflags", "+discardcorrupt", "-i", "a.mp4", "-fflags", "+genpts+discardcorrupt", "-i", "b.m3u8", "-c", "copy", "o"]);
        assert_eq!(
            keeping_corrupt(two, true),
            args(&["-fflags", "+discardcorrupt", "-i", "a.mp4", "-fflags", "+genpts", "-i", "b.m3u8", "-c", "copy", "o"])
        );
    }

    #[test]
    fn media_synced_in_ram_is_not_paced() {
        let canvas = |filter: &str| {
            args(&[
                "-init_hw_device",
                "cuda",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                filter,
                "-c:v",
                "h264_nvenc",
                "o",
            ])
        };
        let graph = "[0:0]format=yuv420p,hwupload[m];[1:0]format=yuva420p,hwupload[g];[m][g]overlay_cuda[v]";
        // Synced: no -readrate, the graphics pace the media.
        let synced = overlay_paced(canvas(graph), Some(12.0), true);
        assert!(!synced.contains(&OsString::from("-readrate")));
        assert_eq!(synced, media_waits_in_ram(canvas(graph), true));
        // Off, or a graph the sync does not take: paced as before.
        assert_eq!(overlay_paced(canvas(graph), Some(12.0), false), paced_media(canvas(graph), Some(12.0)));
        let two = "[0:0]hwupload[m];[2:0]hwupload[n];[m][1:0]overlay_cuda[o];[o][n]overlay_cuda[v]";
        assert_eq!(overlay_paced(canvas(two), Some(12.0), true), paced_media(canvas(two), Some(12.0)));
    }

    #[test]
    fn graphics_upload_through_the_one_cuda_device() {
        let graph = "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=8:2:0:0,format=yuva420p,hwupload_cuda[vz_g0];\
                     [vz_s1]crop=8:2:0:2,hwupload_cuda[vz_g1];[0:0]hwupload,scale_cuda=8:4[v];[v][vz_g0]overlay_cuda[o0];\
                     [o0][vz_g1]overlay_cuda[o]";
        let command = |device: &[&str], graph: &str| {
            let mut all: Vec<&str> = device.to_vec();
            all.extend(["-i", "a", "-i", "pipe:6", "-filter_complex", graph, "-c:v", "h264_nvenc", "o"]);
            args(&all)
        };
        let shared = graph.replace("hwupload_cuda", "hwupload");
        assert_eq!(shared_upload(command(&["-init_hw_device", "cuda"], graph)), command(&["-init_hw_device", "cuda"], &shared));
        assert_eq!(shared_upload(command(&["-init_hw_device", "cuda=gpu:0"], graph)), command(&["-init_hw_device", "cuda=gpu:0"], &shared));
        // No device of its own, two devices, or an upload with options: unchanged.
        assert_eq!(shared_upload(command(&[], graph)), command(&[], graph));
        let two = ["-init_hw_device", "cuda", "-init_hw_device", "vaapi"];
        assert_eq!(shared_upload(command(&two, graph)), command(&two, graph));
        let device = "[1:0]hwupload_cuda=device=1[g]";
        assert_eq!(shared_upload(command(&["-init_hw_device", "cuda"], device)), command(&["-init_hw_device", "cuda"], device));
        assert_eq!(
            shared_upload(command(&["-init_hw_device", "cuda"], "[1:0]hwupload_cuda,hwupload_cuda[g]")),
            command(&["-init_hw_device", "cuda"], "[1:0]hwupload,hwupload[g]")
        );
    }

    #[test]
    fn preset_only_for_software_encoders_without_one() {
        let x264 = args(&["-i", "a", "-vcodec", "libx264", "-b:v", "1500k", "-f", "hls", "o"]);
        assert_eq!(
            with_preset(x264.clone(), "superfast"),
            args(&["-i", "a", "-vcodec", "libx264", "-preset:v", "superfast", "-b:v", "1500k", "-f", "hls", "o"])
        );
        assert_eq!(with_preset(x264.clone(), ""), x264);
        let x265 = args(&["-i", "a", "-c:v", "libx265", "o"]);
        assert_eq!(with_preset(x265, "veryfast"), args(&["-i", "a", "-c:v", "libx265", "-preset:v", "veryfast", "o"]));
        let own = args(&["-i", "a", "-vcodec", "libx264", "-preset:v", "medium", "o"]);
        assert_eq!(with_preset(own.clone(), "superfast"), own);
        let nvenc = args(&["-i", "a", "-vcodec", "h264_nvenc", "o"]);
        assert_eq!(with_preset(nvenc.clone(), "superfast"), nvenc);
        let canvas = args(&["-f", "rawvideo", "-i", "pipe:0", "-c:v", "ffv1", "-f", "nut", "pipe:1"]);
        assert_eq!(with_preset(canvas.clone(), "superfast"), canvas);
    }

    #[test]
    fn x264_params_only_for_libx264() {
        let x264 = args(&["-i", "a", "-vcodec", "libx264", "-b:v", "1500k", "-f", "hls", "o"]);
        assert_eq!(
            with_x264_params(x264.clone(), "bframes=0:subme=0"),
            args(&["-i", "a", "-vcodec", "libx264", "-x264-params:v", "bframes=0:subme=0", "-b:v", "1500k", "-f", "hls", "o"])
        );
        assert_eq!(with_x264_params(x264.clone(), ""), x264);
        let own = args(&["-i", "a", "-c:v", "libx264", "-x264-params", "keyint=50", "o"]);
        assert_eq!(with_x264_params(own, "subme=0"), args(&["-i", "a", "-c:v", "libx264", "-x264-params", "keyint=50:subme=0", "o"]));
        for other in ["libx265", "h264_nvenc", "h264_vaapi"] {
            let command = args(&["-i", "a", "-c:v", other, "o"]);
            assert_eq!(with_x264_params(command.clone(), "subme=0"), command);
        }
    }

    #[test]
    fn profile_for_h264_encoders_without_one() {
        let x264 = args(&["-i", "a", "-c:v", "libx264", "o"]);
        assert_eq!(with_profile(x264.clone(), "main"), args(&["-i", "a", "-c:v", "libx264", "-profile:v", "main", "o"]));
        assert_eq!(with_profile(x264.clone(), ""), x264);
        let own = args(&["-i", "a", "-c:v", "libx264", "-profile:v", "high", "o"]);
        assert_eq!(with_profile(own.clone(), "main"), own);
        let nvenc = args(&["-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(with_profile(nvenc, "high"), args(&["-i", "a", "-c:v", "h264_nvenc", "-profile:v", "high", "o"]));
        let vaapi = args(&["-c:v", "h264_vaapi", "o"]);
        assert_eq!(with_profile(vaapi, "baseline"), args(&["-c:v", "h264_vaapi", "-profile:v", "constrained_baseline", "o"]));
        let hevc = args(&["-c:v", "hevc_nvenc", "o"]);
        assert_eq!(with_profile(hevc.clone(), "main"), hevc);
    }

    #[test]
    fn encoder_file_over_environment() {
        let dir = std::env::temp_dir().join(format!("vizrs-overlay-encoder-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(encoder::FILE);
        let env = || Settings { preset: "veryfast".into(), x264_params: "subme=0".into(), ..Settings::from_env().with_encoder_file("") };
        let missing = env().with_encoder_file(path.to_str().unwrap());
        assert_eq!((missing.preset.as_str(), missing.x264_params.as_str()), ("veryfast", "subme=0"));
        std::fs::write(&path, r#"{"preset":"superfast","profile":"main","x264_params":"keyint=1"}"#).unwrap();
        let set = env().with_encoder_file(path.to_str().unwrap());
        assert_eq!((set.preset.as_str(), set.profile.as_str(), set.x264_params.as_str()), ("superfast", "main", ""), "the file as a whole");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catchup_only_for_hls_inputs_at_a_readrate() {
        let hybrid = "http://localhost:8409/iptv/channel/4.m3u8?mode=segmenter&subtitles=false";
        let reader =
            args(&["-nostdin", "-readrate", "1.0", "-readrate_initial_burst", "6", "-i", hybrid, "-c", "copy", "-f", "mpegts", "pipe:1"]);
        assert_eq!(
            with_catchup(reader.clone(), "2"),
            args(&[
                "-nostdin",
                "-readrate",
                "1.0",
                "-readrate_catchup",
                "2",
                "-readrate_initial_burst",
                "6",
                "-i",
                hybrid,
                "-c",
                "copy",
                "-f",
                "mpegts",
                "pipe:1"
            ])
        );
        assert_eq!(with_catchup(reader.clone(), ""), reader);
        let own = args(&["-readrate", "1.0", "-readrate_catchup", "1.5", "-i", "a.m3u8", "-f", "mpegts", "o"]);
        assert_eq!(with_catchup(own.clone(), "2"), own);
        // The transcode of a media file, and a playlist read without a readrate.
        let media =
            args(&["-readrate", "1.0", "-i", "http://localhost:8409/internal/media/1?sig=x", "-i", "b.m3u8", "-f", "hls", "o.m3u8"]);
        assert_eq!(with_catchup(media.clone(), "2"), media);
        let second = args(&["-i", "a.mp4", "-readrate", "1", "-i", "b.m3u8", "o"]);
        assert_eq!(with_catchup(second, "2"), args(&["-i", "a.mp4", "-readrate", "1", "-readrate_catchup", "2", "-i", "b.m3u8", "o"]));
    }

    #[test]
    fn graphics_placed_at_the_area() {
        // ErsatzTV's software, NVIDIA and VAAPI graphs, the canvas as input 1.
        let cases = [
            ("[0:0]scale=8:4[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0[v_o0]", "[v_m0][v_s0]overlay=x=10:y=940[v_o0]"),
            (
                "[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]",
                "[v_m0][v_s0]overlay_cuda=x=10:y=940[v_o0]",
            ),
            (
                "[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=format=0,format=nv12|vaapi,hwupload[v_o0]",
                "[v_m0][v_s0]overlay=format=0:x=10:y=940,format",
            ),
            ("[1:0]format=yuva420p,hwupload[v_s0];[v_m0][v_s0]overlay_vaapi[v_o0]", "[v_m0][v_s0]overlay_vaapi=x=10:y=940[v_o0]"),
            ("[0][1]overlay", "[0][1]overlay=x=10:y=940"),
            // A watermark (input 2) overlaid first: only the graphics move.
            (
                "[0:0][2:0]overlay=x=5:y=5[w];[1:0]format=yuva420p[g];[w][g]overlay[o]",
                "[0:0][2:0]overlay=x=5:y=5[w];[1:0]format=yuva420p[g];[w][g]overlay=x=10:y=940[o]",
            ),
        ];
        for (filter, expected) in cases {
            let placed = placed(filter, 1, 10, 940).unwrap();
            assert!(placed.contains(expected), "{placed}");
            assert_eq!(placed.matches("overlay").count(), filter.matches("overlay").count());
        }
        // Positional options, graphics scaled to the channel, no overlay of the canvas: not placed.
        assert_eq!(placed("[0][1]overlay=5:5", 1, 10, 940), None);
        assert_eq!(placed("[1:0]scale=1920:1080,format=yuva420p[g];[0][g]overlay", 1, 10, 940), None);
        assert_eq!(placed("[0]scale=8:4[v]", 1, 10, 940), None);
        assert_eq!(placed("[0][2]overlay", 1, 10, 940), None);

        // Two areas stacked: split, crop, prepare each, overlay one after another.
        let (ticker, logo) = (Area { x: 0, y: 930, w: 1920, h: 140 }, Area { x: 1760, y: 40, w: 140, h: 140 });
        let two = [(ticker, 0), (logo, 140)];
        assert_eq!(
            placed_areas("[0:0]scale=8:4[v_m0];[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]", 1, &two)
                .unwrap(),
            "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1920:140:0:0,format=yuva420p,hwupload_cuda[vz_g0];\
             [vz_s1]crop=140:140:0:140,format=yuva420p,hwupload_cuda[vz_g1];[0:0]scale=8:4[v_m0];\
             [v_m0][vz_g0]overlay_cuda=x=0:y=930[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=1760:y=40[v_o0]"
        );
        assert_eq!(
            placed_areas("[0][1]overlay=format=0,format=nv12[o]", 1, &two).unwrap(),
            "[1]split=2[vz_s0][vz_s1];[vz_s0]crop=1920:140:0:0[vz_g0];[vz_s1]crop=140:140:0:140[vz_g1];\
             [0][vz_g0]overlay=format=0:x=0:y=930[vz_o0];[vz_o0][vz_g1]overlay=format=0:x=1760:y=40,format=nv12[o]"
        );
        assert_eq!(placed_areas("[1:0]scale=1920:1080[g];[0][g]overlay", 1, &two), None);

        // ErsatzTV Next: audio in a graph of its own, before the video's.
        let next = args(&[
            "-i",
            "a",
            "-i",
            "http://x/internal/graphics/4/7",
            "-filter_complex",
            "[0:1]loudnorm=I=-16[a]",
            "-filter_complex",
            "[0:0]fps=25[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0:format=0[v_o0]",
            "-f",
            "hls",
            "o",
        ]);
        let moved = with_areas(&next, 1, &two).unwrap();
        assert_eq!(moved[5], next[5]);
        assert!(moved[7].to_str().unwrap().contains("[vz_o0][vz_g1]overlay=format=0:x=1760:y=40[v_o0]"), "{:?}", moved[7]);

        let list = args(&["-i", "a", "-i", "http://x/internal/graphics/1/7", "-filter_complex", "[0][1]overlay[o]", "-f", "hls", "o"]);
        assert_eq!(input_number(&list, 2), 1);
        assert_eq!(with_position(&list, 1, 2, 4).unwrap()[5], "[0][1]overlay=x=2:y=4[o]");
        assert_eq!(with_position(&args(&["-i", "a", "-f", "null", "-"]), 1, 2, 4), None);
    }

    #[test]
    fn form_values() {
        assert_eq!(quote_plus("1 2/ä"), "1+2%2F%C3%A4");
        assert_eq!(unquote("a+b%2E"), "a+b.");
    }
}
