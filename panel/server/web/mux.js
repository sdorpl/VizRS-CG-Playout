// Przepakowanie segmentów HLS (MPEG-TS z H.264 albo HEVC + AAC albo AC-3) na fragmenty MP4 dla Media Source Extensions.
// HEVC i AC-3 tylko, gdy przeglądarka je dekoduje w MSE (Transmuxer({ hevc, ac3 }) – sprawdza player.js; K18).
// Bez kodowania – tylko zmiana opakowania, więc podgląd kanału kosztuje przeglądarkę tyle, co zwykły odtwarzacz.
// Moduł bez DOM (test w Node: tests/e2e.cjs).

const AAC_RATES = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
const TS = 90000; // zegar MPEG-TS i ścieżki obrazu

// ---------- MPEG-TS ----------

function pesTime(b, i) {
  return (b[i] & 0x0e) * 536870912 + b[i + 1] * 4194304 + (b[i + 2] & 0xfe) * 16384 + b[i + 3] * 128 + (b[i + 4] >> 1);
}

/** Pakiety PES obrazu i dźwięku z jednego segmentu: {video: [{pts, dts, data}], audio: [...], videoType, audioType}. */
export class TsDemuxer {
  constructor() { this.pmt = -1; this.vpid = -1; this.apid = -1; this.videoType = 0; this.audioType = 0; }

  parse(buf) {
    const out = { video: [], audio: [] };
    const open = new Map();
    let off = buf.indexOf(0x47);
    while (off >= 0 && off + 188 <= buf.length) {
      if (buf[off] !== 0x47) { off = buf.indexOf(0x47, off + 1); continue; }
      const start = buf[off + 1] & 0x40;
      const pid = ((buf[off + 1] & 0x1f) << 8) | buf[off + 2];
      const afc = (buf[off + 3] >> 4) & 3;
      let p = off + 4;
      if (afc & 2) p += 1 + buf[off + 4];
      const payload = afc & 1 && p < off + 188 ? buf.subarray(p, off + 188) : null;
      off += 188;
      if (!payload) continue;
      if (pid === 0) { if (start) this.pat(payload); continue; }
      if (pid === this.pmt) { if (start) this.pmtTable(payload); continue; }
      if (pid !== this.vpid && pid !== this.apid) continue;
      let pes = open.get(pid);
      if (start) {
        if (pes) this.flush(pid, pes, out);
        pes = { chunks: [], len: 0 };
        open.set(pid, pes);
      }
      if (pes) { pes.chunks.push(payload); pes.len += payload.length; }
    }
    for (const [pid, pes] of open) this.flush(pid, pes, out);
    out.videoType = this.videoType;
    out.audioType = this.audioType;
    return out;
  }

  pat(d) {
    const q = d[0] + 1, end = q + 3 + (((d[q + 1] & 0x0f) << 8) | d[q + 2]) - 4;
    for (let i = q + 8; i + 4 <= end; i += 4) {
      if ((d[i] << 8 | d[i + 1]) !== 0) { this.pmt = ((d[i + 2] & 0x1f) << 8) | d[i + 3]; return; }
    }
  }

  pmtTable(d) {
    const q = d[0] + 1, end = q + 3 + (((d[q + 1] & 0x0f) << 8) | d[q + 2]) - 4;
    let i = q + 12 + (((d[q + 10] & 0x0f) << 8) | d[q + 11]);
    while (i + 5 <= end) {
      const type = d[i], pid = ((d[i + 1] & 0x1f) << 8) | d[i + 2];
      if (this.vpid < 0 && (type === 0x1b || type === 0x24)) { this.vpid = pid; this.videoType = type; }
      if (this.apid < 0 && (type === 0x0f || type === 0x11 || type === 0x81)) { this.apid = pid; this.audioType = type; }
      i += 5 + (((d[i + 3] & 0x0f) << 8) | d[i + 4]);
    }
  }

  flush(pid, pes, out) {
    if (pes.len < 9) return;
    const b = new Uint8Array(pes.len);
    let o = 0;
    for (const c of pes.chunks) { b.set(c, o); o += c.length; }
    if (b[0] !== 0 || b[1] !== 0 || b[2] !== 1) return;
    const declared = (b[4] << 8) | b[5];
    if (declared && declared + 6 > b.length) return; // ucięty na końcu segmentu
    const flags = b[7] >> 6, hdr = 9 + b[8];
    if (!(flags & 2) || hdr > b.length) return;
    const pts = pesTime(b, 9);
    const dts = flags === 3 ? pesTime(b, 14) : pts;
    const data = b.subarray(hdr, declared ? declared + 6 : b.length);
    (pid === this.vpid ? out.video : out.audio).push({ pts, dts, data });
  }
}

// ---------- H.264 ----------

/** Jednostki NAL z zapisu Annex B (kody startu 00 00 01 / 00 00 00 01). */
export function nalUnits(d) {
  const units = [];
  let start = -1;
  for (let i = 0; i + 2 < d.length;) {
    if (d[i] === 0 && d[i + 1] === 0 && d[i + 2] === 1) {
      if (start >= 0) { let e = i; while (e > start && d[e - 1] === 0) e--; units.push(d.subarray(start, e)); }
      i += 3; start = i;
    } else i++;
  }
  if (start >= 0 && start < d.length) units.push(d.subarray(start));
  return units;
}

class Bits {
  constructor(nal) {
    const b = [];
    for (let i = 1; i < nal.length; i++) { // bez nagłówka NAL i bajtów 03 chroniących przed kodem startu
      if (i >= 3 && nal[i] === 3 && nal[i - 1] === 0 && nal[i - 2] === 0) continue;
      b.push(nal[i]);
    }
    this.b = b; this.p = 0;
  }
  u(n) { let v = 0; for (let k = 0; k < n; k++) { v = v * 2 + ((this.b[this.p >> 3] >> (7 - (this.p & 7))) & 1); this.p++; } return v; }
  ue() { let z = 0; while (this.u(1) === 0 && z < 32) z++; return 2 ** z - 1 + this.u(z); }
  se() { const v = this.ue(); return v & 1 ? (v + 1) / 2 : -v / 2; }
}

/** Rozmiar obrazu z SPS (z przycięciem). */
export function spsSize(sps) {
  const r = new Bits(sps);
  const profile = r.u(8); r.u(16); r.ue();
  let chroma = 1;
  if ([100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].includes(profile)) {
    chroma = r.ue();
    if (chroma === 3) r.u(1);
    r.ue(); r.ue(); r.u(1);
    if (r.u(1)) {
      for (let i = 0; i < (chroma === 3 ? 12 : 8); i++) {
        if (!r.u(1)) continue;
        let last = 8, next = 8;
        for (let j = 0; j < (i < 6 ? 16 : 64); j++) {
          if (next) next = (last + r.se() + 256) % 256;
          last = next || last;
        }
      }
    }
  }
  r.ue();
  const poc = r.ue();
  if (poc === 0) r.ue();
  else if (poc === 1) { r.u(1); r.se(); r.se(); for (let n = r.ue(); n > 0; n--) r.se(); }
  r.ue(); r.u(1);
  const w = r.ue() + 1, hgt = r.ue() + 1, frameOnly = r.u(1);
  if (!frameOnly) r.u(1);
  r.u(1);
  let cl = 0, cr = 0, ct = 0, cb = 0;
  if (r.u(1)) { cl = r.ue(); cr = r.ue(); ct = r.ue(); cb = r.ue(); }
  const ux = chroma === 1 || chroma === 2 ? 2 : 1;
  const uy = (chroma === 1 ? 2 : 1) * (2 - frameOnly);
  return { width: w * 16 - (cl + cr) * ux, height: (2 - frameOnly) * hgt * 16 - (ct + cb) * uy };
}

const hex2 = (n) => n.toString(16).padStart(2, '0');

// ---------- HEVC (H.265, K18) ----------

/** SPS HEVC: rozmiar (z przycięciem), profile_tier_level (12 bajtów), chroma i głębia – do hvcC i nazwy kodeka. */
export function hevcSps(sps) {
  const r = new Bits(sps.subarray(1)); // nagłówek NAL HEVC ma 2 bajty – Bits pomija drugi
  r.u(4);
  const subLayers = r.u(3);
  r.u(1);
  const ptl = [];
  for (let i = 0; i < 12; i++) ptl.push(r.u(8));
  const subProf = [], subLev = [];
  for (let i = 0; i < subLayers; i++) { subProf.push(r.u(1)); subLev.push(r.u(1)); }
  if (subLayers > 0) for (let i = subLayers; i < 8; i++) r.u(2);
  for (let i = 0; i < subLayers; i++) { if (subProf[i]) { r.u(32); r.u(32); r.u(24); } if (subLev[i]) r.u(8); }
  r.ue();
  const chroma = r.ue();
  if (chroma === 3) r.u(1);
  let width = r.ue(), height = r.ue();
  if (r.u(1)) {
    const cl = r.ue(), cr = r.ue(), ct = r.ue(), cb = r.ue();
    width -= (cl + cr) * (chroma === 1 || chroma === 2 ? 2 : 1);
    height -= (ct + cb) * (chroma === 1 ? 2 : 1);
  }
  return { width, height, ptl, chroma, depthLuma: r.ue() + 8, depthChroma: r.ue() + 8, subLayers };
}

/** Nazwa kodeka HEVC wg ISO/IEC 14496-15 (np. hvc1.1.6.L93.B0). */
export function hevcCodec(ptl) {
  const space = ['', 'A', 'B', 'C'][ptl[0] >> 6];
  let compat = (ptl[1] << 24 | ptl[2] << 16 | ptl[3] << 8 | ptl[4]) >>> 0, rev = 0;
  for (let i = 0; i < 32; i++) { rev = (rev << 1 | (compat & 1)) >>> 0; compat >>>= 1; }
  const cons = ptl.slice(5, 11);
  while (cons.length > 1 && cons[cons.length - 1] === 0) cons.pop();
  return `hvc1.${space}${ptl[0] & 31}.${rev.toString(16).toUpperCase()}.${ptl[0] & 32 ? 'H' : 'L'}${ptl[11]}.${cons.map((b) => b.toString(16).toUpperCase()).join('.')}`;
}

// ---------- AC-3 (K18) ----------

const AC3_RATES = [48000, 44100, 32000];
const AC3_WORDS = [
  [64, 64, 80, 80, 96, 96, 112, 112, 128, 128, 160, 160, 192, 192, 224, 224, 256, 256, 320, 320, 384, 384, 448, 448, 512, 512, 640, 640, 768, 768, 896, 896, 1024, 1024, 1152, 1152, 1280, 1280],
  [69, 70, 87, 88, 104, 105, 121, 122, 139, 140, 174, 175, 208, 209, 243, 244, 278, 279, 348, 349, 417, 418, 487, 488, 557, 558, 696, 697, 835, 836, 975, 976, 1114, 1115, 1253, 1254, 1393, 1394],
  [96, 96, 120, 120, 144, 144, 168, 168, 192, 192, 240, 240, 288, 288, 336, 336, 384, 384, 480, 480, 576, 576, 672, 672, 768, 768, 960, 960, 1152, 1152, 1344, 1344, 1536, 1536, 1728, 1728, 1920, 1920],
];
const AC3_CHANNELS = [2, 1, 2, 3, 3, 4, 4, 5];

/** Nagłówek ramki AC-3 (od słowa synchronizacji 0B 77): parametry do dac3 i długość ramki w bajtach. */
export function ac3Header(d, i) {
  const fscod = d[i + 4] >> 6, frmsizecod = d[i + 4] & 63;
  if (fscod > 2 || frmsizecod > 37) return null;
  const bsid = d[i + 5] >> 3, bsmod = d[i + 5] & 7;
  const bits = (d[i + 6] << 8) | d[i + 7];
  const acmod = bits >> 13;
  let p = 3; // bity po acmod (od najstarszego w `bits`)
  if ((acmod & 1) && acmod !== 1) p += 2;
  if (acmod & 4) p += 2;
  if (acmod === 2) p += 2;
  const lfeon = (bits >> (15 - p)) & 1;
  return { fscod, frmsizecod, bsid, bsmod, acmod, lfeon, len: AC3_WORDS[fscod][frmsizecod] * 2, rate: AC3_RATES[fscod], channels: AC3_CHANNELS[acmod] + lfeon };
}

// ---------- MP4 ----------

function box(type, ...parts) {
  const flat = parts.flat(Infinity);
  let size = 8;
  for (const p of flat) size += p.length;
  const out = new Uint8Array(size);
  new DataView(out.buffer).setUint32(0, size);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  let o = 8;
  for (const p of flat) { out.set(p, o); o += p.length; }
  return out;
}
const u8 = (...v) => Uint8Array.from(v);
const u16 = (v) => u8(v >> 8 & 255, v & 255);
const u32 = (v) => u8(v >>> 24 & 255, v >>> 16 & 255, v >>> 8 & 255, v & 255);
const u64 = (v) => [u32(Math.floor(v / 4294967296)), u32(v % 4294967296 >>> 0)];
const zeros = (n) => new Uint8Array(n);
const str = (s) => Uint8Array.from(s, (c) => c.charCodeAt(0));
const full = (type, version, flags, ...parts) => box(type, u32((version << 24) | flags), ...parts);
const MATRIX = [0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000].map(u32);

function sampleEntry(t) {
  if (t.kind === 'video' && t.hevc) {
    const s = t.hs;
    const arrays = [[32, t.vps], [33, t.sps], [34, t.pps]].map(([type, n]) => [u8(0x80 | type), u16(1), u16(n.length), n]);
    const hvcC = box('hvcC', u8(1, ...s.ptl.slice(0, 1)), u8(...s.ptl.slice(1, 11)), u8(s.ptl[11]), u16(0xf000), u8(0xfc, 0xfc | s.chroma, 0xf8 | (s.depthLuma - 8), 0xf8 | (s.depthChroma - 8)),
      u16(0), u8(((s.subLayers + 1) << 3) | (1 << 2) | 3), u8(3), arrays);
    return box('hvc1', zeros(6), u16(1), zeros(16), u16(t.width), u16(t.height), u32(0x00480000), u32(0x00480000), u32(0), u16(1), zeros(32), u16(0x18), u16(0xffff), hvcC);
  }
  if (t.kind === 'audio' && t.ac3) {
    const c = t.ac3;
    const v = (c.fscod << 22) | (c.bsid << 17) | (c.bsmod << 14) | (c.acmod << 11) | (c.lfeon << 10) | ((c.frmsizecod >> 1) << 5);
    const dac3 = box('dac3', u8(v >> 16 & 255, v >> 8 & 255, v & 255));
    return box('ac-3', zeros(6), u16(1), zeros(8), u16(t.channels), u16(16), zeros(4), u32(t.rate * 65536), dac3);
  }
  if (t.kind === 'video') {
    const avcC = box('avcC', u8(1, t.sps[1], t.sps[2], t.sps[3], 0xff, 0xe1), u16(t.sps.length), t.sps, u8(1), u16(t.pps.length), t.pps);
    return box('avc1', zeros(6), u16(1), zeros(16), u16(t.width), u16(t.height), u32(0x00480000), u32(0x00480000), u32(0), u16(1), zeros(32), u16(0x18), u16(0xffff), avcC);
  }
  const asc = u16((t.object << 11) | (t.rateIndex << 7) | (t.channels << 3));
  const dcd = [u8(4, 17, 0x40, 0x15, 0, 0, 0), u32(0), u32(0), u8(5, 2), asc];
  const esds = full('esds', 0, 0, u8(3, 25), u16(t.id), u8(0), dcd, u8(6, 1, 2));
  return box('mp4a', zeros(6), u16(1), zeros(8), u16(t.channels), u16(16), zeros(4), u32(t.rate * 65536), esds);
}

/** Segment inicjujący (ftyp + moov) jednej ścieżki. */
export function initSegment(t) {
  const video = t.kind === 'video';
  const empty = (type) => full(type, 0, 0, u32(0));
  const trak = box('trak',
    full('tkhd', 0, 3, u32(0), u32(0), u32(t.id), u32(0), u32(0), zeros(8), u16(0), u16(0), u16(video ? 0 : 0x0100), u16(0), MATRIX,
      u32((video ? t.width : 0) * 65536), u32((video ? t.height : 0) * 65536)),
    box('mdia',
      full('mdhd', 0, 0, u32(0), u32(0), u32(t.timescale), u32(0), u16(0x55c4), u16(0)),
      full('hdlr', 0, 0, u32(0), str(video ? 'vide' : 'soun'), zeros(12), str(video ? 'VideoHandler\0' : 'SoundHandler\0')),
      box('minf',
        video ? full('vmhd', 0, 1, zeros(8)) : full('smhd', 0, 0, zeros(4)),
        box('dinf', full('dref', 0, 0, u32(1), full('url ', 0, 1))),
        box('stbl', full('stsd', 0, 0, u32(1), sampleEntry(t)), empty('stts'), empty('stsc'), full('stsz', 0, 0, u32(0), u32(0)), empty('stco')))));
  const moov = box('moov',
    full('mvhd', 0, 0, u32(0), u32(0), u32(1000), u32(0), u32(0x00010000), u16(0x0100), zeros(10), MATRIX, zeros(24), u32(0xffffffff)),
    trak,
    box('mvex', full('trex', 0, 0, u32(t.id), u32(1), u32(0), u32(0), u32(0))));
  return concat([box('ftyp', str('isom'), u32(1), str('isomiso6avc1mp41')), moov]);
}

/** Fragment (moof + mdat): samples = [{data, duration, cts, key}], base = czas dekodowania pierwszej próbki. */
export function fragment(t, seq, base, samples) {
  const trunLen = 12 + 8 + samples.length * 16;
  const moofLen = 8 + 16 + 8 + 16 + 20 + trunLen;
  const rows = samples.map((s) => [u32(s.duration), u32(s.data.length), u32(s.key ? 0x02000000 : 0x01010000), u32(s.cts | 0)]);
  const moof = box('moof',
    full('mfhd', 0, 0, u32(seq)),
    box('traf',
      full('tfhd', 0, 0x020000, u32(t.id)),
      full('tfdt', 1, 0, u64(base)),
      full('trun', 1, 0xf01, u32(samples.length), u32(moofLen + 8), rows)));
  return concat([moof, box('mdat', samples.map((s) => s.data))]);
}

export function concat(parts) {
  let n = 0;
  for (const p of parts) n += p.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of parts) { out.set(p, o); o += p.length; }
  return out;
}

const sameBytes = (a, b) => a && b && a.length === b.length && a.every((v, i) => v === b[i]);

// ---------- przepakowanie segmentów ----------

/**
 * Segment po segmencie: push(ts, {pdt, disc}) → {video, audio, start, end, error}.
 * video/audio = {codec, init?, data} (init tylko przy pierwszym segmencie i zmianie parametrów).
 * Oś czasu: gdy playlista ma PROGRAM-DATE-TIME, czas w MSE = (pdt − epoch) s, czyli zegar anteny.
 */
export class Transmuxer {
  /** opts.hevc / opts.ac3 – przeglądarka dekoduje HEVC / AC-3 w MSE (inaczej HEVC = błąd „HEVC”, AC-3 = bez dźwięku). */
  constructor(opts = {}) {
    this.opts = opts;
    this.demux = new TsDemuxer();
    this.offset = null;   // przesunięcie znaczników PES → oś odtwarzacza (90 kHz)
    this.epoch = null;    // ms; początek osi odtwarzacza przy PROGRAM-DATE-TIME
    this.end = 0;         // koniec poprzedniego segmentu na osi (90 kHz)
    this.v = { kind: 'video', id: 1, timescale: TS, seq: 0, dur: 3600, started: false };
    this.a = { kind: 'audio', id: 2, seq: 0 };
  }

  push(ts, { pdt = null, disc = false } = {}) {
    const d = this.demux.parse(ts);
    const res = { video: null, audio: null, start: 0, end: 0, error: null };
    if (d.videoType === 0x24 && !this.opts.hevc) { res.error = 'HEVC'; return res; }
    this.v.hevc = d.videoType === 0x24;
    const firstV = d.video.length ? d.video[0].pts : null;
    const firstA = d.audio.length ? d.audio[0].pts : null;
    const first = firstV ?? firstA;
    if (first == null) return res;
    if (pdt != null && this.epoch == null) this.epoch = pdt - 10000;
    const want = pdt != null ? (pdt - this.epoch) * 90 : this.end || TS;
    if (this.offset == null || disc || Math.abs(first + this.offset - want) > TS / 2) this.offset = Math.round(want - first);
    res.video = this.videoFragment(d.video);
    res.audio = d.audioType === 0x0f ? this.audioFragment(d.audio) : d.audioType === 0x81 && this.opts.ac3 ? this.ac3Fragment(d.audio) : null;
    res.start = (first + this.offset) / TS;
    res.end = this.end / TS;
    return res;
  }

  videoFragment(list) {
    const t = this.v;
    const samples = [];
    for (const pes of list) {
      const nals = [];
      let key = false;
      for (const n of nalUnits(pes.data)) {
        if (t.hevc) {
          const ht = (n[0] >> 1) & 0x3f;
          if (ht === 32) { if (!sameBytes(n, t.vps)) { t.vps = n.slice(); t.changed = true; } continue; }
          if (ht === 33) { if (!sameBytes(n, t.sps)) { t.sps = n.slice(); t.changed = true; } continue; }
          if (ht === 34) { if (!sameBytes(n, t.pps)) { t.pps = n.slice(); t.changed = true; } continue; }
          if (ht === 35) continue; // AUD
          if (ht >= 16 && ht <= 21) key = true; // BLA, IDR, CRA
          nals.push(u32(n.length), n);
          continue;
        }
        const type = n[0] & 0x1f;
        if (type === 7) { if (!sameBytes(n, t.sps)) { t.sps = n.slice(); t.changed = true; } continue; }
        if (type === 8) { if (!sameBytes(n, t.pps)) { t.pps = n.slice(); t.changed = true; } continue; }
        if (type === 9 || type === 12) continue;
        if (type === 5) key = true;
        nals.push(u32(n.length), n);
      }
      if (!nals.length) continue;
      samples.push({ dts: pes.dts + this.offset, pts: pes.pts + this.offset, key, data: concat(nals) });
    }
    if (!t.sps || !t.pps || (t.hevc && !t.vps)) return null;
    if (!t.started) {
      const k = samples.findIndex((s) => s.key);
      if (k < 0) return null;
      samples.splice(0, k);
      t.started = true;
    }
    if (!samples.length) return null;
    for (let i = 0; i < samples.length; i++) {
      const s = samples[i];
      if (i + 1 < samples.length) t.dur = Math.max(1, samples[i + 1].dts - s.dts);
      s.duration = t.dur;
      s.cts = s.pts - s.dts;
    }
    if (t.changed && t.hevc) { t.hs = hevcSps(t.sps); t.width = t.hs.width; t.height = t.hs.height; }
    const out = { codec: t.hevc ? hevcCodec(t.hs.ptl) : 'avc1.' + hex2(t.sps[1]) + hex2(t.sps[2]) + hex2(t.sps[3]) };
    if (t.changed) { if (!t.hevc) Object.assign(t, spsSize(t.sps)); out.init = initSegment(t); t.changed = false; }
    const base = Math.max(0, samples[0].dts);
    out.data = fragment(t, ++t.seq, base, samples);
    this.end = Math.max(this.end, samples[samples.length - 1].dts + t.dur);
    return out;
  }

  /** AC-3: ramki z PES (słowo synchronizacji 0B 77), 1536 próbek na ramkę. */
  ac3Fragment(list) {
    const t = this.a;
    const frames = [];
    let conf = null;
    for (const pes of list) {
      const d = pes.data;
      for (let i = 0; i + 8 <= d.length;) {
        if (d[i] !== 0x0b || d[i + 1] !== 0x77) { i++; continue; }
        const hd = ac3Header(d, i);
        if (!hd || i + hd.len > d.length) break;
        conf = hd;
        frames.push(d.subarray(i, i + hd.len));
        i += hd.len;
      }
    }
    if (!conf || !frames.length) return null;
    const out = { codec: 'ac-3' };
    const key = `${conf.fscod}/${conf.acmod}/${conf.lfeon}/${conf.bsid}/${conf.frmsizecod >> 1}`;
    if (t.ac3key !== key) {
      Object.assign(t, { ac3: conf, ac3key: key, rate: conf.rate, timescale: conf.rate, channels: conf.channels, object: null });
      out.init = initSegment(t);
    }
    const base = Math.max(0, Math.round((list[0].pts + this.offset) * t.rate / TS));
    out.data = fragment(t, ++t.seq, base, frames.map((f) => ({ data: f, duration: 1536, cts: 0, key: true })));
    return out;
  }

  audioFragment(list) {
    const t = this.a;
    const frames = [];
    let conf = null;
    for (const pes of list) {
      const d = pes.data;
      for (let i = 0; i + 7 <= d.length;) {
        if (d[i] !== 0xff || (d[i + 1] & 0xf6) !== 0xf0) { i++; continue; }
        const hdr = d[i + 1] & 1 ? 7 : 9;
        const len = ((d[i + 3] & 3) << 11) | (d[i + 4] << 3) | (d[i + 5] >> 5);
        if (len <= hdr || i + len > d.length) break;
        conf = { object: (d[i + 2] >> 6) + 1, rateIndex: (d[i + 2] >> 2) & 15, channels: ((d[i + 2] & 1) << 2) | (d[i + 3] >> 6) };
        frames.push({ pts: frames.length ? null : pes.pts, data: d.subarray(i + hdr, i + len) });
        i += len;
      }
    }
    if (!conf || !frames.length || !AAC_RATES[conf.rateIndex]) return null;
    const out = { codec: 'mp4a.40.' + conf.object };
    if (t.object !== conf.object || t.rateIndex !== conf.rateIndex || t.channels !== conf.channels) {
      Object.assign(t, conf, { rate: AAC_RATES[conf.rateIndex], timescale: AAC_RATES[conf.rateIndex], ac3: null, ac3key: null });
      out.init = initSegment(t);
    }
    const base = Math.max(0, Math.round((list[0].pts + this.offset) * t.rate / TS));
    out.data = fragment(t, ++t.seq, base, frames.map((f) => ({ data: f.data, duration: 1024, cts: 0, key: true })));
    return out;
  }
}
