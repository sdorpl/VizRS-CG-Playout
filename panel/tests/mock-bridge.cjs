// Atrapa mostu VizRS Bridge (bez Dockera i bez legacy) – do zrzutów UI i szybkiego sprawdzenia ekranów.
// Nie zastępuje stosu testowego (tests/stack.sh + e2e): dane są stałe, zapisy tylko w pamięci.
//
//   node tests/mock-bridge.cjs [port=18409] [token=test-token-0123456789]
//   LEGACY_SRC=<klon ErsatzTV/legacy> – metadane komend (parametry, wyliczenia) z rekordów MediatR jak /vizrs-bridge/commands;
//   bez niego – same nazwy z bridge/commands.txt (formularze bez list wyboru).
//   MOCK_CG=1 – atrapa VizRS CG Playout na porcie+1 (token test-cg-token-0123456789): stan z warstwami, TAKE/CONTINUE/OUT, dziennik emisji (K12–K17).
//   MOCK_HLS=1 – strumienie testowe kanałów 24–26 (ffmpeg: plansza testowa + ton, HLS) dla podglądu i mozaiki.
//   MOCK_NO_HW=1 – maszyna bez akceleracji sprzętowej (sonda Next: żadna akceleracja nie koduje) – ekran Profile kodowania bez karty.
// Panel: server/target/…/vizrs-live --bridge http://127.0.0.1:18409 --bridge-token <token> --web server/web
'use strict';
const http = require('http');
const fs = require('fs');
const path = require('path');

const PORT = Number(process.argv[2] || 18409);
const TOKEN = process.argv[3] || 'test-token-0123456789';
const ROOT = path.join(__dirname, '..');
const allowed = fs.readFileSync(path.join(ROOT, 'bridge/commands.txt'), 'utf8').split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'));

// ---------- metadane z rekordów legacy ----------
function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) { if (!/^(bin|obj|\.git|node_modules)$/.test(e.name)) walk(p, out); } else if (e.name.endsWith('.cs')) out.push(p);
  }
  return out;
}
function splitTop(s) {
  const out = []; let d = 0, cur = '';
  for (const ch of s) {
    if ('<([{'.includes(ch)) d++;
    if ('>)]}'.includes(ch)) d--;
    if (ch === ',' && d === 0) { out.push(cur); cur = ''; } else cur += ch;
  }
  if (cur.trim()) out.push(cur);
  return out;
}
const PRIM = { int: 'Int32', long: 'Int64', string: 'String', bool: 'Boolean', double: 'Double', float: 'Single', decimal: 'Decimal', 'byte[]': 'Byte[]' };
function typeName(t, enums) {
  t = t.trim();
  let m = t.match(/^Option<(.+)>$/);
  if (m) return typeName(m[1], enums) + '?';
  if (t.endsWith('?')) return typeName(t.slice(0, -1), enums) + '?';
  m = t.match(/^([A-Za-z]+)<(.+)>$/);
  if (m) return `${m[1]}<${splitTop(m[2]).map((x) => typeName(x, enums)).join(', ')}>`;
  return PRIM[t] || t;
}
function loadMeta() {
  const src = process.env.LEGACY_SRC;
  if (!src || !fs.existsSync(src)) return allowed.map((name) => ({ name, kind: /^(Get|Query|Search|Count)/.test(name) ? 'query' : 'command', parameters: [] }));
  const files = walk(src).filter((f) => !/Tests/.test(f));
  const enums = {}, records = {};
  for (const f of files) {
    const s = fs.readFileSync(f, 'utf8');
    for (const m of s.matchAll(/public enum (\w+)\s*(?::\s*\w+\s*)?\{([^}]*)\}/g)) {
      enums[m[1]] = m[2].replace(/\/\/[^\n]*/g, '').replace(/\[[^\]]*\]/g, '').split(',').map((x) => x.split('=')[0].trim()).filter((x) => /^\w+$/.test(x));
    }
    // rekordy z białej listy (także dziedziczące, np. CreateClassicPlayout : CreatePlayout)
    for (const m of s.matchAll(/public (?:sealed )?record (\w+)\s*(\(([^;{]*?)\))?\s*[:;{]/gs)) {
      if (allowed.includes(m[1]) && !records[m[1]]) records[m[1]] = { params: m[3] || '', query: /[\\/]Queries[\\/]/.test(f) };
    }
  }
  return allowed.filter((n) => records[n]).map((name) => {
    const r = records[name];
    const parameters = splitTop(r.params).map((p) => {
      p = p.replace(/\[[^\]]*\]/g, '').trim();
      const def = p.includes('=');
      const [decl] = p.split('=');
      const parts = decl.trim().split(/\s+/);
      const pname = parts.pop();
      const raw = parts.join(' ');
      const type = typeName(raw, enums);
      const base = raw.replace(/^Option<(.+)>$/, '$1').replace(/\?$/, '');
      return { name: pname, type, optional: def || type.endsWith('?'), values: enums[base] || null };
    }).filter((p) => p.name);
    return { name, kind: r.query ? 'query' : 'command', parameters };
  });
}
const META = loadMeta();
const KNOWN = new Set(META.map((m) => m.name));

// ---------- dane ----------
const now = Date.now();
const iso = (ms) => new Date(ms).toISOString();
const res = [
  { Id: 1, Name: '1920x1080', Width: 1920, Height: 1080, IsCustom: false },
  { Id: 2, Name: '1280x720', Width: 1280, Height: 720, IsCustom: false },
  { Id: 3, Name: '720x576', Width: 720, Height: 576, IsCustom: false },
  { Id: 9, Name: '1024x576', Width: 1024, Height: 576, IsCustom: true },
];
/** Klatkaż profili w moście (GET /framerates) – profil 2 ustawiony na 25. */
const FRAME_RATES = { 2: '25' };
const profile = (Id, Name, r, hw, extra = {}) => ({
  Id, Name, ThreadCount: 0, NormalizeAudio: true, NormalizeVideo: true, HardwareAcceleration: hw, VaapiDisplay: 'drm', VaapiDriver: 'Default', VaapiDevice: '',
  QsvExtraHardwareFrames: 64, Resolution: res[r], ScalingBehavior: 'ScaleAndPad', PadMode: 'Software', VideoFormat: 'H264', VideoProfile: 'high', VideoPreset: 'veryfast',
  AllowBFrames: false, BitDepth: 'EightBit', VideoBitrate: 4000, VideoBufferSize: 8000, TonemapAlgorithm: 'Linear', AudioFormat: 'Aac', AudioBitrate: 192, AudioBufferSize: 384,
  NormalizeLoudnessMode: 'LoudNorm', TargetLoudness: -23, AudioChannels: 2, AudioSampleRate: 48, NormalizeFramerate: false, NormalizeColors: true, DeinterlaceVideo: true, ...extra,
});
const DB = {
  resolutions: res,
  profiles: [profile(1, '1080p H.264 (NVENC)', 0, 'Nvenc'), profile(2, '720p H.264', 1, 'None', { VideoBitrate: 2500 }), profile(3, 'SD 576i VAAPI', 2, 'Vaapi', { VaapiDriver: 'iHD', VaapiDevice: '/dev/dri/renderD128' })],
  channels: [
    ['24', 'INFO24', 'Informacje', 1, 1], ['25', 'Muzyka VizRS', 'Muzyka', 2, 2], ['26', 'Kino Klasyka', 'Filmy', 2, 3], ['27', 'Sport Extra', 'Sport', 1, 4],
  ].map(([Number, Name, Group, FFmpegProfileId, Id]) => ({
    Id, Number, Name, Group, Categories: Group.toLowerCase(), FFmpegProfileId, SlugSeconds: null, Logo: { Path: '', ContentType: '' }, StreamSelectorMode: 'Default', StreamSelector: '',
    PreferredAudioLanguageCode: 'pol', PreferredAudioTitle: '', PlayoutSource: 'Generated', PlayoutMode: 'Continuous', MirrorSourceChannelId: null, PlayoutOffset: null, StreamingEngine: 'Next',
    NextEngineTextSubtitleMode: 'Burn', StreamingMode: 'HttpLiveStreamingSegmenter', WatermarkId: Id === 1 ? 1 : null, FallbackFillerId: 1, PlayoutCount: Id < 4 ? 1 : 0,
    PreferredSubtitleLanguageCode: 'pol', SubtitleMode: 'None', MusicVideoCreditsMode: 'None', MusicVideoCreditsTemplate: '', SongVideoMode: 'Default', TranscodeMode: 'OnDemand',
    IdleBehavior: 'StopOnDisconnect', IsEnabled: true, ShowInEpg: true,
  })),
  playouts: [
    { PlayoutId: 1, ScheduleKind: 'Classic', ChannelName: 'INFO24', ChannelNumber: '24', PlayoutMode: 'Continuous', ScheduleName: 'INFO24 – dzień', ScheduleFile: null, DbDailyRebuildTime: null, BuildStatus: { LastBuild: iso(now - 7200e3), Success: true, Message: '' } },
    { PlayoutId: 2, ScheduleKind: 'Block', ChannelName: 'Muzyka VizRS', ChannelNumber: '25', PlayoutMode: 'Continuous', ScheduleName: '', ScheduleFile: null, DbDailyRebuildTime: '04:00:00', BuildStatus: { LastBuild: iso(now - 7200e3), Success: true, Message: '' } },
    { PlayoutId: 3, ScheduleKind: 'Classic', ChannelName: 'Kino Klasyka', ChannelNumber: '26', PlayoutMode: 'Continuous', ScheduleName: 'Kino – wieczór', ScheduleFile: null, DbDailyRebuildTime: null, BuildStatus: { LastBuild: iso(now - 600e3), Success: false, Message: 'Collection "Westerny" is empty' } },
  ],
  watermarks: [{ Id: 1, Name: 'Logo INFO24', Image: '', ImageSource: 'ChannelLogo', Mode: 'Permanent', Location: 'TopRight', Size: 'Scaled', WidthPercent: 12, HorizontalMarginPercent: 4, VerticalMarginPercent: 5, FrequencyMinutes: 15, DurationSeconds: 15, Opacity: 90, PlaceWithinSourceContent: false, ZIndex: 0 }],
  fillers: [{ Id: 1, Name: 'Plansza zapasowa', FillerKind: 'Fallback', FillerMode: 'None' }, { Id: 2, Name: 'Zapowiedzi', FillerKind: 'MidRoll', FillerMode: 'Count' }],
};
const TITLES = {
  1: ['Serwis informacyjny', 'Pogoda', 'Rozmowa dnia', 'Serwis informacyjny', 'Magazyn regionalny', 'Pogoda', 'Gość VizRS', 'Serwis informacyjny'],
  2: ['Lista przebojów', 'Teledyski non-stop', 'Koncert w studiu', 'Nocne granie', 'Teledyski non-stop', 'Lista przebojów', 'Nowości', 'Teledyski non-stop'],
  3: ['Zapowiedzi', 'Rejs (1970)', 'Miś (1981)', 'Zapowiedzi', 'Seksmisja (1984)', 'Vabank (1981)', 'Zapowiedzi', 'Kingsajz (1987)'],
};
function items(pid, n = 8) {
  const titles = TITLES[pid] || [];
  const out = []; let t = now - 12 * 60000;
  for (let i = 0; i < n; i++) {
    const len = pid === 3 ? (i % 3 === 0 ? 3 : 95) : pid === 1 ? [30, 5, 25][i % 3] : 20;
    if (pid === 3 && i === 4) t += 7 * 60000; // dziura w ramówce (K9)
    out.push({ Title: titles[i % titles.length], Start: iso(t), Finish: iso(t + len * 60000), Duration: `${String(Math.floor(len / 60)).padStart(2, '0')}:${String(len % 60).padStart(2, '0')}:00`, SchedulingContext: '', FillerKind: titles[i % titles.length] === 'Zapowiedzi' ? 'PreRoll' : null });
    t += len * 60000;
  }
  return out;
}
const probe = {
  Capability: {
    accels: [
      { accel: 'cuda', configured: true, device: null, driver: null, status: 'Available', error: null, decode: [{ format: 'h264', pixel_formats: ['yuv420p'] }, { format: 'hevc', pixel_formats: ['yuv420p', 'yuv420p10le'] }], encode: [{ format: 'h264', bit_depths: [8] }, { format: 'hevc', bit_depths: [8, 10] }] },
      { accel: 'vaapi', configured: true, device: '/dev/dri/renderD128', driver: 'ihd', status: 'ProbeFailed', error: 'vaInitialize failed with error code -1', decode: [], encode: [] },
      { accel: 'qsv', configured: false, device: null, driver: null, status: 'Skipped', error: null, decode: [], encode: [] },
    ],
    ffmpeg: { hwaccels: ['cuda', 'vaapi', 'qsv'], path: '/usr/local/bin/ffmpeg', version: '7.1.1' },
  },
  Json: '{}', FFmpegPath: '/usr/local/bin/ffmpeg', ProbedAt: iso(now - 3600e3),
};
if (process.env.MOCK_NO_HW) for (const a of probe.Capability.accels) Object.assign(a, { status: a.configured ? 'ProbeFailed' : 'Skipped', error: a.configured ? 'no device' : null, decode: [], encode: [] });
const VIDEOS = ['Serwis 06:00', 'Serwis 12:00', 'Pogoda', 'Pogoda', 'Rozmowa dnia', 'Magazyn regionalny', 'Gość VizRS'].map((Title, i) => ({ MediaItemId: 200 + i, Title, Subtitle: `/media/info24/${Title.toLowerCase().replace(/\W+/g, '-')}${i === 3 ? '-weekend' : ''}.mp4`, SortTitle: Title, Poster: '', State: 'Normal' }));
const PLAYLISTS = [{ Id: 1, PlaylistGroupId: 1, Name: 'Poranek INFO24', IsSystem: false }];
const PL_ITEMS = {};
const LOGS = []; // wpisy dopisywane przez zadania w tle atrapy (K6)
const logs = () => [...LOGS,
  { Timestamp: iso(now - 20e3), Level: 'Information', Message: 'Checking for overlapping playout items in 3 playouts', Properties: '' },
  { Timestamp: iso(now - 18e3), Level: 'Warning', Message: 'Playout for channel Kino Klasyka has overlapping playout items; this may be a bug.', Properties: '' },
  { Timestamp: iso(now - 60e3), Level: 'Information', Message: 'Building playout 1 for channel 24 - INFO24', Properties: '' },
];

const H = {
  GetAllChannels: () => DB.channels,
  GetChannelById: (a) => DB.channels.find((c) => c.Id === a.Id) || null,
  GetAllChannelsForSort: () => DB.channels.map((c) => ({ Id: c.Id, Number: c.Number, Name: c.Name, OriginalNumber: c.Number, HasChanged: false })),
  UpdateChannelNumbers: (a) => { for (const x of a.Channels || []) { const c = DB.channels.find((y) => y.Id === x.Id); if (c) c.Number = x.Number; } return null; },
  GetAllFFmpegProfiles: () => DB.profiles,
  GetChannelFramerate: (a) => { const ch = DB.channels.find((c) => c.Number === a.ChannelNumber); const r = ch && FRAME_RATES[ch.FFmpegProfileId]; return r ? { FrameRateString: r, RFrameRate: r } : null; },
  NewFFmpegProfile: () => ({ ...profile(0, 'Nowy profil', 0, 'None'), Id: 0 }),
  GetAllResolutions: () => DB.resolutions,
  CreateCustomResolution: (a) => { DB.resolutions.push({ Id: Math.max(...DB.resolutions.map((r) => r.Id)) + 1, Name: `${a.Width}x${a.Height}`, Width: a.Width, Height: a.Height, IsCustom: true }); return null; },
  DeleteCustomResolution: (a) => { DB.resolutions = DB.resolutions.filter((r) => r.Id !== a.ResolutionId); return null; },
  GetSupportedHardwareAccelerationKinds: () => ['None', 'Nvenc', 'Vaapi', 'Qsv'],
  GetNextVersion: () => '0.2.5',
  GetNextDescribe: () => ({ binary: { version: '0.2.5', commit: 'c5cb065' }, describe_version: 1, contract: { playout_schema: { breaking: 2, compatible: 3, uri: '' }, channel_config_schema: { breaking: 1, compatible: 4, uri: '' }, segment_seconds: 4, keyframe_seconds: 2 }, schema_capability: { video_formats: ['h264', 'hevc', 'mpeg2video'], audio_formats: ['aac', 'ac3'], accels: ['cuda', 'qsv', 'vaapi', 'videotoolbox', 'amf', 'rkmpp'], subtitle_modes: ['burn', 'none'], audio_copy_formats: [] } }),
  GetNextProbe: () => probe,
  RefreshNextProbe: () => probe,
  GetAllWatermarks: () => DB.watermarks,
  // kolekcje inteligentne: jedna audycji LIVE (prowadzi ją CG Playout – w panelu tylko do odczytu) i jedna zwykła
  GetAllSmartCollections: () => [{ Id: 1, Name: 'LIVE · Koncert w studiu', Query: 'type:remote_stream AND sort_title:"koncert w studiu"' }, { Id: 2, Name: 'Archiwum INFO24', Query: 'type:other_video AND tag:info' }],
  GetAllFillerPresets: () => DB.fillers,
  GetPagedPlayouts: () => ({ TotalCount: DB.playouts.length, Page: DB.playouts }),
  GetAllBlockPlayouts: () => DB.playouts.filter((p) => p.ScheduleKind === 'Block'),
  GetFuturePlayoutItemsById: (a) => { const it = items(a.PlayoutId, a.PageSize || 8); return { TotalCount: it.length, Page: it }; },
  GetPlayoutWarningsCount: () => 1,
  GetRecentLogEntries: (a) => { const l = logs().filter((e) => !a.Filter || e.Message.toLowerCase().includes(String(a.Filter).toLowerCase())); return { TotalCount: l.length, Page: l }; },
  GetAllHealthCheckResults: () => [
    { Title: 'FFmpeg Version', Status: 'Pass', Message: 'FFmpeg 7.1.1', BriefMessage: 'FFmpeg 7.1.1' },
    { Title: 'Hardware Acceleration', Status: 'Warning', Message: 'VAAPI probe failed', BriefMessage: 'VAAPI: test nieudany' },
    { Title: 'Zero Duration', Status: 'Pass', Message: 'OK', BriefMessage: 'OK' },
  ],
  GetTroubleshootingInfo: () => ({ Version: 'v26.12.0', FFmpegVersion: '7.1.1', NextVersion: '0.2.5', Cpus: [{ Manufacturer: 'ARM', Name: 'Neoverse-N1' }], VideoControllers: [], Channels: DB.channels, FFmpegProfiles: DB.profiles, Watermarks: DB.watermarks }),
  GetAllLocalLibraries: () => [{ Id: 1, Name: 'Archiwum INFO24', MediaKind: 'OtherVideos' }, { Id: 2, Name: 'Plansze i grafiki', MediaKind: 'Images' }, { Id: 3, Name: 'Seriale', MediaKind: 'Shows' }],
  GetLocalLibraryPaths: (a) => [{ Id: a.LocalLibraryId * 10, Path: ['', '/media/info24', '/media/plansze', '/media/seriale'][a.LocalLibraryId] || '/media', LibraryId: a.LocalLibraryId }],
  CountMediaItemsByLibrary: (a) => [0, 412, 36, 128][a.LibraryId] ?? 0,
  CountMediaItemsByLibraryPath: (a) => [0, 412, 36, 128][a.LibraryPathId / 10] ?? 0,
  GetImageFolders: (a) => (a.LibraryFolderId == null
    ? [{ LibraryFolderId: 21, Name: 'plansze', FullPath: '/media/plansze', SubfolderCount: 2, ImageCount: 12, DurationSeconds: 10 }, { LibraryFolderId: 22, Name: 'pogoda', FullPath: '/media/pogoda', SubfolderCount: 0, ImageCount: 24, DurationSeconds: null }]
    : [{ LibraryFolderId: 31, Name: 'rano', FullPath: '/media/plansze/rano', SubfolderCount: 0, ImageCount: 6, DurationSeconds: 15 }, { LibraryFolderId: 32, Name: 'wieczor', FullPath: '/media/plansze/wieczor', SubfolderCount: 0, ImageCount: 6, DurationSeconds: null }]),
  UpdateImageFolderDuration: (a) => a.ImageFolderDuration,
  QuerySearchIndexAllItems: (a) => (/FileNotFound/.test(a.Query || '') ? { OtherVideoIds: [9], MovieIds: [] } : { OtherVideoIds: [1, 2, 3], ShowIds: [101, 102], MovieIds: [], SeasonIds: [], EpisodeIds: [], MusicVideoIds: [], ArtistIds: [], SongIds: [], ImageIds: [5, 6], RemoteStreamIds: [] }),
  QuerySearchIndexShows: () => ({ Count: 2, Cards: [{ TelevisionShowId: 101, MediaItemId: 101, Title: 'Kryminalni', Subtitle: '2004', SortTitle: 'kryminalni', Poster: '', State: 'Normal' }, { TelevisionShowId: 102, MediaItemId: 102, Title: 'Ranczo', Subtitle: '2006', SortTitle: 'ranczo', Poster: '', State: 'Normal' }] }),
  QuerySearchIndexOtherVideos: (a) => (/title:"/.test(a.Query || '') ? (() => { const t = /title:"([^"]*)"/.exec(a.Query)[1].trim().toLowerCase(); const c = VIDEOS.filter((v) => v.Title.toLowerCase() === t || v.Title.toLowerCase().includes(t)); return { Count: c.length, Cards: c }; })() : /FileNotFound/.test(a.Query || '') ? { Count: 1, Cards: [{ MediaItemId: 9, Title: 'Seksmisja (1984)', Subtitle: '/media/kino/seksmisja.mkv', State: 'FileNotFound' }] } : { Count: 3, Cards: [['Serwis 06:00', 1], ['Pogoda – plansza', 2], ['Rozmowa dnia – zapowiedź', 3]].map(([Title, MediaItemId]) => ({ MediaItemId, Title, Subtitle: `/media/info24/${MediaItemId}.mp4`, SortTitle: Title, Poster: '', State: 'Normal' })) }),
  GetMediaItemInfo: (a) => ({ Id: a.Id, Title: a.Id > 100 ? 'Kryminalni' : 'Serwis 06:00', Kind: a.Id > 100 ? 'Show' : 'OtherVideo', LibraryName: a.Id > 100 ? 'Seriale' : 'Archiwum INFO24', State: 'Normal', Duration: '00:25:00', Width: 1920, Height: 1080, DisplayAspectRatio: '16:9', RFrameRate: '25/1', Streams: [{ Index: 0, Kind: 'Video', Codec: 'h264', Profile: 'High' }, { Index: 1, Kind: 'Audio', Codec: 'aac', Language: 'pol', Channels: 2, Default: true }, { Index: 2, Kind: 'Subtitle', Codec: 'subrip', Language: 'pol' }], Chapters: [] }),
  GetTelevisionShowById: (a) => ({ Id: a.Id, LibraryId: 3, Title: 'Kryminalni', Year: '2004' }),
  QueueShowScanByLibraryId: () => true,
  GetAllPlaylistGroups: () => [{ Id: 1, Name: 'Ramówki INFO24', IsSystem: false, PlaylistCount: PLAYLISTS.length }],
  GetPlaylistsByPlaylistGroupId: () => PLAYLISTS,
  CreatePlaylist: (a) => { const p = { Id: PLAYLISTS.length + 1, PlaylistGroupId: a.PlaylistGroupId, Name: a.Name, IsSystem: false }; PLAYLISTS.push(p); return p; },
  ReplacePlaylistItems: (a) => { PL_ITEMS[a.PlaylistId] = a.Items; return []; },
  GetPlaylistItems: (a) => (PL_ITEMS[a.PlaylistId] || []).map((i) => ({ ...i, MediaItem: { MediaItemId: i.MediaItemId, Title: (VIDEOS.find((v) => v.MediaItemId === i.MediaItemId) || {}).Title } })),
  GetAllBlocksForPlayout: () => [{ Id: 4, GroupId: 1, GroupName: 'Muzyka', Name: 'Teledyski 20 min' }, { Id: 5, GroupId: 1, GroupName: 'Muzyka', Name: 'Lista przebojów' }],
  GetBlockPlayoutHistory: () => ({ TotalCount: 2, Page: [
    { Id: 11, When: iso(now - 3 * 3600e3), Finish: iso(now - 2 * 3600e3), Key: '{"b":4,"i":2}', Details: '{"CollectionType":"Collection","Name":"Teledyski"}' },
    { Id: 12, When: iso(now - 2 * 3600e3), Finish: iso(now - 3600e3), Key: '{"b":4,"i":3}', Details: '{"CollectionType":"Collection","Name":"Teledyski"}' }] }),
  DecodePlayoutHistory: () => ({ PlaybackOrder: 'Shuffle', CollectionType: 'Collection', Name: 'Teledyski', MediaItemType: 'MusicVideo', MediaItemTitle: 'Teledysk 17' }),
};

// ---------- strumienie testowe (MOCK_HLS=1) ----------
const HLS_DIR = path.join(require('os').tmpdir(), `vizrs-mock-hls-${PORT}`);
if (process.env.MOCK_HLS === '1') {
  const { spawn } = require('child_process');
  const SRC = { 24: 'testsrc2', 25: 'smptehdbars', 26: 'mandelbrot' };
  for (const [ch, src] of Object.entries(SRC)) {
    const dir = path.join(HLS_DIR, ch);
    fs.mkdirSync(dir, { recursive: true });
    const ff = spawn('ffmpeg', ['-loglevel', 'error', '-re', '-f', 'lavfi', '-i', `${src}=size=640x360:rate=25`, '-f', 'lavfi', '-i', `sine=frequency=${300 + Number(ch) * 10}`,
      '-c:v', 'libx264', '-preset', 'ultrafast', '-tune', 'zerolatency', '-g', '50', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '96k',
      '-f', 'hls', '-hls_time', '2', '-hls_list_size', '6', '-hls_flags', 'delete_segments+program_date_time',
      '-hls_base_url', `http://127.0.0.1:${PORT}/iptv/session/${ch}/`, path.join(dir, 'live.m3u8')], { stdio: 'inherit' });
    process.on('exit', () => ff.kill());
  }
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => process.exit(0));
}
function serveFile(res, file, type) {
  fs.readFile(file, (e, b) => {
    if (e) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { 'Content-Type': type, 'Content-Length': b.length });
    res.end(b);
  });
}

function reply(res, status, obj) {
  const b = Buffer.from(JSON.stringify(obj));
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Content-Length': b.length });
  res.end(b);
}
http.createServer((req, res) => {
  const u = new URL(req.url, 'http://x');
  let m = u.pathname.match(/^\/iptv\/channel\/(\d+)\.m3u8$/);
  if (m) return serveFile(res, path.join(HLS_DIR, m[1], 'live.m3u8'), 'application/vnd.apple.mpegurl');
  m = u.pathname.match(/^\/iptv\/session\/(\d+)\/([\w-]+\.(ts|m3u8))$/);
  if (m) return serveFile(res, path.join(HLS_DIR, m[1], m[2]), m[3] === 'ts' ? 'video/mp2t' : 'application/vnd.apple.mpegurl');
  if (req.headers['x-vizrs-token'] !== TOKEN && u.pathname.startsWith('/vizrs-bridge/')) return reply(res, 401, { ok: false, error: 'zły token mostu' });
  let body = '';
  req.on('data', (d) => (body += d));
  req.on('end', () => {
    const p = u.pathname.replace(/^\/vizrs-bridge/, '');
    if (p === '/health') return reply(res, 200, { ok: true, result: { bridge: 'atrapa', legacy: 'v26.12.0 (atrapa)', commandsAllowed: KNOWN.size, commandsMissing: allowed.filter((n) => !KNOWN.has(n)) } });
    if (p === '/commands') return reply(res, 200, { ok: true, result: META });
    if (p === '/sessions') return reply(res, 200, { ok: true, result: [{ ChannelNumber: '24', State: 'Running', TranscodedUntil: iso(now + 60e3), LastAccess: iso(now), StartedAt: iso(now - 3600e3) }] });
    if (p.startsWith('/session/')) return reply(res, 200, { ok: true, result: true });
    if (p === '/framerates') return reply(res, 200, { ok: true, result: { active: true, problem: null, rates: FRAME_RATES } });
    if (p.startsWith('/framerate/')) {
      let rate = null;
      try { rate = JSON.parse(body || '{}').rate ?? null; } catch { return reply(res, 400, { ok: false, error: 'zły JSON' }); }
      const id = p.slice('/framerate/'.length);
      if (rate == null) delete FRAME_RATES[id]; else FRAME_RATES[id] = rate;
      return reply(res, 200, { ok: true, result: { active: true, problem: null, rates: FRAME_RATES } });
    }
    if (p.startsWith('/command/')) {
      const name = p.slice('/command/'.length);
      if (!allowed.includes(name)) return reply(res, 403, { ok: false, error: `komenda '${name}' nie jest dozwolona (biała lista mostu)` });
      let args = {};
      try { args = body ? JSON.parse(body) : {}; } catch { return reply(res, 400, { ok: false, error: 'zły JSON' }); }
      if (name === 'CheckForOverlappingPlayoutItems' && args.PlayoutId === 3) setTimeout(() => LOGS.unshift({ Timestamp: iso(Date.now()), Level: 'Warning', Message: 'Playout for channel Kino Klasyka has overlapping playout items; this may be a bug.' }), 1500);
      if (name === 'ReleaseMemory') setTimeout(() => LOGS.unshift({ Timestamp: iso(Date.now()), Level: 'Debug', Message: 'Completed garbage collection' }), 1000);
      if (/^(Build|Queue|Extract|Check|Refresh(?!NextProbe)|EmptyTrash|DeleteOrphaned|ReleaseMemory|Synchronize)/.test(name)) return reply(res, 202, { ok: true, queued: 'atrapa' });
      const fn = H[name];
      const result = fn ? fn(args) : /^(Get|Query|Search)/.test(name) ? [] : /^Count/.test(name) ? 0 : null;
      return reply(res, 200, { ok: true, result });
    }
    reply(res, 404, { ok: false, error: 'nieznany adres mostu' });
  });
}).listen(PORT, '127.0.0.1', () => console.log(`atrapa mostu: http://127.0.0.1:${PORT}/vizrs-bridge (${META.length} komend z metadanymi${process.env.LEGACY_SRC ? '' : ' – bez LEGACY_SRC'})`));

// ---------- atrapa VizRS CG Playout (MOCK_CG=1) – kontrakt K12–K17 (DOKUMENTACJA §3) ----------
if (process.env.MOCK_CG === '1') {
  const CGT = 'test-cg-token-0123456789';
  const layers = { 24: [{ layer: 10, page: '101', name: 'belka 101', scene: 'belka', since: now - 300e3 }, { layer: 20, page: '4001', name: 'zegar', scene: 'zegar', since: now - 3600e3 }], 25: [{ layer: 10, page: '201', name: 'teraz gramy', since: now - 120e3 }], 26: [] };
  const asrun = [];
  let encoder = { preset: 'veryfast', profile: '', x264_params: '', nvenc_preset: '', qsv_preset: '', bframes: '0' }; // koder nakładki (GET/PUT /api/live/encoder)
  const log = (e) => asrun.push({ t: Date.now(), operator: 'VizRS Live · atrapa', ...e });
  for (let i = 0; i < 8; i++) { const it = items(1, 8)[i]; asrun.push({ t: +new Date(it.Start), kind: 'item_start', title: it.Title }); if (i % 3 === 0) asrun.push({ t: +new Date(it.Start) + 10e3, kind: 'take', page: '101', layer: 10, title: it.Title, operator: 'reguła: on_clip' }); }
  asrun.push({ t: now - 1800e3, kind: 'hold', operator: 'VizRS Live · MCR 1' }, { t: now - 1700e3, kind: 'release', operator: 'VizRS Live · MCR 1' });
  http.createServer((q, r) => {
    const u = new URL(q.url, 'http://x');
    let b = '';
    q.on('data', (d) => (b += d));
    q.on('end', () => {
      if (q.headers.authorization !== 'Bearer ' + CGT) return reply(r, 401, { error: 'zły token panelu' });
      if (u.pathname === '/health') return reply(r, 200, { sessions: 3, overlay: ['24'], ingest: { channels: ['24'] }, captures: [] });
      if (u.pathname === '/api/live/state') return reply(r, 200, {
        ingest: true, now: Date.now(), version: 'atrapa', features: ['layers', 'take', 'asrun', 'encoder', 'thumbs', 'webhook', 'logs', 'on_live'], settings: { graphics_off: false },
        channels: DB.channels.slice(0, 3).map((c) => ({ number: c.Number, name: c.Name, source: { live: c.Number === '24' }, on_air: c.Number === '24', hold: false, takeover: c.Number === '24', anytime: c.Number === '24', layers: layers[c.Number] || [],
          next_live: c.Number === '25' ? { start: Date.now() + 3 * 60e3, title: 'Koncert w studiu' } : null })),
        events: [], guide: {},
      });
      if (u.pathname === '/api/live/encoder') {
        if (q.method === 'PUT') { const v = JSON.parse(b || '{}'); encoder = Object.fromEntries(Object.keys(encoder).map((k) => [k, String(v[k] ?? '')])); }
        return reply(r, 200, { used: encoder, from: 'panel', store: true, presets: ['ultrafast', 'superfast', 'veryfast', 'faster', 'fast', 'medium', 'slow', 'slower', 'veryslow', 'placebo'],
          profiles: ['baseline', 'main', 'high'], x264_keys: ['bframes', 'subme', 'ref', 'me', 'trellis'], nvenc_presets: ['p1', 'p2', 'p3', 'p4', 'p5', 'p6', 'p7'], qsv_presets: ['veryfast', 'faster', 'fast', 'medium', 'slow', 'slower', 'veryslow'], max_bframes: 4 });
      }
      if (u.pathname === '/api/live/inputs') return reply(r, 200, { listeners: [{ channel: '24', url: 'rtmp://0.0.0.0:1936/live/x' }] });
      if (u.pathname === '/etv/pages') return reply(r, 200, { pages: [{ number: '101', name: 'belka tytułowa' }, { number: '102', name: 'za chwilę' }, { number: '4001', name: 'zegar' }] });
      if (u.pathname === '/api/live/asrun') return reply(r, 200, { entries: asrun });
      if (u.pathname === '/api/live/logs') {
        const n = Date.now(), src = u.searchParams.get('source');
        const lines = { ingest: [[n - 900e3, 'info', 'channel 24: publisher on rtmp://0.0.0.0:1936/live/***'], [n - 600e3, 'warn', 'channel 24: publisher gone; listening on rtmp://0.0.0.0:1936/live/***']],
          takeover: [[n - 880e3, 'info', 'channel 24: live run ended after 44.0 s in 43.6 s (1.01x, exit 0) | seq 120, listed 11'], [n - 590e3, 'warn', "channel 24: live leaves the channel's timeline 11.2 s behind the clock; the schedule makes it up"]],
          cg: [[n - 899e3, 'info', 'ingest: channel 24: live (h264, with audio)'], [n - 500e3, 'warn', 'graphics stream: the air time is +4.2 s off the end of the run before; going on from its end']] }[src] || [];
        return reply(r, 200, { tz: 'UTC', truncated: false, entries: lines.map(([t, level, text]) => ({ t, level, text })) });
      }
      if (u.pathname === '/api/live/graphics') return reply(r, 200, { rules: { on_clip: 'TAKE 101 headline={title}', on_ad: '', ad_match: { path: '/reklamy/', title: '' }, on_end: 'TAKE 102 next={next_title}', end_seconds: 30, shows: [], channels: { 24: { on_live: 'TAKE 4001' } } }, source: 'config', store: true });
      if (u.pathname === '/api/live/shows') return reply(r, 200, { shows: [{ id: '25-koncert', channel: '25', title: 'Koncert w studiu', duration: '01:00:00', plot: '', collection: true, collection_name: 'LIVE · Koncert w studiu', next: null, file_ok: true, airings: 2 }], error: null,
        channels: DB.channels.slice(0, 3).map((c) => ({ number: c.Number, name: c.Name })), guide_known: true, max: 50, etv: { configured: true, ok: true, error: null }, settings: { library: null, store: true } });
      const m = u.pathname.match(/^\/api\/live\/channels\/(\w+)\/(take|continue|out)$/);
      if (m && q.method === 'POST') {
        const a = b ? JSON.parse(b) : {};
        const L = (layers[m[1]] ||= []);
        if (m[2] === 'take') { const l = { layer: 30, page: String(a.page), name: 'strona ' + a.page, manual: true, since: Date.now() }; L.splice(0, L.length, ...L.filter((x) => x.layer !== 30), l); log({ kind: 'take', page: a.page, layer: 30 }); }
        if (m[2] === 'out') { const i = L.findIndex((x) => x.layer === a.layer); if (i >= 0) { log({ kind: 'out', page: L[i].page, layer: a.layer }); L.splice(i, 1); } }
        if (m[2] === 'continue') log({ kind: 'continue', layer: a.layer });
        return reply(r, 200, { ok: true });
      }
      reply(r, 404, { error: 'nieznany adres atrapy CG' });
    });
  }).listen(PORT + 1, '127.0.0.1', () => console.log(`atrapa CG Playout: http://127.0.0.1:${PORT + 1}`));
}
