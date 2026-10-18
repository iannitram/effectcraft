# Goal 5 — Performance: benchmarks at 1080p / 4K, real-time cached playback, hardware video (G5)

*Machine-drafted by Qwen 2026-10-16, reviewed and corrected by Claude 2026-10-17.*

## Goal

Ship measurable performance at 1080p and 4K resolutions, real-time cached playback with
dropped-frame awareness, and hardware-accelerated video decode/encode — so previews and exports
hold frame rate at full resolution on typical desktop hardware.

## What exists

### Render benchmark (`xtask/src/bench.rs`)
- Generates deterministic projects (`effectcraft-cli`) and renders them with `/usr/bin/time`.
- Records wall time, peak memory (RSS), and instructions retired to a JSONL results file.
- `--compare OLD.jsonl` prints % change; `--fail-over PCT` exits 1 on regressions.
- Scenarios: `demo` (built-in comp), `layers` (1080p, 100 layers with effects + keyframes),
  `precomps` (nested 3 deep), `footage` (PNG sequence, H.264 clip, 4K still).
- Each renders at Full, Half, and one frame (`frame`); `open` measures project load cost.

### Engine ops benchmark (`crates/engine/src/perf.rs`)
- `effectcraft-cli bench --ops` measures: open, save, auto-save, large edits with undo/redo,
  first frame. Uses `LargeSpec` (default: 200 comps, 5 000 main layers, 300 footage items,
  20-level precomp nesting, expressions every 4th layer).

### Layer render cache (`crates/render/src/cache.rs`)
- `LayerCache`: LRU, memory-budgeted (`budget`/`set_budget`), keyed by content hash.
- `CacheStats`: entries, bytes, hits, misses — exposed for perf readouts and tests.
- `set_gate`: drops inserts while GPU readbacks are in flight (deferred browser path).
- `set_disk` / `set_store`: optional disk cache or browser Origin Private File System store.

### Disk cache (`crates/render/src/disk_cache.rs`)
- `DiskCache`: persistent, LZ4-compressed, atomic writes (temp file + rename), LRU eviction.
- `DiskStats`: entries, bytes, max_bytes, hits, misses, writes, dropped.
- `LayerPrefetch`: remembers layer-cache misses per frame (up to 512), plans disk fetches
  before the next frame.
- Write queue bounded to 64 entries; a full queue drops writes rather than stalling rendering.

### Media pool (`crates/media/src/pool.rs`)
- `MediaPool`: LRU frame cache with memory budgeting.
- Sequential playback prefetch: reads one frame ahead on a worker thread (native builds).
- `PoolStats`: entries, bytes, prefetched count.
- `set_read_ahead`: enable/disable prefetch during playback.
- `frame_at(&self, footage: &Footage, t: Tick) -> Result<Arc<Image>>`: cache-aware frame lookup (no playback flag in the public signature).

### GPU compositor (`crates/gpu/src/lib.rs`)
- wgpu compute shaders (Metal, Vulkan, D3D12, WebGPU) for compositing.
- Layer content rendered on CPU; GPU handles transforms, motion blur, track mattes, blend modes,
  3D (classic + advanced), GPU effects via WGSL.
- `Gpu::render_display`: skips readback entirely for egui-wgpu viewer.
- `Gpu::read_display` / `read_display_async`: RGBA8 readback for export.

### Auto pick (`crates/render/src/auto.rs`)
- Per-comp CPU/GPU timing history (exponentially weighted average).
- `AutoPick::choose`: renders on whichever measured faster for the comp/scale/path.
- Warm-up (3 frames), re-probe every 48 frames, margin of 0.85× for CPU preference.

## What's missing

### 1. 1080p / 4K benchmark scenarios (G5 — benchmarks)
Existing benchmarks render at Full and Half. Full is the comp's native size, so the 1080p comps
(`layers`, `precomps`, `footage`) already measure 1080p. There is no 4K comp or footage-clip
scenario: the only 4K item is the still layered into `footage` (`xtask/src/bench.rs`).

**Gap:** `xtask/src/bench.rs` needs new scenarios:
- `layers_4k`: 4K comp with N solid/text/shape layers + effects + keyframes.
- `footage_4k`: 4K footage clips on multiple layers (H.264, ProRes, PNG sequence).
- `precomps_4k`: nested precomps at 4K resolution.
- Each scenario renders at Full, Half and one frame like the existing ones (Full is native 4K).

**Small first step:** Add a `--resolution` flag to `cargo xtask bench` that overrides the comp
dimensions for existing scenarios (e.g., `--resolution 4k` sets 3840×2160).

### 2. Real-time cached playback metrics (G5 — playback)
The `LayerCache` tracks hits/misses and `DiskCache` tracks dropped writes, but there is no
aggregate metric for real-time playback compliance. We need:

**Gap:** A `PlaybackStats` struct (or extension to existing stats) that tracks:
- Frames rendered on time vs. frames dropped (missed the frame budget).
- Cache hit rate during playback (RAM hit / disk hit / miss).
- Prefetch effectiveness (frames prefetched vs. frames needed).
- GPU vs. CPU path selection frequency (from `AutoPick`).

**Small first step:** Add a `PlaybackStats` to `crates/render/src/cache.rs` or a new
`playback.rs` module. It collects per-frame timing from the render pipeline
and flags frames that exceed the frame budget (1000ms / fps). Wire it through the viewer's
playback loop. Expose via `cache.playbackStats` in the control protocol.

### 3. Hardware video decode/encode (G5 — hardware video, gaps row 15)
Gaps row 15 marks it missing: no VideoToolbox (macOS), NVENC (NVIDIA), Quick Sync
(Intel), or VA-API (Linux) implementations. All video decode/encode runs through the pure-Rust
CPU codecs (FilmCraft crates behind `crates/media`; ffmpeg is only a test oracle). CUDA / NVENC
need Nvidia's proprietary SDK through FFI, which the no-`unsafe` rule forbids (#498). Open PR #819
adds hardware decoding via `filmcraft-platform`; review it before starting here.

**Gap:** Hardware decode for common codecs (H.264, HEVC, VP9, AV1) and hardware encode for
export (H.264, HEVC). This is a large effort spanning:
- `crates/media/src/` — hardware decode backends (platform-specific modules).
- `crates/export/` or `crates/render/` — hardware encode backends.
- Fallback to CPU decode when hardware is unavailable (never crash).

**Small first step:** Add a `HardwareVideo` probe module that detects available hardware
acceleration on the platform (query adapters, check codec support) without implementing any
decode/encode. Expose via `media.hardwareVideo` in the control protocol.
This is a pure readout — no decoding changes — and informs the user and the auto-pick system.

### 4. Cache miss rate during playback (G5 — cached playback)
`CacheStats` gives total hits/misses across all time, not scoped to a playback session. We need
per-session cache metrics to answer: "did the cache help during this preview?"

**Gap:** Add `CacheStats::session_reset()` or a `SessionStats` that captures hits/misses at
playback start and compares against end. Compute a hit rate percentage.

**Small first step:** Add `CacheStats::since(&self, epoch: u64) -> CacheStats` that returns
hits/misses since the given epoch. Call `epoch = stats.hits + stats.misses` at playback start.

## Small first steps

1. **Add `--resolution` flag to `cargo xtask bench`** (`xtask/src/bench.rs`):
   - Accept `1080p` or `4k`; override comp dimensions for all scenarios.
    - Output results to separate JSONL files per resolution.
   - ~50 lines of code in the scenario generation path.

2. **Add `PlaybackStats` struct** (`crates/render/src/cache.rs` or a new `playback.rs` module):
   - Fields: `frames_on_time: u64`, `frames_dropped: u64`, `frames_rendered: u64`.
   - Method `record_frame(&mut self, took_ms: f64, budget_ms: f64)`.
   - Method `hit_rate(&self) -> f64` (reads from `LayerCache::stats()`).
   - Expose via `cache.playbackStats` in control protocol.

3. **Add `HardwareVideo::probe()`** (new `hw_probe.rs` module in `crates/media/src/`):
   - Detect platform (macOS/Windows/Linux).
   - Query wgpu adapters for vendor/features (Metal, Vulkan, D3D12).
   - Return `HardwareVideoInfo { platform, has_hardware: bool, codecs: Vec<String> }`.
   - ~80 lines, no decode changes.

4. **Add `CacheStats::since()`** (`crates/render/src/cache.rs`):
   - Takes an epoch (u64 = hits + misses at a point in time).
   - Returns new `CacheStats` with hits/misses since that epoch.
   - ~10 lines.
