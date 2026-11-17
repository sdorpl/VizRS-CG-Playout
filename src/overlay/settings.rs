//! Ustawienia nakładki ze środowiska: ffmpeg, kodowanie, CUDA, strefy, pobieranie klatek.

use super::*;

/// The overlay's environment.
pub struct Settings {
    pub ffmpeg: OsString,
    /// Sidecar URL without the trailing `/`; empty = no overlay.
    pub sidecar: String,
    /// Forced channel frame rate; empty = the media's.
    pub fps: String,
    /// Preset of software encoders without one; empty = ErsatzTV's.
    pub preset: String,
    /// H.264 profile of libx264 without one (`-profile:v`); only from VizRS Live, empty = x264's.
    pub profile: String,
    /// Options of libx264 (`-x264-params`); empty = none.
    pub x264_params: String,
    /// Catch-up rate of HLS playlist inputs read at `-readrate`; empty = ffmpeg's.
    pub catchup: String,
    /// Blend frames in the media's frame rate conversion.
    pub blend: bool,
    /// Live ingest of the sidecar takes transcodes to HLS over (src/takeover.rs).
    pub ingest: bool,
    /// Picture of a live programme without its source; empty = black.
    pub standby: String,
    /// What of an NVENC command stays on the card (VIZRS_FFMPEG_CUDA).
    pub cuda: Cuda,
    /// Preset of NVENC encoders without one (p1–p7); only from VizRS Live, empty = ffmpeg's (p4).
    pub nvenc_preset: String,
    /// Preset of QSV encoders without one; only from VizRS Live, empty = the driver's.
    pub qsv_preset: String,
    /// B-frames of hardware encoders (NVENC, QSV, VAAPI) without `-bf`; empty = ffmpeg's.
    pub bframes: String,
    /// Free GPU memory (MiB) `Cuda::Auto` needs for `overlay` (VIZRS_FFMPEG_CUDA_MIN_FREE).
    pub cuda_min_free: u64,
    /// `-shortest` left out of commands whose output has a `-t` (VIZRS_FFMPEG_NO_SHORTEST).
    pub no_shortest: bool,
    /// With `Cuda::Overlay`, media frames wait for the graphics in RAM (VIZRS_FFMPEG_SYNC_RAM).
    pub sync_ram: bool,
    /// Stream copies of HLS playlists keep frames flagged corrupt (VIZRS_FFMPEG_NO_DISCARDCORRUPT).
    pub no_discardcorrupt: bool,
}

/// What of a command encoding with NVENC stays on the card (`VIZRS_FFMPEG_CUDA`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Cuda {
    /// Empty: ErsatzTV's command (CUDA decoding, filters and encoder).
    #[default]
    Full,
    /// `encode`: only the encoder; decoding and filters on the CPU.
    Encode,
    /// `overlay`: decoding on the CPU; filters, the overlay of the graphics and the encoder on the card.
    Overlay,
    /// `auto`: `overlay` while the card has enough free memory, `encode` below.
    Auto,
}

impl Cuda {
    pub fn parse(spec: &str) -> Cuda {
        match spec.trim().to_ascii_lowercase().as_str() {
            "encode" => Cuda::Encode,
            "overlay" => Cuda::Overlay,
            "auto" => Cuda::Auto,
            _ => Cuda::Full,
        }
    }
}

impl Settings {
    pub fn from_env() -> Settings {
        let var = |name| std::env::var(name).unwrap_or_default();
        Settings {
            ffmpeg: std::env::var_os("VIZRS_FFMPEG").unwrap_or_else(|| DEFAULT_FFMPEG.into()),
            sidecar: var("VIZRS_CG_URL").trim_end_matches('/').to_owned(),
            fps: var("VIZRS_FFMPEG_FPS").trim().to_owned(),
            preset: var("VIZRS_FFMPEG_PRESET").trim().to_owned(),
            profile: String::new(),
            x264_params: var("VIZRS_FFMPEG_X264_PARAMS").trim().to_owned(),
            catchup: var("VIZRS_FFMPEG_READRATE_CATCHUP").trim().to_owned(),
            blend: var("VIZRS_FFMPEG_FPS_BLEND").trim() == "1",
            ingest: var("VIZRS_FFMPEG_INGEST").trim() == "1",
            standby: var("VIZRS_FFMPEG_STANDBY").trim().to_owned(),
            cuda: Cuda::parse(&var("VIZRS_FFMPEG_CUDA")),
            nvenc_preset: String::new(),
            qsv_preset: String::new(),
            bframes: var("VIZRS_FFMPEG_NVENC_BF").trim().to_owned(),
            cuda_min_free: var("VIZRS_FFMPEG_CUDA_MIN_FREE").trim().parse().unwrap_or(DEFAULT_CUDA_MIN_FREE),
            no_shortest: var("VIZRS_FFMPEG_NO_SHORTEST").trim() == "1",
            sync_ram: var("VIZRS_FFMPEG_SYNC_RAM").trim() == "1",
            no_discardcorrupt: var("VIZRS_FFMPEG_NO_DISCARDCORRUPT").trim() == "1",
        }
        .with_encoder_file(&std::env::var("VIZRS_FFMPEG_SETTINGS").unwrap_or_else(|_| encoder::DEFAULT_PATH.into()))
    }

    /// The encoder settings of VizRS Live (`encoder.json`, src/encoder.rs) over
    /// those of the environment, key by key; `path` empty or no file = the environment's.
    pub fn with_encoder_file(self, path: &str) -> Settings {
        let env = Encoder {
            preset: self.preset.clone(),
            profile: self.profile.clone(),
            x264_params: self.x264_params.clone(),
            nvenc_preset: self.nvenc_preset.clone(),
            qsv_preset: self.qsv_preset.clone(),
            bframes: self.bframes.clone(),
        };
        let (Encoder { preset, profile, x264_params, nvenc_preset, qsv_preset, bframes }, _) =
            Encoder::in_force((!path.is_empty()).then(|| Path::new(path)), &env);
        Settings { preset, profile, x264_params, nvenc_preset, qsv_preset, bframes, ..self }
    }
}
